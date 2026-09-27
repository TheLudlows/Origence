//! Kuzu graph backend (M4).
//!
//! Kuzu is synchronous and holds an OS lock for its write host, so the local
//! API and Worker share one in-process `Database` and route every call through
//! a bounded blocking executor (A2.4). Nodes/relations carry a `source_id` /
//! `version` projection for the non-authoritative, last-writer source-tagged
//! delete; owner/visibility authority stays in the SQLite ledger (A5). The
//! authoritative shared-owner delete is `delete_objects`, driven by graph-core
//! G4 after an `owned_artifacts` diff.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kuzu::{Connection, Database, SystemConfig, Value as KValue};
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::graph::{entity_id, relation_id};
use crate::storage::capabilities::Capabilities;
use crate::storage::error::{StorageError, StorageResult};
use crate::storage::scope::{Scope, SourceVersion};
use crate::storage::traits::{GraphStore, Lifecycle};
use crate::types::{Entity, Relation};

const ENTITY_DDL: &str = "CREATE NODE TABLE IF NOT EXISTS Entity(uid STRING, tenant STRING, workspace STRING, id STRING, name STRING, entity_type STRING, description STRING, source_id STRING, version INT64, PRIMARY KEY(uid))";
const RELATION_DDL: &str = "CREATE REL TABLE IF NOT EXISTS Relation(FROM Entity TO Entity, id STRING, tenant STRING, workspace STRING, predicate STRING, source_id STRING, version INT64)";

/// A graph backend owning a single Kuzu database file.
pub struct KuzuStore {
    db: Arc<Database>,
    semaphore: Arc<Semaphore>,
    _path: PathBuf,
    max_hops: usize,
}

impl KuzuStore {
    /// Open the store at `dir`, creating `dir/kuzu.db`. Kuzu's write host
    /// excludes any second process from opening the file (A2.4), so this is
    /// the single owner for the process.
    pub async fn open(dir: impl AsRef<Path>) -> StorageResult<Self> {
        let dir = dir.as_ref().to_path_buf();
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join("kuzu.db");
        let db = tokio::task::spawn_blocking({
            let path = path.clone();
            move || {
                Database::new(
                    &path,
                    SystemConfig::default()
                        .buffer_pool_size(64 * 1024 * 1024)
                        .max_num_threads(2),
                )
            }
        })
        .await
        .map_err(|e| StorageError::Backend(e.to_string()))?
        .map_err(|e| StorageError::Backend(e.to_string()))?;
        let store = Self {
            db: Arc::new(db),
            semaphore: Arc::new(Semaphore::new(1)),
            _path: path,
            max_hops: 3,
        };
        store.initialize().await?;
        Ok(store)
    }

    /// Run a synchronous Kuzu operation on a fresh `Connection` inside the
    /// bounded blocking executor (A2.4). The single permit serializes graph
    /// operations so the check-then-create relation upsert cannot race, and
    /// it matches Kuzu's single-writer model.
    async fn with_conn<T, F>(&self, f: F) -> StorageResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> StorageResult<T> + Send + 'static,
    {
        let _permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = Connection::new(db.as_ref())
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            f(&conn)
        })
        .await
        .map_err(|e| StorageError::Backend(e.to_string()))?
    }
}

