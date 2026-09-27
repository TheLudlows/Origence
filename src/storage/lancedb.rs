//! LanceDB vector backend (M4).
//!
//! One LanceDB table per embedding `profile`, named `vec_` + the first 16
//! bytes of SHA-256(profile). Each profile's vectors share a fixed dimension,
//! so `nearest_to` can run on a `FixedSizeList`. Writes are idempotent by
//! `(tenant, workspace, id)` via `merge_insert`; the deterministic artifact id
//! makes replay target the same row (A2.6).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use arrow_array::{
    types::Float32Type, Array, FixedSizeListArray, Float32Array, Int32Array, Int64Array,
    RecordBatch, RecordBatchIterator, StringArray,
};
use arrow_schema::{DataType, Field, Schema};
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::storage::capabilities::{Capabilities, VectorEntry, VectorHit, VectorQuery};
use crate::storage::error::{StorageError, StorageResult};
use crate::storage::scope::{Scope, SourceVersion};
use crate::storage::traits::{Lifecycle, VectorStore};

/// A vector backend rooted at a LanceDB directory.
pub struct LanceDbStore {
    db: lancedb::Connection,
}

fn table_name(profile: &str) -> String {
    let digest = Sha256::digest(profile.as_bytes());
    let mut s = String::with_capacity(4 + 32);
    s.push_str("vec_");
    for b in &digest[..16] {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn quote(s: &str) -> String {
    s.replace('\'', "''")
}

fn schema_for(dim: usize) -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("tenant", DataType::Utf8, false),
        Field::new("workspace", DataType::Utf8, false),
        Field::new("id", DataType::Utf8, false),
        Field::new("source_id", DataType::Utf8, false),
        Field::new("version", DataType::Int32, false),
        Field::new("generation", DataType::Int64, false),
        Field::new(
            "vector",
            DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), dim as i32),
            true,
        ),
    ]))
}

impl LanceDbStore {
    /// Open the store rooted at `dir`, creating the directory if absent.
    pub async fn open(dir: impl AsRef<Path>) -> StorageResult<Self> {
        let dir = dir.as_ref().to_path_buf();
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir
            .to_str()
            .ok_or_else(|| StorageError::Unavailable("non-UTF8 vector path".into()))?;
        let db = lancedb::connect(path)
            .read_consistency_interval(Duration::ZERO)
            .execute()
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        Ok(Self { db })
    }

    async fn ensure_table(&self, name: &str, dim: usize) -> StorageResult<()> {
        let names = self
            .db
            .table_names()
            .execute()
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        if !names.iter().any(|n| n == name) {
            self.db
                .create_empty_table(name, schema_for(dim))
                .execute()
                .await
                .map_err(|e| StorageError::Backend(e.to_string()))?;
        }
        Ok(())
    }
}

