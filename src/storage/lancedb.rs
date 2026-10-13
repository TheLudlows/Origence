//! LanceDB derived full-text and vector backend.
//!
//! One LanceDB table per embedding `profile`, named `vec_` + the first 16
//! bytes of SHA-256(profile). Each profile's vectors share a fixed dimension,
//! so `nearest_to` can run on a `FixedSizeList`. Writes are idempotent by
//! `(tenant, workspace, source_id, version, id, generation)` via `merge_insert`;
//! replay targets the same ledger identity without replacing other generations.

mod ann;
mod eligibility;
mod filter_indexes;
mod full_text;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use arrow_array::{
    Array, FixedSizeListArray, Float32Array, Int32Array, Int64Array, RecordBatch,
    RecordBatchIterator, StringArray, types::Float32Type,
};
use arrow_schema::{DataType, Field, Schema};
use futures::TryStreamExt;
use lancedb::database::CreateTableMode;
use lancedb::query::ExecutableQuery;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::storage::capabilities::{
    Capabilities, FullTextEntry, FullTextHit, FullTextQuery, VectorEntry, VectorHit,
    VectorQuery,
};
use crate::storage::error::{StorageError, StorageResult};
use crate::storage::scope::{Scope, SourceVersion};
use crate::storage::traits::{Lifecycle, VectorStore};

/// Derived native indexes rooted at a local LanceDB directory.
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

fn validate_vector(dimension: usize, embedding: &[f32]) -> StorageResult<()> {
    if dimension == 0 || dimension > i32::MAX as usize || embedding.len() != dimension {
        return Err(StorageError::Conflict(
            "vector dimension must be positive, fit i32 and match embedding length".into(),
        ));
    }
    if embedding.iter().any(|value| !value.is_finite()) {
        return Err(StorageError::Conflict(
            "vector values must be finite".into(),
        ));
    }
    Ok(())
}

async fn check_dimension(table: &lancedb::Table, dimension: usize) -> StorageResult<()> {
    let schema = table
        .schema()
        .await
        .map_err(|e| StorageError::Backend(e.to_string()))?;
    if schema.as_ref() == schema_for(dimension).as_ref() {
        Ok(())
    } else {
        Err(StorageError::Conflict(
            "embedding profile schema or dimension does not match table".into(),
        ))
    }
}

pub(super) fn schema_for(dim: usize) -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("tenant", DataType::Utf8, false),
        Field::new("workspace", DataType::Utf8, false),
        Field::new("id", DataType::Utf8, false),
        Field::new("source_id", DataType::Utf8, false),
        Field::new("version", DataType::Int32, false),
        Field::new("generation", DataType::Int64, false),
        Field::new(
            "vector",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                dim as i32,
            ),
            true,
        ),
    ]))
}

/// Recognized pure full-text table, for offline maintenance schema validation.
pub(super) const FULL_TEXT_TABLE: &str = full_text::TABLE_NAME;

pub(super) fn full_text_schema() -> Arc<Schema> {
    full_text::schema_for()
}

fn scoped_filter(scope: Scope, generation: i64, ids: Option<&[Uuid]>) -> String {
    let mut filter = format!(
        "tenant='{}' AND workspace='{}' AND generation={}",
        scope.tenant_id, scope.workspace_id, generation,
    );
    if let Some(ids) = ids {
        let values = ids.iter().map(|id| format!("'{id}'")).collect::<Vec<_>>().join(",");
        filter.push_str(&format!(" AND id IN ({values})"));
    }
    filter
}

fn vector_table_name(name: &str) -> bool {
    name.len() == 36 && name.starts_with("vec_") && name[4..].bytes().all(|b| b.is_ascii_hexdigit())
}

