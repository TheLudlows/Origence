//! native Turso graph backend (M4).
//!
//! Entities and relations live in two tables inside one native Turso database file.
//! The relational store already holds the process's single-writer lock on the
//! data directory, so this store takes no file lock of its own. Writes
//! auto-commit per object; the
//! cross-store ledger — not this store — is the authority for what stays
//! visible, and `Service::open` reaps un-committed objects on restart (A2.6).

use std::path::Path;

use serde_json::{Value, json};
use crate::storage::native_turso::{self, Connection, Pool};
use uuid::Uuid;

use crate::graph::{entity_id, relation_id};
use crate::storage::capabilities::Capabilities;
use crate::storage::error::{StorageError, StorageResult};
use crate::storage::scope::{Scope, SourceVersion};
use crate::storage::traits::{GraphStore, Lifecycle};
use crate::types::{Entity, Relation};

const SCHEMA: &str = include_str!("sqlite-graph-schema.sql");

/// A graph backend owning a single native Turso database file.
pub struct TursoGraphStore {
    pool: Pool,
    max_hops: usize,
}

fn turso_err(e: native_turso::Error) -> StorageError {
    match e {
        native_turso::Error::Incompatible(message) => StorageError::Unavailable(message.into()),
        error => StorageError::Backend(error.to_string()),
    }
}

fn entity_uid(scope: Scope, id: Uuid) -> String {
    format!("{}|{}|{}", scope.tenant_id, scope.workspace_id, id)
}

impl TursoGraphStore {
    /// Open the store at `path` (a native Turso file), creating the file and schema
    /// when absent. Fails without repairing if the schema is incompatible.
    pub async fn open(path: impl AsRef<Path>) -> StorageResult<Self> {
        Self::open_inner(path, None).await
    }

    /// The application graph shares the authority's host lock, including
    /// cancelled initialization and deferred rollback after Service drop.
    #[cfg(feature = "local-storage")]
    pub(crate) async fn open_with_authority(
        path: impl AsRef<Path>,
        authority: &crate::storage::TursoStore,
    ) -> StorageResult<Self> {
        Self::open_inner(path, Some(authority.native_lifetime_guard())).await
    }

    async fn open_inner(
        path: impl AsRef<Path>,
        guard: Option<std::sync::Arc<dyn Send + Sync>>,
    ) -> StorageResult<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            tokio::fs::create_dir_all(parent).await?;
        }
        if path.exists() && tokio::fs::metadata(path).await?.len() != 0 {
            let pool = Pool::open_read_only(path).await.map_err(turso_err)?;
            if let Some(guard) = &guard { pool.hold_lifetime_guard(guard.clone()); }
            let outcome = async {
                let mut conn = pool.acquire().await.map_err(turso_err)?;
                native_turso::check_format(&mut conn, "graph").await.map_err(turso_err)?;
                check_schema(&mut conn).await
            }.await;
            pool.close().await;
            outcome?;
        }
        let pool = Pool::open(path).await.map_err(turso_err)?;
        if let Some(guard) = guard { pool.hold_lifetime_guard(guard); }
        let store = Self { pool, max_hops: 3 };
        store.initialize().await?;
        Ok(store)
    }

    /// Scoped graph identities for hybrid recall. Returns the stable
    /// snapshot shape: `id`/`name` for entities and
    /// `id`/`source_id`/`target_id`/`fact_text` for relations, every name and
    /// predicate lower-cased and trimmed, both lists sorted by `id`.
    pub async fn snapshot(&self, scope: Scope) -> StorageResult<Value> {
        let tenant = scope.tenant_id.to_string();
        let workspace = scope.workspace_id.to_string();
        let mut tx = self.pool.begin().await.map_err(turso_err)?;
        let rows: Vec<(String, String)> = native_turso::query_as(
            "SELECT id, name FROM oc_graph_entities WHERE tenant_id = ? AND workspace_id = ?",
        )
        .bind(&tenant)
        .bind(&workspace)
        .fetch_all(&mut *tx)
        .await
        .map_err(turso_err)?;
        let mut entities: Vec<Value> = rows
            .into_iter()
            .map(|(id, name)| json!({"id": id, "name": name.trim().to_lowercase()}))
            .collect();
        entities.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));

        let rows: Vec<(String, String, String, String, String, String)> = native_turso::query_as(
            "SELECT r.id, e1.id, e2.id, e1.name, r.predicate, e2.name \
             FROM oc_graph_relations r \
             JOIN oc_graph_entities e1 ON e1.uid = r.head_uid \
             JOIN oc_graph_entities e2 ON e2.uid = r.tail_uid \
             WHERE r.tenant_id = ? AND r.workspace_id = ?",
        )
        .bind(&tenant)
        .bind(&workspace)
        .fetch_all(&mut *tx)
        .await
        .map_err(turso_err)?;
        let mut relations: Vec<Value> = rows
            .into_iter()
            .map(|(id, source_id, target_id, a_name, predicate, b_name)| {
                json!({
                    "id": id,
                    "source_id": source_id,
                    "target_id": target_id,
                    "fact_text": format!(
                        "{} {} {}",
                        a_name.trim().to_lowercase(),
                        predicate.trim().to_lowercase(),
                        b_name.trim().to_lowercase()
                    ),
                })
            })
            .collect();
        relations.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));

        tx.commit().await.map_err(turso_err)?;
        Ok(json!({"entities": entities, "relations": relations}))
    }
}

