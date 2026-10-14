//! native Turso graph backend (M4).
//!
//! Entities and relations live in two tables inside one native Turso database file.
//! The relational store already holds the process's single-writer lock on the
//! data directory, so this store takes no file lock of its own. Each object
//! write is atomic independently; the
//! cross-store ledger — not this store — is the authority for what stays
//! visible, and `Service::open` reaps un-committed objects on restart (A2.6).

use std::collections::{BTreeSet, HashSet};
use std::path::Path;

use crate::storage::native_turso::{self, Connection, Pool};
use serde_json::{Value, json};
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

// Graph identifiers and scope are stored as TEXT, unlike authority UUID BLOBs.
// Preserve native values until a selected output field needs strict decoding.
#[derive(Clone)]
struct GraphKey(turso::Value);

impl native_turso::Decode<'_> for GraphKey {
    fn decode(value: &turso::Value) -> native_turso::Result<Self> {
        Ok(Self(value.clone()))
    }
}

impl native_turso::Encode for GraphKey {
    fn encode(self) -> native_turso::Result<turso::Value> {
        Ok(self.0)
    }
}

const ENDPOINT_BATCH: usize = 8;
const FRONTIER_BATCH: usize = 32;

/// Associate only the requested endpoint UIDs, without adding an endpoint scope
/// predicate or inventing Rust equality for TEXT/BLOB/NULL values.
async fn endpoint_rows(
    conn: &mut Connection,
    keys: &[GraphKey],
) -> StorageResult<Vec<Option<native_turso::Row>>> {
    let mut matched: Vec<_> = (0..keys.len()).map(|_| None).collect();
    for offset in (0..keys.len().max(1)).step_by(ENDPOINT_BATCH) {
        let batch = &keys[offset..(offset + ENDPOINT_BATCH).min(keys.len())];
        let mut query = native_turso::query(
            "SELECT id,name,(uid=?) AS m0,(uid=?) AS m1,(uid=?) AS m2,(uid=?) AS m3,(uid=?) AS m4,(uid=?) AS m5,(uid=?) AS m6,(uid=?) AS m7 FROM oc_graph_entities WHERE uid IN (?,?,?,?,?,?,?,?)",
        );
        for _ in 0..2 {
            for slot in 0..ENDPOINT_BATCH {
                query = query.bind(batch.get(slot).cloned());
            }
        }
        for row in query.fetch_all(&mut *conn).await.map_err(turso_err)? {
            for (slot, name) in ["m0", "m1", "m2", "m3", "m4", "m5", "m6", "m7"]
                .iter()
                .take(batch.len())
                .enumerate()
            {
                if row.try_get::<Option<bool>>(*name).map_err(turso_err)? == Some(true) {
                    matched[offset + slot] =
                        Some(native_turso::FromRow::from_row(&row).map_err(turso_err)?);
                }
            }
        }
    }
    Ok(matched)
}

async fn snapshot_relations(
    conn: &mut Connection,
    tenant: &str,
    workspace: &str,
) -> StorageResult<Vec<Value>> {
    let rows = native_turso::query(
        "SELECT id,head_uid,tail_uid,predicate FROM oc_graph_relations WHERE tenant_id=? AND workspace_id=?",
    )
    .bind(tenant)
    .bind(workspace)
    .fetch_all(&mut *conn)
    .await
    .map_err(turso_err)?;
    let mut keys = Vec::with_capacity(rows.len() * 2);
    for row in &rows {
        keys.push(row.try_get("head_uid").map_err(turso_err)?);
        keys.push(row.try_get("tail_uid").map_err(turso_err)?);
    }
    let endpoints = endpoint_rows(conn, &keys).await?;
    let mut relations = Vec::new();
    for (index, row) in rows.into_iter().enumerate() {
        let (Some(head), Some(tail)) = (&endpoints[index * 2], &endpoints[index * 2 + 1]) else {
            continue;
        };
        // The former inner association excluded missing endpoints before
        // decoding these six selected fields. Do not decode excluded payloads.
        let id: String = row.try_get("id").map_err(turso_err)?;
        let source_id: String = head.try_get("id").map_err(turso_err)?;
        let target_id: String = tail.try_get("id").map_err(turso_err)?;
        let head_name: String = head.try_get("name").map_err(turso_err)?;
        let predicate: String = row.try_get("predicate").map_err(turso_err)?;
        let tail_name: String = tail.try_get("name").map_err(turso_err)?;
        relations.push(json!({
            "id": id,
            "source_id": source_id,
            "target_id": target_id,
            "fact_text": format!(
                "{} {} {}",
                head_name.trim().to_lowercase(),
                predicate.trim().to_lowercase(),
                tail_name.trim().to_lowercase()
            ),
        }));
    }
    // Preserve multiple relations sharing an id but having different endpoints.
    relations.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    Ok(relations)
}