impl LanceDbStore {
    /// Open the store rooted at `dir`, creating the directory if absent.
    pub async fn open(dir: impl AsRef<Path>) -> StorageResult<Self> {
        let dir = dir.as_ref().to_path_buf();
        tokio::fs::create_dir_all(&dir).await?;
        // Absolute filesystem paths cannot be interpreted as remote URIs.
        // Unlike canonicalize, absolute does not add Windows device prefixes.
        // The crate's remote feature is only a 0.40.0 compilation workaround.
        let dir = std::path::absolute(&dir)?;
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

    /// Read-only health gate for relationally committed full-text publications.
    /// Empty/new authorities need not call this until an index is required.
    pub async fn check_full_text_available(&self) -> StorageResult<()> {
        full_text::check_available(self).await
    }

    /// Compare every eligible native identity against the caller's relational
    /// snapshot before top-k. Final live authority verification remains required.
    pub(crate) async fn search_authorized(
        &self,
        query: VectorQuery,
        expected_sources: &HashMap<Uuid, SourceVersion>,
    ) -> StorageResult<Vec<VectorHit>> {
        self.search_inner(query, Some(expected_sources)).await
    }

    pub(crate) async fn search_full_text_authorized(
        &self,
        query: FullTextQuery,
        expected_sources: &HashMap<Uuid, SourceVersion>,
    ) -> StorageResult<Vec<FullTextHit>> {
        full_text::search(self, query, Some(expected_sources)).await
    }

    async fn search_inner(
        &self,
        mut query: VectorQuery,
        expected_sources: Option<&HashMap<Uuid, SourceVersion>>,
    ) -> StorageResult<Vec<VectorHit>> {
        validate_vector(query.dimension, &query.embedding)?;
        if query.limit == 0 || query.artifact_ids.as_ref().is_some_and(Vec::is_empty) {
            return Ok(Vec::new());
        }
        if let Some(ids) = query.artifact_ids.as_mut() {
            ids.sort_unstable();
            ids.dedup();
        }
        let started = std::time::Instant::now();
        let Some(table) = self.open_existing(&table_name(&query.profile)).await? else {
            return if query.artifact_ids.is_some() {
                Err(StorageError::Unavailable("native vector table is missing".into()))
            } else {
                Ok(Vec::new())
            };
        };
        let version = table.version().await
            .map_err(|error| StorageError::Backend(error.to_string()))?;
        table.checkout(version).await
            .map_err(|error| StorageError::Backend(error.to_string()))?;
        let open_ms = started.elapsed().as_secs_f64() * 1000.0;
        let prepare_started = std::time::Instant::now();
        let filter = scoped_filter(query.scope, query.generation, query.artifact_ids.as_deref());
        check_dimension(&table, query.dimension).await?;
        eligibility::check(&table, &filter, query.generation, query.artifact_ids.as_deref(), expected_sources).await?;
        let (native, used_ann) = ann::prepare(&table, &query, &filter).await?;
        let prepare_ms = prepare_started.elapsed().as_secs_f64() * 1000.0;
        let execute_started = std::time::Instant::now();
        let stream = native.execute().await
            .map_err(|error| StorageError::Backend(error.to_string()))?;
        let execute_ms = execute_started.elapsed().as_secs_f64() * 1000.0;
        let collect_started = std::time::Instant::now();
        let batches: Vec<RecordBatch> = stream.try_collect().await
            .map_err(|error| StorageError::Backend(error.to_string()))?;
        let collect_ms = collect_started.elapsed().as_secs_f64() * 1000.0;
        let decode_started = std::time::Instant::now();
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
            let tenants = batch.column_by_name("tenant")
                .and_then(|column| column.as_any().downcast_ref::<StringArray>())
                .ok_or_else(|| StorageError::Backend("invalid vector tenant column".into()))?;
            let workspaces = batch.column_by_name("workspace")
                .and_then(|column| column.as_any().downcast_ref::<StringArray>())
                .ok_or_else(|| StorageError::Backend("invalid vector workspace column".into()))?;
            let generations = batch.column_by_name("generation")
                .and_then(|column| column.as_any().downcast_ref::<Int64Array>())
                .ok_or_else(|| StorageError::Backend("invalid vector generation column".into()))?;
            for i in 0..ids.len() {
                if [ids, srcs, tenants, workspaces].iter().any(|column| column.is_null(i))
                    || vers.is_null(i) || generations.is_null(i) || dists.is_null(i)
                    || !dists.value(i).is_finite() || dists.value(i) < 0.0
                    || tenants.value(i) != query.scope.tenant_id.to_string()
                    || workspaces.value(i) != query.scope.workspace_id.to_string()
                    || generations.value(i) != query.generation
                {
                    return Err(StorageError::Backend("invalid scoped vector candidate".into()));
                }
                let id = Uuid::parse_str(ids.value(i))
                    .map_err(|_| StorageError::Backend("invalid vector identity".into()))?;
                if query.artifact_ids.as_ref().is_some_and(|allowed| allowed.binary_search(&id).is_err()) {
                    return Err(StorageError::Backend("ineligible vector candidate".into()));
                }
                hits.push(VectorHit {
                    id,
                    score: 1.0 / (1.0 + dists.value(i)),
                    source: SourceVersion {
                        source_id: Uuid::parse_str(srcs.value(i))
                            .map_err(|e| StorageError::Backend(e.to_string()))?,
                        version: vers.value(i),
                    },
                });
            }
        }
        hits.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
        tracing::debug!(
            target: "origence::vector_timing",
            used_ann, open_ms, prepare_ms, execute_ms, collect_ms,
            decode_ms = decode_started.elapsed().as_secs_f64() * 1000.0,
            total_ms = started.elapsed().as_secs_f64() * 1000.0,
            returned_count = hits.len(),
            "vector query completed"
        );
        Ok(hits)
    }