impl GraphStore for TursoGraphStore {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            filtered_ann: false,
            exact_search: false,
            max_hops: self.max_hops,
            shared_source_delete: false,
        }
    }

    async fn upsert_entities(
        &self,
        scope: Scope,
        source: SourceVersion,
        entities: Vec<Entity>,
    ) -> StorageResult<()> {
        let tenant = scope.tenant_id.to_string();
        let workspace = scope.workspace_id.to_string();
        let source_id = source.source_id.to_string();
        let version = source.version as i64;
        for e in &entities {
            let id = entity_id(&e.name);
            let uid = entity_uid(scope, id);
            native_turso::query(
                "INSERT INTO oc_graph_entities(uid, tenant_id, workspace_id, id, name, entity_type, description, source_id, version) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT(uid) DO UPDATE SET \
                   tenant_id = excluded.tenant_id, workspace_id = excluded.workspace_id, \
                   id = excluded.id, name = excluded.name, entity_type = excluded.entity_type, \
                   description = excluded.description, source_id = excluded.source_id, \
                   version = excluded.version",
            )
            .bind(&uid)
            .bind(&tenant)
            .bind(&workspace)
            .bind(id.to_string())
            .bind(&e.name)
            .bind(&e.entity_type)
            .bind(&e.description)
            .bind(&source_id)
            .bind(version)
            .execute(&self.pool)
            .await
            .map_err(turso_err)?;
        }
        Ok(())
    }

    async fn upsert_relations(
        &self,
        scope: Scope,
        source: SourceVersion,
        relations: Vec<Relation>,
    ) -> StorageResult<()> {
        let tenant = scope.tenant_id.to_string();
        let workspace = scope.workspace_id.to_string();
        let source_id = source.source_id.to_string();
        let version = source.version as i64;
        for r in &relations {
            let head = entity_id(&r.source);
            let tail = entity_id(&r.target);
            let rel = relation_id(head, &r.predicate, tail);
            let head_uid = entity_uid(scope, head);
            let tail_uid = entity_uid(scope, tail);
            // First writer wins; missing endpoints are a no-op. Inserting
            // from the scoped entity join is atomic with endpoint existence.
            native_turso::query(
                "INSERT INTO oc_graph_relations(id, tenant_id, workspace_id, head_uid, tail_uid, predicate, source_id, version) \
                 SELECT ?, ?, ?, a.uid, b.uid, ?, ?, ? \
                 FROM oc_graph_entities a, oc_graph_entities b \
                 WHERE a.uid = ? AND b.uid = ? \
                 ON CONFLICT(head_uid, tail_uid, id) DO NOTHING",
            )
            .bind(rel.to_string())
            .bind(&tenant)
            .bind(&workspace)
            .bind(&r.predicate)
            .bind(&source_id)
            .bind(version)
            .bind(&head_uid)
            .bind(&tail_uid)
            .execute(&self.pool)
            .await
            .map_err(turso_err)?;
        }
        Ok(())
    }

    async fn traverse(
        &self,
        scope: Scope,
        from: Uuid,
        max_hops: usize,
    ) -> StorageResult<Vec<Uuid>> {
        let hops = max_hops.min(self.max_hops);
        if hops == 0 {
            return Ok(Vec::new());
        }
        let from_uid = entity_uid(scope, from);
        let tenant = scope.tenant_id.to_string();
        let workspace = scope.workspace_id.to_string();
        let ids: Vec<String> = native_turso::query_scalar(
            "WITH RECURSIVE reach(uid, depth) AS ( \
                SELECT uid, 0 FROM oc_graph_entities WHERE uid = ? \
                UNION \
                SELECT r.tail_uid, reach.depth + 1 \
                FROM oc_graph_relations r JOIN reach ON r.head_uid = reach.uid \
                WHERE reach.depth < ? AND r.tenant_id = ? AND r.workspace_id = ? \
             ) \
             SELECT DISTINCT e.id \
             FROM reach JOIN oc_graph_entities e ON e.uid = reach.uid \
             WHERE reach.depth > 0 AND e.tenant_id = ? AND e.workspace_id = ? AND e.uid <> ? \
             ORDER BY e.id",
        )
        .bind(&from_uid)
        .bind(hops as i64)
        .bind(&tenant)
        .bind(&workspace)
        .bind(&tenant)
        .bind(&workspace)
        .bind(&from_uid)
        .fetch_all(&self.pool)
        .await
        .map_err(turso_err)?;
        Ok(ids
            .into_iter()
            .filter_map(|s| Uuid::parse_str(&s).ok())
            .collect())
    }

    async fn delete_source(&self, scope: Scope, source: SourceVersion) -> StorageResult<()> {
        let tenant = scope.tenant_id.to_string();
        let workspace = scope.workspace_id.to_string();
        let source_id = source.source_id.to_string();
        let version = source.version as i64;
        // Foreign-key cascades atomically detach incident relations.
        native_turso::query(
            "DELETE FROM oc_graph_entities WHERE tenant_id = ? AND workspace_id = ? AND source_id = ? AND version = ?",
        )
        .bind(&tenant)
        .bind(&workspace)
        .bind(&source_id)
        .bind(version)
        .execute(&self.pool)
        .await
        .map_err(turso_err)?;
        native_turso::query(
            "DELETE FROM oc_graph_relations WHERE tenant_id = ? AND workspace_id = ? AND source_id = ? AND version = ?",
        )
        .bind(&tenant)
        .bind(&workspace)
        .bind(&source_id)
        .bind(version)
        .execute(&self.pool)
        .await
        .map_err(turso_err)?;
        Ok(())
    }

    async fn list_ids(&self, scope: Scope) -> StorageResult<Value> {
        let tenant = scope.tenant_id.to_string();
        let workspace = scope.workspace_id.to_string();
        let mut tx = self.pool.begin().await.map_err(turso_err)?;
        let entities: Vec<String> = native_turso::query_scalar(
            "SELECT id FROM oc_graph_entities WHERE tenant_id = ? AND workspace_id = ? ORDER BY id",
        )
        .bind(&tenant)
        .bind(&workspace)
        .fetch_all(&mut *tx)
        .await
        .map_err(turso_err)?;
        let relations: Vec<String> = native_turso::query_scalar(
            "SELECT id FROM oc_graph_relations WHERE tenant_id = ? AND workspace_id = ? ORDER BY id",
        )
        .bind(&tenant)
        .bind(&workspace)
        .fetch_all(&mut *tx)
        .await
        .map_err(turso_err)?;
        tx.commit().await.map_err(turso_err)?;
        Ok(json!({"entities": entities, "relations": relations}))
    }

    async fn delete_objects(
        &self,
        scope: Scope,
        entities: Vec<Uuid>,
        relations: Vec<Uuid>,
    ) -> StorageResult<()> {
        let tenant = scope.tenant_id.to_string();
        let workspace = scope.workspace_id.to_string();
        for id in &entities {
            // Deleting the entity also detaches its incident relations.
            native_turso::query(
                "DELETE FROM oc_graph_entities WHERE tenant_id = ? AND workspace_id = ? AND id = ?",
            )
            .bind(&tenant)
            .bind(&workspace)
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(turso_err)?;
        }
        for id in &relations {
            native_turso::query(
                "DELETE FROM oc_graph_relations WHERE tenant_id = ? AND workspace_id = ? AND id = ?",
            )
            .bind(&tenant)
            .bind(&workspace)
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(turso_err)?;
        }
        Ok(())
    }
}