async fn frontier_edges(
    conn: &mut Connection,
    tenant: &str,
    workspace: &str,
    frontier: &[GraphKey],
) -> StorageResult<Vec<native_turso::Row>> {
    let mut rows = Vec::new();
    // One NULL-filled batch preserves relation/column preparation for an empty
    // positive-hop traversal. This is a batch bound, never a candidate limit.
    for offset in (0..frontier.len().max(1)).step_by(FRONTIER_BATCH) {
        let batch = &frontier[offset..(offset + FRONTIER_BATCH).min(frontier.len())];
        let mut query = native_turso::query(
            "SELECT rowid,tail_uid FROM oc_graph_relations WHERE tenant_id=? AND workspace_id=? AND head_uid IN (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(tenant)
        .bind(workspace);
        for slot in 0..FRONTIER_BATCH {
            query = query.bind(batch.get(slot).cloned());
        }
        rows.extend(query.fetch_all(&mut *conn).await.map_err(turso_err)?);
    }
    Ok(rows)
}

async fn reached_entity_ids(
    conn: &mut Connection,
    tenant: &str,
    workspace: &str,
    from_uid: &str,
    reached: &[GraphKey],
) -> StorageResult<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    for offset in (0..reached.len().max(1)).step_by(FRONTIER_BATCH) {
        let batch = &reached[offset..(offset + FRONTIER_BATCH).min(reached.len())];
        let mut query = native_turso::query_scalar::<String>(
            "SELECT DISTINCT id FROM oc_graph_entities WHERE tenant_id=? AND workspace_id=? AND uid<>? AND uid IN (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) ORDER BY id",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(from_uid);
        for slot in 0..FRONTIER_BATCH {
            query = query.bind(batch.get(slot).cloned());
        }
        ids.extend(query.fetch_all(&mut *conn).await.map_err(turso_err)?);
    }
    // The admitted schema has default BINARY TEXT collation. String ordering
    // merges exact native id text across batches, before any UUID parsing.
    Ok(ids)
}

async fn traverse_ids(
    conn: &mut Connection,
    tenant: &str,
    workspace: &str,
    from_uid: &str,
    hops: usize,
) -> StorageResult<BTreeSet<String>> {
    let source_exists: bool = native_turso::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM oc_graph_entities WHERE uid=?)",
    )
    .bind(from_uid)
    .fetch_one(&mut *conn)
    .await
    .map_err(turso_err)?;
    let mut frontier = if source_exists {
        vec![GraphKey(turso::Value::Text(from_uid.to_owned()))]
    } else {
        Vec::new()
    };
    let mut visited = HashSet::new();
    let mut reached = Vec::new();
    for _ in 0..hops {
        let rows = frontier_edges(conn, tenant, workspace, &frontier).await?;
        let mut next = Vec::new();
        for row in rows {
            let rowid: i64 = row.try_get("rowid").map_err(turso_err)?;
            if visited.insert(rowid) {
                let tail: GraphKey = row.try_get("tail_uid").map_err(turso_err)?;
                reached.push(tail.clone());
                next.push(tail);
            }
        }
        // Each rowid is expanded first at its minimum reachable depth. A later
        // path cannot improve reachability; no intermediate entity check applies.
        frontier = next;
        if frontier.is_empty() {
            break;
        }
    }
    reached_entity_ids(conn, tenant, workspace, from_uid, &reached).await
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
            if let Some(guard) = &guard {
                pool.hold_lifetime_guard(guard.clone());
            }
            let outcome = async {
                let mut conn = pool.acquire().await.map_err(turso_err)?;
                native_turso::check_format(&mut conn, "graph")
                    .await
                    .map_err(turso_err)?;
                check_schema(&mut conn).await
            }
            .await;
            pool.close().await;
            outcome?;
        }
        let pool = Pool::open(path).await.map_err(turso_err)?;
        if let Some(guard) = guard {
            pool.hold_lifetime_guard(guard);
        }
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

        let relations = snapshot_relations(&mut tx, &tenant, &workspace).await?;

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
            // One transaction per object preserves the successful prefix of a
            // batch while making endpoint checks and insertion atomic together.
            let mut tx = self
                .pool
                .begin_with("BEGIN IMMEDIATE")
                .await
                .map_err(turso_err)?;
            let endpoints: Vec<(bool, bool)> = native_turso::query_as(
                "SELECT uid=?,uid=? FROM oc_graph_entities WHERE uid IN (?,?)",
            )
            .bind(&head_uid)
            .bind(&tail_uid)
            .bind(&head_uid)
            .bind(&tail_uid)
            .fetch_all(&mut *tx)
            .await
            .map_err(turso_err)?;
            let present = endpoints.iter().any(|(head, _)| *head)
                && endpoints.iter().any(|(_, tail)| *tail);
            // No FROM source: the only relation is the insertion target. The
            // false gate remains a no-op while preparing all original columns
            // and the conflict key even when an endpoint is absent.
            native_turso::query(
                "INSERT INTO oc_graph_relations(id,tenant_id,workspace_id,head_uid,tail_uid,predicate,source_id,version) \
                 SELECT ?,?,?,?,?,?,?,? WHERE ? ON CONFLICT(head_uid,tail_uid,id) DO NOTHING",
            )
            .bind(rel.to_string())
            .bind(&tenant)
            .bind(&workspace)
            .bind(&head_uid)
            .bind(&tail_uid)
            .bind(&r.predicate)
            .bind(&source_id)
            .bind(version)
            .bind(present)
            .execute(&mut *tx)
            .await
            .map_err(turso_err)?;
            tx.commit().await.map_err(turso_err)?;
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
        let mut tx = self.pool.begin().await.map_err(turso_err)?;
        let ids = traverse_ids(&mut tx, &tenant, &workspace, &from_uid, hops).await?;
        tx.commit().await.map_err(turso_err)?;
        // Every selected field has now decoded as String. UUID text failures
        // alone are filtered; do not deduplicate parsed UUID values afterward.
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
    native_turso::check_format(conn, "graph")
        .await
        .map_err(turso_err)?;
    native_turso::verify_schema(conn, &[SCHEMA])
        .await
        .map_err(turso_err)
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
            native_turso::install_format(&mut tx, "graph")
                .await
                .map_err(turso_err)?;
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
            .await
            .unwrap();
        authority.shutdown().await.unwrap();
        drop(authority);
        assert!(matches!(
            crate::storage::sqlite::lock_for_maintenance(&path).await,
            Err(StorageError::Conflict(_))
        ));
        graph.shutdown().await.unwrap();
        drop(graph);
        let _maintenance = crate::storage::sqlite::lock_for_maintenance(&path)
            .await
            .unwrap();
    }
}

#[cfg(test)]
#[path = "sqlite_graph_read_tests.rs"]
mod read_tests;