    async fn open_existing(&self, name: &str) -> StorageResult<Option<lancedb::Table>> {
        match self.db.open_table(name).execute().await {
            Ok(table) => Ok(Some(table)),
            Err(lancedb::Error::TableNotFound { .. }) => Ok(None),
            Err(error) => Err(StorageError::Backend(error.to_string())),
        }
    }

    async fn managed_tables(&self) -> StorageResult<Vec<lancedb::Table>> {
        let names = self.db.table_names().execute().await
            .map_err(|error| StorageError::Backend(error.to_string()))?;
        let mut tables = Vec::new();
        for name in names {
            if name != FULL_TEXT_TABLE && !vector_table_name(&name) {
                return Err(StorageError::Conflict("unexpected native index table".into()));
            }
            let table = self.db.open_table(&name).execute().await
                .map_err(|error| StorageError::Backend(error.to_string()))?;
            if name == FULL_TEXT_TABLE {
                full_text::check_schema(&table).await?;
            } else {
                let schema = table.schema().await
                    .map_err(|error| StorageError::Backend(error.to_string()))?;
                let dimension = match schema.field_with_name("vector").map(|field| field.data_type()) {
                    Ok(DataType::FixedSizeList(_, size)) if *size > 0 => *size as usize,
                    _ => return Err(StorageError::Conflict("incompatible vector schema".into())),
                };
                check_dimension(&table, dimension).await?;
            }
            tables.push(table);
        }
        Ok(tables)
    }

    async fn ensure_table(&self, name: &str, dim: usize) -> StorageResult<()> {
        let table = self
            .db
            .create_empty_table(name, schema_for(dim))
            .mode(CreateTableMode::exist_ok(|request| request))
            .execute()
            .await
            .map_err(|e| match e {
                lancedb::Error::Schema { message } => StorageError::Conflict(message),
                other => StorageError::Backend(other.to_string()),
            })?;
        check_dimension(&table, dim).await
    }
}