impl GraphStore for KuzuStore {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            filtered_ann: false,
            exact_search: false,
            max_hops: 3,
            shared_source_delete: false,
        }
    }

    async fn upsert_entities(
        &self,
        scope: Scope,
        source: SourceVersion,
        entities: Vec<Entity>,
    ) -> StorageResult<()> {
        if entities.is_empty() {
            return Ok(());
        }
        self.with_conn(move |conn| {
            let tenant = scope.tenant_id.to_string();
            let workspace = scope.workspace_id.to_string();
            let source_id = source.source_id.to_string();
            let version = source.version as i64;
            for e in &entities {
                let id = entity_id(&e.name);
                let uid = format!("{tenant}|{workspace}|{id}");
                let mut stmt = conn
                    .prepare(
                        "MERGE (n:Entity {uid:$uid}) SET n.tenant=$tenant, n.workspace=$workspace, n.id=$id, n.name=$name, n.entity_type=$entity_type, n.description=$description, n.source_id=$source_id, n.version=$version",
                    )
                    .map_err(|e| StorageError::Backend(e.to_string()))?;
                conn.execute(
                    &mut stmt,
                    vec![
                        ("uid", KValue::String(uid)),
                        ("tenant", KValue::String(tenant.clone())),
                        ("workspace", KValue::String(workspace.clone())),
                        ("id", KValue::String(id.to_string())),
                        ("name", KValue::String(e.name.clone())),
                        ("entity_type", KValue::String(e.entity_type.clone())),
                        ("description", KValue::String(e.description.clone())),
                        ("source_id", KValue::String(source_id.clone())),
                        ("version", KValue::Int64(version)),
                    ],
                )
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            }
            Ok(())
        })
        .await
    }

    async fn upsert_relations(
        &self,
        scope: Scope,
        source: SourceVersion,
        relations: Vec<Relation>,
    ) -> StorageResult<()> {
        if relations.is_empty() {
            return Ok(());
        }
        self.with_conn(move |conn| {
            let tenant = scope.tenant_id.to_string();
            let workspace = scope.workspace_id.to_string();
            let source_id = source.source_id.to_string();
            let version = source.version as i64;
            for r in &relations {
                let head = entity_id(&r.source);
                let tail = entity_id(&r.target);
                let rel = relation_id(head, &r.predicate, tail);
                let head_uid = format!("{tenant}|{workspace}|{head}");
                let tail_uid = format!("{tenant}|{workspace}|{tail}");
                // Kuzu rel tables have no primary key, so idempotency is a
                // serialized check-then-create keyed on the deterministic
                // relation id (single semaphore permit => no race).
                let exists = {
                    let mut check = conn
                        .prepare(
                            "MATCH (a:Entity {uid:$head_uid})-[r:Relation]->(b:Entity {uid:$tail_uid}) WHERE r.id=$id RETURN r.id",
                        )
                        .map_err(|e| StorageError::Backend(e.to_string()))?;
                    let mut rows = conn
                        .execute(
                            &mut check,
                            vec![
                                ("head_uid", KValue::String(head_uid.clone())),
                                ("tail_uid", KValue::String(tail_uid.clone())),
                                ("id", KValue::String(rel.to_string())),
                            ],
                        )
                        .map_err(|e| StorageError::Backend(e.to_string()))?;
                    rows.next().is_some()
                };
                if exists {
                    continue;
                }
                let mut stmt = conn
                    .prepare(
                        "MATCH (a:Entity {uid:$head_uid}), (b:Entity {uid:$tail_uid}) CREATE (a)-[r:Relation {id:$id, tenant:$tenant, workspace:$workspace, predicate:$predicate, source_id:$source_id, version:$version}]->(b)",
                    )
                    .map_err(|e| StorageError::Backend(e.to_string()))?;
                conn.execute(
                    &mut stmt,
                    vec![
                        ("head_uid", KValue::String(head_uid)),
                        ("tail_uid", KValue::String(tail_uid)),
                        ("id", KValue::String(rel.to_string())),
                        ("tenant", KValue::String(tenant.clone())),
                        ("workspace", KValue::String(workspace.clone())),
                        ("predicate", KValue::String(r.predicate.clone())),
                        ("source_id", KValue::String(source_id.clone())),
                        ("version", KValue::Int64(version)),
                    ],
                )
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            }
            Ok(())
        })
        .await
    }

    async fn traverse(&self, scope: Scope, from: Uuid, max_hops: usize) -> StorageResult<Vec<Uuid>> {
        let hops = max_hops.min(self.max_hops);
        if hops == 0 {
            return Ok(Vec::new());
        }
        let from_uid = format!("{}|{}|{}", scope.tenant_id, scope.workspace_id, from);
        let tenant = scope.tenant_id.to_string();
        let workspace = scope.workspace_id.to_string();
        self.with_conn(move |conn| {
            let query = format!(
                "MATCH (a:Entity {{uid:$from_uid}})-[r:Relation*1..{hops}]->(b:Entity) WHERE b.tenant=$tenant AND b.workspace=$workspace RETURN DISTINCT b.id"
            );
            let mut stmt = conn
                .prepare(&query)
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            let result = conn
                .execute(
                    &mut stmt,
                    vec![
                        ("from_uid", KValue::String(from_uid)),
                        ("tenant", KValue::String(tenant)),
                        ("workspace", KValue::String(workspace)),
                    ],
                )
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            Ok(result
                .filter_map(|row| match row.first() {
                    Some(KValue::String(s)) => Uuid::parse_str(s).ok(),
                    _ => None,
                })
                .collect())
        })
        .await
    }

    async fn delete_source(&self, scope: Scope, source: SourceVersion) -> StorageResult<()> {
        let tenant = scope.tenant_id.to_string();
        let workspace = scope.workspace_id.to_string();
        let source_id = source.source_id.to_string();
        let version = source.version as i64;
        self.with_conn(move |conn| {
            let mut stmt = conn
                .prepare(
                    "MATCH (n:Entity) WHERE n.tenant=$tenant AND n.workspace=$workspace AND n.source_id=$source_id AND n.version=$version DETACH DELETE n",
                )
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            conn.execute(
                &mut stmt,
                vec![
                    ("tenant", KValue::String(tenant.clone())),
                    ("workspace", KValue::String(workspace.clone())),
                    ("source_id", KValue::String(source_id.clone())),
                    ("version", KValue::Int64(version)),
                ],
            )
            .map_err(|e| StorageError::Backend(e.to_string()))?;
            let mut stmt = conn
                .prepare(
                    "MATCH ()-[r:Relation]->() WHERE r.tenant=$tenant AND r.workspace=$workspace AND r.source_id=$source_id AND r.version=$version DELETE r",
                )
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            conn.execute(
                &mut stmt,
                vec![
                    ("tenant", KValue::String(tenant)),
                    ("workspace", KValue::String(workspace)),
                    ("source_id", KValue::String(source_id)),
                    ("version", KValue::Int64(version)),
                ],
            )
            .map_err(|e| StorageError::Backend(e.to_string()))?;
            Ok(())
        })
        .await
    }

    async fn list_ids(&self, scope: Scope) -> StorageResult<Value> {
        let tenant = scope.tenant_id.to_string();
        let workspace = scope.workspace_id.to_string();
        self.with_conn(move |conn| {
            let mut entities = Vec::new();
            let mut stmt = conn
                .prepare("MATCH (n:Entity) WHERE n.tenant=$tenant AND n.workspace=$workspace RETURN n.id")
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            let result = conn
                .execute(
                    &mut stmt,
                    vec![
                        ("tenant", KValue::String(tenant.clone())),
                        ("workspace", KValue::String(workspace.clone())),
                    ],
                )
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            for row in result {
                if let Some(KValue::String(s)) = row.first() {
                    if let Ok(id) = Uuid::parse_str(s) {
                        entities.push(id.to_string());
                    }
                }
            }
            let mut relations = Vec::new();
            let mut stmt = conn
                .prepare("MATCH ()-[r:Relation]->() WHERE r.tenant=$tenant AND r.workspace=$workspace RETURN r.id")
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            let result = conn
                .execute(
                    &mut stmt,
                    vec![
                        ("tenant", KValue::String(tenant)),
                        ("workspace", KValue::String(workspace)),
                    ],
                )
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            for row in result {
                if let Some(KValue::String(s)) = row.first() {
                    if let Ok(id) = Uuid::parse_str(s) {
                        relations.push(id.to_string());
                    }
                }
            }
            Ok(json!({"entities": entities, "relations": relations}))
        })
        .await
    }

    async fn delete_objects(
        &self,
        scope: Scope,
        entities: Vec<Uuid>,
        relations: Vec<Uuid>,
    ) -> StorageResult<()> {
        if entities.is_empty() && relations.is_empty() {
            return Ok(());
        }
        let tenant = scope.tenant_id.to_string();
        let workspace = scope.workspace_id.to_string();
        self.with_conn(move |conn| {
            for id in &entities {
                let mut stmt = conn
                    .prepare(
                        "MATCH (n:Entity) WHERE n.tenant=$tenant AND n.workspace=$workspace AND n.id=$id DETACH DELETE n",
                    )
                    .map_err(|e| StorageError::Backend(e.to_string()))?;
                conn.execute(
                    &mut stmt,
                    vec![
                        ("tenant", KValue::String(tenant.clone())),
                        ("workspace", KValue::String(workspace.clone())),
                        ("id", KValue::String(id.to_string())),
                    ],
                )
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            }
            for id in &relations {
                let mut stmt = conn
                    .prepare(
                        "MATCH ()-[r:Relation]->() WHERE r.tenant=$tenant AND r.workspace=$workspace AND r.id=$id DELETE r",
                    )
                    .map_err(|e| StorageError::Backend(e.to_string()))?;
                conn.execute(
                    &mut stmt,
                    vec![
                        ("tenant", KValue::String(tenant.clone())),
                        ("workspace", KValue::String(workspace.clone())),
                        ("id", KValue::String(id.to_string())),
                    ],
                )
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            }
            Ok(())
        })
        .await
    }
}

impl Lifecycle for KuzuStore {
    async fn initialize(&self) -> StorageResult<()> {
        self.with_conn(|conn| {
            conn.query(ENTITY_DDL)
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            conn.query(RELATION_DDL)
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            Ok(())
        })
        .await
    }

    async fn check(&self) -> StorageResult<()> {
        self.with_conn(|conn| {
            conn.prepare("MATCH (n:Entity) RETURN n.uid LIMIT 1")
                .map_err(|e| StorageError::Unavailable(format!("entity table missing: {e}")))?;
            conn.prepare("MATCH ()-[r:Relation]->() RETURN r.id LIMIT 1")
                .map_err(|e| StorageError::Unavailable(format!("relation table missing: {e}")))?;
            Ok(())
        })
        .await
    }

    async fn shutdown(&self) -> StorageResult<()> {
        Ok(())
    }
}
