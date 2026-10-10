# Replace Kuzu Graph Backend with SQLite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Kuzu (native C++ graph DB) `GraphStore` implementation with a pure-Rust SQLite-backed `SqliteGraphStore`, keeping the `GraphStore` trait, `StorageEngine`, domain code, and all observable behavior bit-for-bit unchanged.

**Architecture:** Add a new `SqliteGraphStore` that stores entities and relations in two SQLite tables and reproduces Kuzu's exact semantics (idempotent `MERGE` upsert, scoped recursive-CTE traversal, `DETACH DELETE` cascades, last-writer source tagging, and the `snapshot()` JSON shape). Wire it into `LocalEngine`, then delete Kuzu and its `local-graph` feature plus the CMake/Ninja/C++ toolchain requirements it dragged in.

**Tech Stack:** Rust (edition 2024, MSRV 1.98.0), sqlx 0.8 (SQLite, WAL), tokio, uuid, serde_json. Pure SQL — no native deps added.

## Global Constraints

- All cargo commands use `--locked` (CI enforces this). Never edit `Cargo.lock` by hand.
- Clippy must pass `cargo clippy --locked --all-targets -- -D warnings` (CI gate); no warnings allowed.
- `cargo fmt --all -- --check` must pass.
- Feature layout after this change: `default = ["local-storage"]`, `local-vector = ["dep:lancedb", "dep:arrow-array", "dep:arrow-schema"]`, `local-storage = ["local-vector"]`. The `local-graph` feature is removed entirely.
- Graph ids are deterministic and scope-independent, from `crate::graph`: `entity_id(name) = uuid(sha256(name.trim().to_lowercase()))`, `relation_id(head, predicate, tail) = uuid(sha256(head_uuid || predicate.trim().to_lowercase() || tail_uuid))`. Do not recompute these differently.
- Entity `uid` (the primary key) is `format!("{tenant}|{workspace}|{entity_id}")`.
- Kuzu semantics to preserve exactly:
  - `upsert_entities` = `MERGE … SET …` → last-writer-wins on `source_id`/`version`.
  - `upsert_relations` = check-then-create, skip if exists → first-writer-wins (do NOT overwrite an existing relation).
  - `delete_objects` on an entity = `DETACH DELETE` → also removes incident relations.
  - `delete_source` = delete matching entities (with cascade) then delete remaining matching relations.
  - `traverse` = variable-length 1..hops paths, `DISTINCT` endpoint id, endpoint filtered by scope, start node excluded.
  - `snapshot` = inherent method (NOT on the `GraphStore` trait); `retrieval.rs` calls it via `engine.graph().snapshot(scope)` and must keep compiling unchanged.
- Every graph write auto-commits (no long-running transaction): `Service::open → worker::cleanup` reaps un-committed graph objects on restart via the cross-store ledger, and that behavior must survive the swap.
- Graph data is derived and re-publishable; no on-disk migration is required. The path changes from `dir/graph/kuzu.db` to `dir/graph.db`.
- End git commit messages with `Co-Authored-By: Claude Code <noreply@anthropic.com>`.

---

## File Structure

**Create:**
- `src/storage/sqlite-graph-schema.sql` — the two-table DDL (entity + relation).
- `src/storage/sqlite_graph.rs` — `SqliteGraphStore`: `open`, `snapshot`, `GraphStore` impl (6 methods), `Lifecycle` impl.
- `tests/sqlite_graph_store.rs` — port of the 5 Kuzu adapter tests to the SQLite store.

**Modify:**
- `src/storage/mod.rs` — add `sqlite_graph` module, swap `LocalEngine` generic, swap `pub use`.
- `src/service.rs:59` — construct `SqliteGraphStore` instead of `KuzuStore`.
- `Cargo.toml` — drop `kuzu`/`cxx-build`, drop `local-graph` feature.
- `tests/local_ledger.rs` — import + construct `SqliteGraphStore`.