impl VectorStore for LanceDbStore {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            filtered_ann: false,
            exact_search: true,
            max_hops: 0,
            shared_source_delete: false,
        }
    }

    async fn upsert(&self, scope: Scope, entries: Vec<VectorEntry>) -> StorageResult<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut groups: HashMap<String, Vec<&VectorEntry>> = HashMap::new();
        for e in &entries {
            groups.entry(e.profile.clone()).or_default().push(e);
        }
        for (profile, group) in groups {
            let dim = group[0].dimension;
            if group.iter().any(|e| e.dimension != dim) {
                return Err(StorageError::Conflict(
                    "mixed dimensions within one profile batch".into(),
                ));
            }
            let name = table_name(&profile);
            self.ensure_table(&name, dim).await?;
            let table = self
                .db
                .open_table(&name)
                .execute()
                .await
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            let schema = schema_for(dim);
            let tenants: Vec<String> = group
                .iter()
                .map(|_| scope.tenant_id.to_string())
                .collect();
            let workspaces: Vec<String> = group
                .iter()
                .map(|_| scope.workspace_id.to_string())
                .collect();
            let ids: Vec<String> = group.iter().map(|e| e.id.to_string()).collect();
            let source_ids: Vec<String> =
                group.iter().map(|e| e.source.source_id.to_string()).collect();
            let versions: Vec<i32> = group.iter().map(|e| e.source.version).collect();
            let generations: Vec<i64> = group.iter().map(|e| e.generation).collect();
            let vectors: Vec<Option<Vec<Option<f32>>>> = group
                .iter()
                .map(|e| Some(e.embedding.iter().map(|&x| Some(x)).collect()))
                .collect();
            let batch = RecordBatch::try_new(
                schema.clone(),
                vec![
                    Arc::new(StringArray::from(tenants)),
                    Arc::new(StringArray::from(workspaces)),
                    Arc::new(StringArray::from(ids)),
                    Arc::new(StringArray::from(source_ids)),
                    Arc::new(Int32Array::from(versions)),
                    Arc::new(Int64Array::from(generations)),
                    Arc::new(FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
                        vectors,
                        dim as i32,
                    )),
                ],
            )
            .map_err(|e| StorageError::Backend(e.to_string()))?;
            let reader = RecordBatchIterator::new(vec![Ok(batch)], schema.clone());
            let mut merge = table.merge_insert(&["tenant", "workspace", "id"]);
            merge.when_matched_update_all(None);
            merge.when_not_matched_insert_all();
            merge
                .execute(Box::new(reader))
                .await
                .map_err(|e| StorageError::Backend(e.to_string()))?;
        }
        Ok(())
    }

    async fn search(&self, query: VectorQuery) -> StorageResult<Vec<VectorHit>> {
        let name = table_name(&query.profile);
        let names = self
            .db
            .table_names()
            .execute()
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        if !names.iter().any(|n| n == &name) {
            return Ok(Vec::new());
        }
        let table = self
            .db
            .open_table(&name)
            .execute()
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        let filter = format!(
            "tenant='{}' AND workspace='{}' AND generation={}",
            quote(&query.scope.tenant_id.to_string()),
            quote(&query.scope.workspace_id.to_string()),
            query.generation
        );
        let stream = table
            .query()
            .nearest_to(query.embedding.as_slice())
            .map_err(|e| StorageError::Backend(e.to_string()))?
            .only_if(filter)
            .limit(query.limit)
            .execute()
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        let batches: Vec<RecordBatch> = stream
            .try_collect()
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        let mut hits = Vec::new();
        for batch in &batches {
            let ids = batch
                .column_by_name("id")
                .ok_or_else(|| StorageError::Backend("missing id column".into()))?
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| StorageError::Backend("id column type".into()))?;
            let dists = batch
                .column_by_name("_distance")
                .ok_or_else(|| StorageError::Backend("missing _distance column".into()))?
                .as_any()
                .downcast_ref::<Float32Array>()
                .ok_or_else(|| StorageError::Backend("_distance column type".into()))?;
            let srcs = batch
                .column_by_name("source_id")
                .ok_or_else(|| StorageError::Backend("missing source_id column".into()))?
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| StorageError::Backend("source_id column type".into()))?;
            let vers = batch
                .column_by_name("version")
                .ok_or_else(|| StorageError::Backend("missing version column".into()))?
                .as_any()
                .downcast_ref::<Int32Array>()
                .ok_or_else(|| StorageError::Backend("version column type".into()))?;
            for i in 0..ids.len() {
                hits.push(VectorHit {
                    id: Uuid::parse_str(ids.value(i))
                        .map_err(|e| StorageError::Backend(e.to_string()))?,
                    score: 1.0 / (1.0 + dists.value(i)),
                    source: SourceVersion {
                        source_id: Uuid::parse_str(srcs.value(i))
                            .map_err(|e| StorageError::Backend(e.to_string()))?,
                        version: vers.value(i),
                    },
                });
            }
        }
        Ok(hits)
    }

    async fn delete_source(&self, scope: Scope, source: SourceVersion) -> StorageResult<()> {
        let names = self
            .db
            .table_names()
            .execute()
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        for name in names {
            let table = self
                .db
                .open_table(&name)
                .execute()
                .await
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            let filter = format!(
                "tenant='{}' AND workspace='{}' AND source_id='{}' AND version={}",
                quote(&scope.tenant_id.to_string()),
                quote(&scope.workspace_id.to_string()),
                quote(&source.source_id.to_string()),
                source.version
            );
            table
                .delete(&filter)
                .await
                .map_err(|e| StorageError::Backend(e.to_string()))?;
        }
        Ok(())
    }
}

impl Lifecycle for LanceDbStore {
    async fn initialize(&self) -> StorageResult<()> {
        Ok(())
    }

    async fn check(&self) -> StorageResult<()> {
        Ok(())
    }

    async fn shutdown(&self) -> StorageResult<()> {
        Ok(())
    }
}