// Compare known DDL, including keys, cascades and indexes, rather than only
// testing column projections. Accept native Turso's whitespace/case normalization.
async fn check_schema(conn: &mut Connection) -> StorageResult<()> {
    native_turso::check_format(conn, "graph").await.map_err(turso_err)?;
    native_turso::verify_schema(conn, &[SCHEMA]).await.map_err(turso_err)
}

impl Lifecycle for TursoGraphStore {
    async fn initialize(&self) -> StorageResult<()> {
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(turso_err)?;
        let present: bool = native_turso::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT GLOB 'sqlite_*')",
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(turso_err)?;
        // Create only an empty database. Never repair a partial schema or
        // accept a same-named view; initialization is an atomic short write.
        if !present {
            native_turso::raw_sql(SCHEMA)
                .execute(&mut *tx)
                .await
                .map_err(turso_err)?;
            native_turso::install_format(&mut tx, "graph").await.map_err(turso_err)?;
        }
        check_schema(&mut tx).await?;
        tx.commit().await.map_err(turso_err)
    }

    async fn check(&self) -> StorageResult<()> {
        let mut conn = self.pool.acquire().await.map_err(turso_err)?;
        check_schema(&mut conn).await
    }

    async fn shutdown(&self) -> StorageResult<()> {
        self.pool.shutdown().await.map_err(turso_err)
    }
}

/// Compatibility alias; graph SQL executes on native Rust Turso.
pub type SqliteGraphStore = TursoGraphStore;

#[cfg(all(test, feature = "local-storage"))]
mod host_lock_tests {
    use super::*;

    #[tokio::test]
    async fn graph_holds_host_lock_after_authority_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("context.db");
        let authority = crate::storage::TursoStore::open(&path).await.unwrap();
        let graph = TursoGraphStore::open_with_authority(dir.path().join("graph.db"), &authority)
            .await.unwrap();
        authority.shutdown().await.unwrap();
        drop(authority);
        assert!(matches!(crate::storage::sqlite::lock_for_maintenance(&path).await, Err(StorageError::Conflict(_))));
        graph.shutdown().await.unwrap();
        drop(graph);
        let _maintenance = crate::storage::sqlite::lock_for_maintenance(&path).await.unwrap();
    }
}