**Delete:**
- `tests/kuzu_store.rs` (replaced by `tests/sqlite_graph_store.rs`).
- `tools/storage-probe/src/graph.rs` (probe's Kuzu backend).

**Toolchain/CI (Task 5):**
- `.github/workflows/ci.yml`, `Dockerfile`, `tools/storage-probe/Cargo.toml`, `tools/storage-probe/src/main.rs`, `tools/storage-probe/verify.py`.

---

### Task 1: Add `SqliteGraphStore` side-by-side with Kuzu

Add the new store and its tests WITHOUT touching Kuzu or `LocalEngine`. Both backends compile and pass independently. This proves behavioral parity before any wiring is changed.

**Files:**
- Create: `src/storage/sqlite-graph-schema.sql`
- Create: `src/storage/sqlite_graph.rs`
- Create: `tests/sqlite_graph_store.rs`
- Modify: `src/storage/mod.rs` (additive only: register the new module and `pub use`)

**Interfaces:**
- Produces: `origence::storage::sqlite_graph::SqliteGraphStore` with `pub async fn open(path) -> StorageResult<Self>`, `pub async fn snapshot(&self, scope) -> StorageResult<Value>`, plus `GraphStore` and `Lifecycle` impls. Later tasks consume these exact names.

- [ ] **Step 1: Create the schema file**

`src/storage/sqlite-graph-schema.sql`:

```sql
-- Graph tables backing SqliteGraphStore (M4). Fixed column projections are
-- validated by Lifecycle::check and never repaired at runtime (A2.3).
CREATE TABLE IF NOT EXISTS oc_graph_entities (
    uid TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    id TEXT NOT NULL,
    name TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    description TEXT NOT NULL,
    source_id TEXT NOT NULL,
    version INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS oc_graph_entities_scope
    ON oc_graph_entities(tenant_id, workspace_id);
CREATE INDEX IF NOT EXISTS oc_graph_entities_source
    ON oc_graph_entities(tenant_id, workspace_id, source_id, version);

CREATE TABLE IF NOT EXISTS oc_graph_relations (
    id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    head_uid TEXT NOT NULL,
    tail_uid TEXT NOT NULL,
    predicate TEXT NOT NULL,
    source_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    PRIMARY KEY (head_uid, tail_uid, id)
);
CREATE INDEX IF NOT EXISTS oc_graph_relations_scope
    ON oc_graph_relations(tenant_id, workspace_id);
CREATE INDEX IF NOT EXISTS oc_graph_relations_head
    ON oc_graph_relations(head_uid);
CREATE INDEX IF NOT EXISTS oc_graph_relations_source
    ON oc_graph_relations(tenant_id, workspace_id, source_id, version);
CREATE INDEX IF NOT EXISTS oc_graph_relations_by_id
    ON oc_graph_relations(tenant_id, workspace_id, id);
```

- [ ] **Step 2: Create the store implementation**

`src/storage/sqlite_graph.rs`:

```rust
//! SQLite graph backend (M4, replaces the Kuzu adapter).
//!
//! Entities and relations live in two tables inside one SQLite database file.
//! The relational store already holds the process's single-writer lock on the
//! data directory, so this store takes no file lock of its own. Writes
//! auto-commit per call (matching Kuzu's per-connection behavior); the
//! cross-store ledger — not this store — is the authority for what stays
//! visible, and `Service::open` reaps un-committed objects on restart (A2.6).

use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use uuid::Uuid;

use crate::graph::{entity_id, relation_id};
use crate::storage::capabilities::Capabilities;
use crate::storage::error::{StorageError, StorageResult};
use crate::storage::scope::{Scope, SourceVersion};
use crate::storage::traits::{GraphStore, Lifecycle};
use crate::types::{Entity, Relation};

const SCHEMA: &str = include_str!("sqlite-graph-schema.sql");

/// A graph backend owning a single SQLite database file.
pub struct SqliteGraphStore {
    pool: SqlitePool,
    max_hops: usize,
}

fn sqlite_err(e: sqlx::Error) -> StorageError {
    StorageError::Backend(e.to_string())
}

fn entity_uid(scope: Scope, id: Uuid) -> String {
    format!("{}|{}|{}", scope.tenant_id, scope.workspace_id, id)
}

impl SqliteGraphStore {
    /// Open the store at `path` (a SQLite file), creating the file and schema
    /// when absent. Fails without repairing if the schema is incompatible.
    pub async fn open(path: impl AsRef<Path>) -> StorageResult<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .busy_timeout(Duration::from_secs(5))
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .map_err(sqlite_err)?;
        let store = Self { pool, max_hops: 3 };
        store.initialize().await?;
        Ok(store)
    }

    /// Scoped graph identities for hybrid recall. Mirrors the Kuzu adapter's
    /// `snapshot` exactly: `id`/`name` for entities and
    /// `id`/`source_id`/`target_id`/`fact_text` for relations, every name and
    /// predicate lower-cased and trimmed, both lists sorted by `id`.
    pub async fn snapshot(&self, scope: Scope) -> StorageResult<Value> {
        let tenant = scope.tenant_id.to_string();
        let workspace = scope.workspace_id.to_string();
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, name FROM oc_graph_entities WHERE tenant_id = ? AND workspace_id = ?",
        )
        .bind(&tenant)
        .bind(&workspace)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlite_err)?;
        let mut entities: Vec<Value> = rows
            .into_iter()
            .map(|(id, name)| json!({"id": id, "name": name.trim().to_lowercase()}))
            .collect();
        entities.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));

        let rows: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
            "SELECT r.id, e1.id, e2.id, e1.name, r.predicate, e2.name \
             FROM oc_graph_relations r \
             JOIN oc_graph_entities e1 ON e1.uid = r.head_uid \
             JOIN oc_graph_entities e2 ON e2.uid = r.tail_uid \
             WHERE r.tenant_id = ? AND r.workspace_id = ?",
        )
        .bind(&tenant)
        .bind(&workspace)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlite_err)?;
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

        Ok(json!({"entities": entities, "relations": relations}))
    }
}

impl GraphStore for SqliteGraphStore {
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
            sqlx::query(
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
            .map_err(sqlite_err)?;
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
            // Kuzu relation tables have no PK; idempotency is a serialized
            // check-then-create that skips on an existing relation (first-writer
            // wins). ON CONFLICT DO NOTHING reproduces that exactly.
            sqlx::query(
                "INSERT INTO oc_graph_relations(id, tenant_id, workspace_id, head_uid, tail_uid, predicate, source_id, version) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT(head_uid, tail_uid, id) DO NOTHING",
            )
            .bind(rel.to_string())
            .bind(&tenant)
            .bind(&workspace)
            .bind(&head_uid)
            .bind(&tail_uid)
            .bind(&r.predicate)
            .bind(&source_id)
            .bind(version)
            .execute(&self.pool)
            .await
            .map_err(sqlite_err)?;
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
        let ids: Vec<String> = sqlx::query_scalar(
            "WITH RECURSIVE reach(uid, depth) AS ( \
                SELECT ?, 0 \
                UNION ALL \
                SELECT r.tail_uid, reach.depth + 1 \
                FROM oc_graph_relations r JOIN reach ON r.head_uid = reach.uid \
                WHERE reach.depth < ? \
             ) \
             SELECT DISTINCT e.id \
             FROM reach JOIN oc_graph_entities e ON e.uid = reach.uid \
             WHERE reach.depth > 0 AND e.tenant_id = ? AND e.workspace_id = ?",
        )
        .bind(from_uid)
        .bind(hops as i64)
        .bind(tenant)
        .bind(workspace)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlite_err)?;
        Ok(ids.into_iter().filter_map(|s| Uuid::parse_str(&s).ok()).collect())
    }

    async fn delete_source(&self, scope: Scope, source: SourceVersion) -> StorageResult<()> {
        let tenant = scope.tenant_id.to_string();
        let workspace = scope.workspace_id.to_string();
        let source_id = source.source_id.to_string();
        let version = source.version as i64;
        // DETACH DELETE equivalent: drop incident relations of matching
        // entities, then the entities, then any remaining matching relations.
        sqlx::query(
            "DELETE FROM oc_graph_relations \
             WHERE head_uid IN (SELECT uid FROM oc_graph_entities WHERE tenant_id = ? AND workspace_id = ? AND source_id = ? AND version = ?) \
                OR tail_uid IN (SELECT uid FROM oc_graph_entities WHERE tenant_id = ? AND workspace_id = ? AND source_id = ? AND version = ?)",
        )
        .bind(&tenant)
        .bind(&workspace)
        .bind(&source_id)
        .bind(version)
        .bind(&tenant)
        .bind(&workspace)
        .bind(&source_id)
        .bind(version)
        .execute(&self.pool)
        .await
        .map_err(sqlite_err)?;
        sqlx::query(
            "DELETE FROM oc_graph_entities WHERE tenant_id = ? AND workspace_id = ? AND source_id = ? AND version = ?",
        )
        .bind(&tenant)
        .bind(&workspace)
        .bind(&source_id)
        .bind(version)
        .execute(&self.pool)
        .await
        .map_err(sqlite_err)?;
        sqlx::query(
            "DELETE FROM oc_graph_relations WHERE tenant_id = ? AND workspace_id = ? AND source_id = ? AND version = ?",
        )
        .bind(&tenant)
        .bind(&workspace)
        .bind(&source_id)
        .bind(version)
        .execute(&self.pool)
        .await
        .map_err(sqlite_err)?;
        Ok(())
    }

    async fn list_ids(&self, scope: Scope) -> StorageResult<Value> {
        let tenant = scope.tenant_id.to_string();
        let workspace = scope.workspace_id.to_string();
        let entities: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM oc_graph_entities WHERE tenant_id = ? AND workspace_id = ? ORDER BY id",
        )
        .bind(&tenant)
        .bind(&workspace)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlite_err)?;
        let relations: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM oc_graph_relations WHERE tenant_id = ? AND workspace_id = ? ORDER BY id",
        )
        .bind(&tenant)
        .bind(&workspace)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlite_err)?;
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
            let uid = entity_uid(scope, *id);
            // DETACH DELETE: remove incident relations before the node.
            sqlx::query("DELETE FROM oc_graph_relations WHERE head_uid = ? OR tail_uid = ?")
                .bind(&uid)
                .bind(&uid)
                .execute(&self.pool)
                .await
                .map_err(sqlite_err)?;
            sqlx::query(
                "DELETE FROM oc_graph_entities WHERE tenant_id = ? AND workspace_id = ? AND id = ?",
            )
            .bind(&tenant)
            .bind(&workspace)
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(sqlite_err)?;
        }
        for id in &relations {
            sqlx::query(
                "DELETE FROM oc_graph_relations WHERE tenant_id = ? AND workspace_id = ? AND id = ?",
            )
            .bind(&tenant)
            .bind(&workspace)
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(sqlite_err)?;
        }
        Ok(())
    }
}

impl Lifecycle for SqliteGraphStore {
    async fn initialize(&self) -> StorageResult<()> {
        let mut conn = self.pool.acquire().await.map_err(sqlite_err)?;
        let present: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name IN ('oc_graph_entities','oc_graph_relations'))",
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(sqlite_err)?;
        // Create only when the database is empty, matching Kuzu's "create only
        // if show_tables is empty" so a partial schema is never repaired.
        if !present {
            sqlx::raw_sql(SCHEMA).execute(&mut *conn).await.map_err(sqlite_err)?;
        }
        self.check().await
    }

    async fn check(&self) -> StorageResult<()> {
        for (table, columns) in [
            (
                "oc_graph_entities",
                "uid,tenant_id,workspace_id,id,name,entity_type,description,source_id,version",
            ),
            (
                "oc_graph_relations",
                "id,tenant_id,workspace_id,head_uid,tail_uid,predicate,source_id,version",
            ),
        ] {
            sqlx::query(&format!("SELECT {columns} FROM {table} LIMIT 0"))
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| {
                    StorageError::Unavailable(format!("incompatible graph schema in {table}: {e}"))
                })?;
        }
        Ok(())
    }

    async fn shutdown(&self) -> StorageResult<()> {
        self.pool.close().await;
        Ok(())
    }
}
```

- [ ] **Step 3: Register the module (additive; leave Kuzu in place)**

In `src/storage/mod.rs`, after line 25 (`pub mod error;`) add the module, and after line 65 (`pub use kuzu::KuzuStore;`) add the re-export. Keep all existing Kuzu lines for now.

```rust
pub mod error;
#[cfg(feature = "local-storage")]
pub mod sqlite_graph;
```

and

```rust
#[cfg(feature = "local-graph")]
pub use kuzu::KuzuStore;
#[cfg(feature = "local-storage")]
pub use sqlite_graph::SqliteGraphStore;
```

- [ ] **Step 4: Port the adapter tests**

`tests/sqlite_graph_store.rs` (full file; tests 1–4 are copied from `tests/kuzu_store.rs` with the store swapped, test 5 is rewritten for SQLite):

```rust
//! SQLite graph adapter tests (M4): idempotent upsert, scoped traversal,
//! source-tagged delete, and delete-by-id primitives against a real SQLite db.
#![cfg(feature = "local-storage")]

use origence::graph::{entity_id, relation_id};
use origence::storage::sqlite_graph::SqliteGraphStore;
use origence::storage::{GraphStore, Lifecycle, Scope, SourceVersion};
use origence::types::{Entity, Relation};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use uuid::Uuid;

fn make_scope() -> Scope {
    Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    }
}

fn src() -> SourceVersion {
    SourceVersion {
        source_id: Uuid::new_v4(),
        version: 1,
    }
}

fn entity(name: &str) -> Entity {
    Entity {
        name: name.into(),
        entity_type: "Person".into(),
        description: String::new(),
    }
}

#[tokio::test]
async fn upsert_is_idempotent_and_traverse_is_scoped() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteGraphStore::open(dir.path().join("graph.db")).await.unwrap();
    let scope = make_scope();
    let src = src();

    let alice = entity("Alice");
    let bob = entity("Bob");
    store
        .upsert_entities(scope, src, vec![alice.clone(), bob.clone()])
        .await
        .unwrap();
    store
        .upsert_entities(scope, src, vec![alice.clone(), bob.clone()])
        .await
        .unwrap();

    let rel = Relation {
        source: "Alice".into(),
        predicate: "knows".into(),
        target: "Bob".into(),
    };
    store
        .upsert_relations(scope, src, vec![rel.clone()])
        .await
        .unwrap();
    store
        .upsert_relations(scope, src, vec![rel.clone()])
        .await
        .unwrap();

    let ids = store.list_ids(scope).await.unwrap();
    assert_eq!(ids["entities"].as_array().unwrap().len(), 2);
    assert_eq!(ids["relations"].as_array().unwrap().len(), 1);
    assert_eq!(
        ids["relations"][0].as_str().unwrap(),
        relation_id(entity_id("Alice"), "knows", entity_id("Bob")).to_string()
    );

    let reached = store.traverse(scope, entity_id("Alice"), 1).await.unwrap();
    assert_eq!(reached, vec![entity_id("Bob")]);

    let other = make_scope();
    let empty = store.list_ids(other).await.unwrap();
    assert_eq!(empty["entities"].as_array().unwrap().len(), 0);
    assert!(
        store
            .traverse(other, entity_id("Alice"), 1)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn delete_by_id_removes_shared_node_only_when_told() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteGraphStore::open(dir.path().join("graph.db")).await.unwrap();
    let scope = make_scope();
    let alice = entity("Alice");

    store
        .upsert_entities(scope, src(), vec![alice.clone()])
        .await
        .unwrap();
    store
        .upsert_entities(scope, src(), vec![alice.clone()])
        .await
        .unwrap();
    assert_eq!(
        store.list_ids(scope).await.unwrap()["entities"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    store
        .delete_objects(scope, vec![entity_id("Alice")], vec![])
        .await
        .unwrap();
    assert_eq!(
        store.list_ids(scope).await.unwrap()["entities"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn source_tagged_delete_removes_last_writer() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteGraphStore::open(dir.path().join("graph.db")).await.unwrap();
    let scope = make_scope();
    let src_a = src();
    store
        .upsert_entities(scope, src_a, vec![entity("Alice")])
        .await
        .unwrap();
    store.delete_source(scope, src_a).await.unwrap();
    assert_eq!(
        store.list_ids(scope).await.unwrap()["entities"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn lifecycle_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteGraphStore::open(dir.path().join("graph.db")).await.unwrap();
    store.initialize().await.unwrap();
    store.check().await.unwrap();
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn partial_graph_schema_is_rejected_without_repair() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.db");
    {
        let options = SqliteConnectOptions::new().filename(&path).create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE oc_graph_entities (uid TEXT PRIMARY KEY)")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }
    // open() runs initialize (no repair) then check (must fail on the partial
    // column projection).
    assert!(SqliteGraphStore::open(&path).await.is_err());
    let options = SqliteConnectOptions::new().filename(&path);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    let tables: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name LIKE 'oc_graph_%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tables, 1); // the partial entities table was not repaired or extended
}
```

- [ ] **Step 5: Run the new test file and confirm it passes**

Run: `cargo test --locked --test sqlite_graph_store`
Expected: 5 passed (the existing `tests/kuzu_store.rs` is untouched and still passes independently).

- [ ] **Step 6: Commit**

```bash
git add src/storage/sqlite-graph-schema.sql src/storage/sqlite_graph.rs src/storage/mod.rs tests/sqlite_graph_store.rs
git commit -m "test(s0): add SQLite graph backend alongside Kuzu with parity tests

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: Swap `LocalEngine` and `Service` to the SQLite store

Point the assembled engine at `SqliteGraphStore`. Kuzu is still compiled (as a reference) but no longer used at runtime.

**Files:**
- Modify: `src/storage/mod.rs` (`LocalEngine` alias)
- Modify: `src/service.rs:59`

**Interfaces:**
- Consumes: `SqliteGraphStore` from Task 1.
- Produces: `LocalEngine`'s graph slot is now `SqliteGraphStore`; `service.engine.graph()` returns `&SqliteGraphStore` so `snapshot()` in `retrieval.rs:98` keeps compiling unchanged.

- [ ] **Step 1: Change the `LocalEngine` type alias**

In `src/storage/mod.rs`, change the alias body (lines 42–48) so the graph parameter is `SqliteGraphStore`:

```rust
#[cfg(feature = "local-storage")]
pub type LocalEngine = StorageEngine<
    std::sync::Arc<sqlite::SqliteStore>,
    std::sync::Arc<sqlite::SqliteStore>,
    LanceDbStore,
    SqliteGraphStore,
    local_blob::LocalBlobStore,
>;
```

- [ ] **Step 2: Change the construction site**

In `src/service.rs:59`, replace:

```rust
KuzuStore::open(dir.join("graph")).await?,
```

with:

```rust
SqliteGraphStore::open(dir.join("graph.db")).await?,
```

(`SqliteGraphStore` is already in scope via the `storage::*` glob import.)

- [ ] **Step 3: Run the full test suite**

Run: `cargo test --locked --no-fail-fast`
Expected: all tests pass, including `sqlite_graph_store`, `local_app`, and `local_ledger` (Kuzu still compiles, so `kuzu_store` also passes). The `local_app` persistence/orphan-reap test at `tests/local_app.rs:1409-1516` is the key integration proof that the ledger-driven cleanup still works over the SQLite graph.

- [ ] **Step 4: Commit**

```bash
git add src/storage/mod.rs src/service.rs
git commit -m "refactor(s0): wire SQLite graph backend into LocalEngine

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: Remove Kuzu, the `local-graph` feature, and the old tests

Delete the native backend now that nothing references it. This is the compile-time payoff: `cmake`/`cxx-build`/`kuzu` leave the dependency graph.

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/storage/mod.rs` (remove `kuzu` module + re-export)
- Delete: `tests/kuzu_store.rs`
- Modify: `tests/local_ledger.rs`

**Interfaces:**
- Produces: no `kuzu` or `local-graph` symbols anywhere; `local-storage` implies only `local-vector`.

- [ ] **Step 1: Edit `Cargo.toml`**

Remove the Kuzu dependency line from `[dependencies]`:

```toml
kuzu = { version = "=0.11.3", optional = true }
```

Remove the entire `[build-dependencies]` section (its only member was `cxx-build`):

```toml
[build-dependencies]
cxx-build = { version = "=1.0.138", optional = true }
```

Change the `[features]` section to:

```toml
[features]
default = ["local-storage"]
local-vector = ["dep:lancedb", "dep:arrow-array", "dep:arrow-schema"]
local-storage = ["local-vector"]
```

- [ ] **Step 2: Remove the Kuzu module and re-export**

In `src/storage/mod.rs`, delete:

```rust
#[cfg(feature = "local-graph")]
pub mod kuzu;
```

and delete:

```rust
#[cfg(feature = "local-graph")]
pub use kuzu::KuzuStore;
```

- [ ] **Step 3: Delete the old test file**

Run: `rm tests/kuzu_store.rs`

- [ ] **Step 4: Update `tests/local_ledger.rs`**

Replace line 6:

```rust
use origence::storage::kuzu::KuzuStore;
```

with:

```rust
use origence::storage::sqlite_graph::SqliteGraphStore;
```

Replace line 22:

```rust
let graph = KuzuStore::open(dir.path().join("graph")).await.unwrap();
```

with:

```rust
let graph = SqliteGraphStore::open(dir.path().join("graph.db")).await.unwrap();
```

- [ ] **Step 5: Verify the dependency graph no longer contains Kuzu or cmake**

Run: `cargo tree --locked -i kuzu` (expects: no matches / "kuzu not found"), then `grep -E '^name = "(kuzu|cmake|cxx-build)"' Cargo.lock` (expects: no output).

- [ ] **Step 6: Run the full suite and lint**

Run: `cargo test --locked --no-fail-fast` and `cargo clippy --locked --all-targets -- -D warnings`
Expected: all pass, zero warnings.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src/storage/mod.rs tests/local_ledger.rs
git rm tests/kuzu_store.rs
git commit -m "refactor(s0): remove Kuzu and the local-graph feature

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: Drop the Kuzu-only native toolchain from the storage probe

The `tools/storage-probe` crate (a separate workspace) had its own Kuzu graph backend. Remove it so nothing in the repo references Kuzu. The probe's `probe-protoc` bin (used by `tools/build.ps1` to locate `protoc`) is unaffected.

**Files:**
- Modify: `tools/storage-probe/Cargo.toml`
- Modify: `tools/storage-probe/src/main.rs`
- Delete: `tools/storage-probe/src/graph.rs`
- Modify: `tools/storage-probe/verify.py`

- [ ] **Step 1: Edit `tools/storage-probe/Cargo.toml`**

Remove `kuzu = { version = "=0.11.3", optional = true }` from `[dependencies]`, remove the `graph = ["dep:kuzu", "dep:cxx-build"]` feature line, remove the `[build-dependencies]` section (`cxx-build`), and change the default features to:

```toml
[features]
default = ["sqlite", "vector"]
```

- [ ] **Step 2: Edit `tools/storage-probe/src/main.rs`**

Delete the module declaration:

```rust
#[cfg(feature = "graph")]
mod graph;
```

and delete the match arm:

```rust
#[cfg(feature = "graph")]
"kuzu" => graph::run(path, request),
```

- [ ] **Step 3: Delete the probe's graph backend**

Run: `rm tools/storage-probe/src/graph.rs`

- [ ] **Step 4: Remove the graph section from `tools/storage-probe/verify.py`**

Open `tools/storage-probe/verify.py`, delete the `probe.graph()` method and its call site (the graph/Kuzu verification block around line 215), keeping the sqlite and vector sections intact.

- [ ] **Step 5: Confirm the protoc probe still builds**

Run: `cargo build --manifest-path tools/storage-probe/Cargo.toml --locked --no-default-features --features build-tools --bin probe-protoc`
Expected: builds successfully (this is the exact invocation `tools/build.ps1` uses).

- [ ] **Step 6: Commit**

```bash
git add tools/storage-probe/Cargo.toml tools/storage-probe/Cargo.lock tools/storage-probe/src/main.rs tools/storage-probe/verify.py
git rm tools/storage-probe/src/graph.rs
git commit -m "refactor(s0): remove Kuzu from the storage probe

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 5: Strip Kuzu's CMake/C++ toolchain from CI and the container

Now that Kuzu is gone, CMake and Ninja are no longer needed. LanceDB still needs `protoc` (its `lance-*` crates use `prost-build`), so protobuf stays.

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `Dockerfile`

- [ ] **Step 1: Edit `.github/workflows/ci.yml` — `linux-native` job**

Remove `CMAKE_GENERATOR: Ninja` and `CMAKE_BUILD_PARALLEL_LEVEL: 2` from the job's `env:`. Change the build-tools step to drop `cmake ninja-build`:

```yaml
      - name: Linux native build tools
        run: |
          sudo apt-get update && sudo apt-get install -y protobuf-compiler libprotobuf-dev
          protoc --proto_path=/usr/include --descriptor_set_out=/tmp/protobuf-check.pb google/protobuf/empty.proto
```

Update the comment on the clippy step from "Rust and CMake each use two workers…" to just "Keep link memory pressure bounded.".

- [ ] **Step 2: Edit `.github/workflows/ci.yml` — `macos` job**

Remove `CMAKE_GENERATOR: Ninja` and `CMAKE_BUILD_PARALLEL_LEVEL: 2` from the job's `env:`. Change:

```yaml
        run: brew install cmake ninja protobuf
```

to:

```yaml
        run: brew install protobuf
```

- [ ] **Step 3: Edit `Dockerfile`**

Change the build-stage apt install (line 3) to drop `cmake ninja-build`:

```dockerfile
RUN apt-get update && apt-get install -y --no-install-recommends protobuf-compiler libprotobuf-dev \
    && rm -rf /var/lib/apt/lists/*
```

Remove the `ENV CMAKE_GENERATOR=Ninja` and `ENV CMAKE_BUILD_PARALLEL_LEVEL=2` lines (Dockerfile lines 11–13), and update the adjacent comment to remove the CMake mention.

- [ ] **Step 4: Confirm a clean local build with no native toolchain assumptions**

Run: `cargo build --locked`
Expected: builds with only `cc` (for `libsqlite3-sys`/`ring`) and `protoc` (for `prost-build`); no CMake/Ninja invocation.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/ci.yml Dockerfile
git commit -m "ci(s0): drop CMake/Ninja now that Kuzu is removed

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 6: Full verification and acceptance

Final gate: format, lint, the whole suite, the HTTP smoke, and the retrieval evaluation (which must be bit-for-bit unchanged in hybrid mode).

- [ ] **Step 1: Format and lint**

Run: `cargo fmt --all -- --check`
Run: `cargo clippy --locked --all-targets -- -D warnings`
Expected: both pass.

- [ ] **Step 2: Full test suite (default features)**

Run: `cargo test --locked --no-fail-fast`
Expected: all tests pass.

- [ ] **Step 3: No-native-features fast path still compiles**

Run: `cargo clippy --locked --no-default-features --all-targets -- -D warnings`
Run: `cargo test --locked --no-default-features --lib`
Expected: both pass (the `--no-default-features` CI job must still work with `local-storage` off).

- [ ] **Step 4: HTTP smoke**

Run: `cargo build --locked && python3 tools/smoke.py --binary target/debug/origence`
Expected: smoke passes.

- [ ] **Step 5: Retrieval evaluation determinism**

Run: `python3 evals/run.py --binary target/debug/origence --commit <sha> --output <tmp-dir>`
Expected: the keyword/vector/hybrid results match the previous Kuzu-backed run (the seed evaluation is keyword-only, but hybrid `snapshot()` output must be structurally identical).

- [ ] **Step 6: Commit and note acceptance**

```bash
git add -A
git commit -m "docs(s0): archive SQLite graph backend acceptance

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

## Self-Review Notes

- **Spec coverage:** every `GraphStore` method (upsert_entities, upsert_relations, traverse, delete_source, list_ids, delete_objects) plus `Lifecycle` and the off-trait `snapshot()` are implemented (Task 1) and parity-tested (Task 1 tests + Task 2 `local_app` integration). Feature/dep removal is Task 3; probe and CI toolchain are Tasks 4–5; acceptance is Task 6.
- **Placeholder scan:** no TBD/TODO; all code blocks are complete and compilable.
- **Type consistency:** `SqliteGraphStore::open`, `snapshot`, and the `GraphStore`/`Lifecycle` impls are named identically in Tasks 1, 2, 3, and 4. `entity_uid` is used consistently across all methods.