impl VectorStore for LanceDbStore {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            filtered_ann: true,
            exact_search: true,
            max_hops: 0,
            shared_source_delete: false,
        }
    }

    async fn upsert(&self, scope: Scope, entries: Vec<VectorEntry>) -> StorageResult<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut unique: HashMap<_, &VectorEntry> = HashMap::new();
        for entry in &entries {
            validate_vector(entry.dimension, &entry.embedding)?;
            let key = (&entry.profile, entry.id, entry.source, entry.generation);
            if let Some(previous) = unique.insert(key, entry)
                && (previous.dimension != entry.dimension || previous.embedding != entry.embedding)
            {
                return Err(StorageError::Conflict("conflicting vector batch identity".into()));
            }
        }
        let mut groups: HashMap<String, Vec<&VectorEntry>> = HashMap::new();
        for entry in unique.into_values() {
            groups.entry(entry.profile.clone()).or_default().push(entry);
        }
        // Validate the whole batch before writing any profile.
        for group in groups.values() {
            let dim = group[0].dimension;
            if group.iter().any(|e| e.dimension != dim) {
                return Err(StorageError::Conflict(
                    "mixed dimensions within one profile batch".into(),
                ));
            }
        }
        for (profile, group) in &groups {
            self.ensure_table(&table_name(profile), group[0].dimension)
                .await?;
        }
        for (profile, group) in groups {
            let dim = group[0].dimension;
            let name = table_name(&profile);
            let table = self
                .db
                .open_table(&name)
                .execute()
                .await
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            let schema = schema_for(dim);
            let tenants: Vec<String> = group.iter().map(|_| scope.tenant_id.to_string()).collect();
            let workspaces: Vec<String> = group
                .iter()
                .map(|_| scope.workspace_id.to_string())
                .collect();
            let ids: Vec<String> = group.iter().map(|e| e.id.to_string()).collect();
            let source_ids: Vec<String> = group
                .iter()
                .map(|e| e.source.source_id.to_string())
                .collect();
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
                    Arc::new(
                        FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
                            vectors, dim as i32,
                        ),
                    ),
                ],
            )
            .map_err(|e| StorageError::Backend(e.to_string()))?;
            let reader = RecordBatchIterator::new(vec![Ok(batch)], schema.clone());
            let mut merge = table.merge_insert(&[
                "tenant",
                "workspace",
                "source_id",
                "version",
                "id",
                "generation",
            ]);
            merge.when_matched_update_all(None);
            merge.when_not_matched_insert_all();
            merge
                .execute(Box::new(reader))
                .await
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            ann::ensure_index(&table).await?;
        }
        Ok(())
    }

    async fn search(&self, query: VectorQuery) -> StorageResult<Vec<VectorHit>> {
        self.search_inner(query, None).await
    }

    async fn upsert_full_text(
        &self,
        scope: Scope,
        entries: Vec<FullTextEntry>,
    ) -> StorageResult<()> {
        full_text::upsert(self, scope, entries).await
    }

    async fn search_full_text(&self, query: FullTextQuery) -> StorageResult<Vec<FullTextHit>> {
        full_text::search(self, query, None).await
    }

    async fn delete_source(&self, scope: Scope, source: SourceVersion) -> StorageResult<()> {
        for table in self.managed_tables().await? {
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

    async fn delete_objects(&self, scope: Scope, ids: Vec<Uuid>) -> StorageResult<()> {
        if ids.is_empty() {
            return Ok(());
        }
        for table in self.managed_tables().await? {
            for batch in ids.chunks(100) {
                let values = batch
                    .iter()
                    .map(|id| format!("'{id}'"))
                    .collect::<Vec<_>>()
                    .join(",");
                table
                    .delete(&format!(
                        "tenant='{}' AND workspace='{}' AND id IN ({values})",
                        scope.tenant_id, scope.workspace_id
                    ))
                    .await
                    .map_err(|e| StorageError::Backend(e.to_string()))?;
            }
        }
        Ok(())
    }
}

impl Lifecycle for LanceDbStore {
    async fn initialize(&self) -> StorageResult<()> {
        Ok(())
    }

    async fn check(&self) -> StorageResult<()> {
        self.managed_tables().await.map(|_| ())
    }

    async fn shutdown(&self) -> StorageResult<()> {
        Ok(())
    }
}
