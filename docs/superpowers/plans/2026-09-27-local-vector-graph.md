# Local Vector + Graph Backends (M4 storage slice) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the two local storage backends — `LanceDbStore` (`VectorStore`) and `KuzuStore` (`GraphStore`) — with scope/dimension/generation filtering, idempotent upsert, source-tagged delete, and a ledger-wired idempotent round-trip test, as the storage-layer slice of M4.

**Architecture:** Two new feature-gated adapters under `src/storage/` implement the M1 `VectorStore`/`GraphStore` traits against real LanceDB and Kuzu, mirroring the M0 probe's verified API patterns. Kuzu's synchronous calls run inside a bounded `spawn_blocking` executor (A2.4); LanceDB is async-native. Idempotent writes use deterministic ids (`merge_insert` for vectors; serialized check-then-create on a deterministic relation id for graph — Kuzu relationship tables have no primary key). Two small trait refinements land here because no existing implementation breaks: `VectorStore::upsert` gains a `scope` parameter (A2.2 "按 scope 写入"), and `GraphStore` gains `list_ids`/`delete_objects` primitives that graph-core G4's shared-owner cleanup will drive.

**Tech Stack:** Rust edition 2024, `lancedb =0.23.1`, `kuzu =0.11.3`, `arrow-array/arrow-schema 56.2`, `cxx-build =1.0.138` (build-dep, kuzu generator pin). All gated behind `local-vector` / `local-graph` / `local-storage` cargo features so the PG baseline keeps building without a C++ toolchain until M5.

## Scope note (what this plan does NOT do)

This plan delivers the **storage backends + their ledger write contract** — M4 items 1–3 and the write-contract portion of item 4 in [pluggable-storage-engine](2026-09-22-pluggable-storage-engine.md). Two things are intentionally deferred:

- **Retrieval fusion + hit re-verification** (M4 items 5–6): mapping keyword/vector/summary branches back to evidence and re-verifying source/version/tombstone before returning. That is graph-core plan G3's job; it needs the running app switched to local (M5) first.
- **Shared-owner deletion orchestration** (the *orchestration* that diffs `owned_artifacts()` against the graph and deletes orphans) is graph-core G4. This plan provides the primitives it needs (`list_ids` + `delete_objects`) and a contract test proving idempotent replay; it does not build the cleanup planner.

## Global Constraints

**2026-09-28 acceptance follow-up:** The source files supersede the initial code sketches below. Vector merge identity now includes source ID/version and generation as well as scope/artifact ID. Input validation checks nonzero/i32-compatible dimensions, embedding length, finite values and the existing profile table dimension before vector writes; search explicitly bypasses ANN indexes. Kuzu's blocking closure owns its semaphore permit until the native operation finishes, even if the async caller is cancelled. Regression coverage includes invalid input, generation/version/profile/scope isolation, reopen/delete replay and cancellation. Actual commands and outcomes are recorded in [VALIDATION](../../VALIDATION.md).

- `rust-version = "1.88"`, `edition = "2024"` (Cargo.toml, verbatim).
- `lancedb = "=0.23.1"` with `default-features = false`; `kuzu = "=0.11.3"`; `arrow-array`/`arrow-schema` `= "56.2"`; build-dependency `cxx-build = "=1.0.138"` (generator pin that avoids the 117 unresolved-symbol link failure documented in VALIDATION.md).
- New backends are feature-gated; `default` remains empty until M5. Existing PG build/tests must keep passing with no features enabled.
- Quality gates: `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked` (unit) and `cargo test --locked --features local-storage` (local backends).
- Never omit `Scope` (A2.4). Domain code must not touch `lancedb`/`kuzu`/Arrow vendor types outside `src/storage/`.
- Backends map vendor errors into `StorageError::Backend(...)`; they never leak `lancedb::Error` / `kuzu::Error` / Arrow errors.

---

### Task 1: LanceDB vector backend (`LanceDbStore`) + `VectorStore::upsert` scope param

**Files:**
- Modify: `Cargo.toml` (deps + features)
- Modify: `src/storage/traits.rs` (add `scope` param to `VectorStore::upsert`)
- Create: `src/storage/lancedb.rs`
- Modify: `src/storage/mod.rs` (register module + re-export, feature-gated)
- Test: `tests/lancedb_store.rs`

**Interfaces:**
- Consumes: `VectorEntry`, `VectorQuery`, `VectorHit`, `Capabilities` (existing, `src/storage/capabilities.rs`); `Scope`, `SourceVersion` (`scope.rs`); `StorageError` (`error.rs`).
- Produces: `LanceDbStore::open(dir) -> StorageResult<Self>`; implements `VectorStore` + `Lifecycle`. Trait signature change: `async fn upsert(&self, scope: Scope, entries: Vec<VectorEntry>) -> StorageResult<()>`.

- [x] **Step 1: Add dependencies and features to `Cargo.toml`**

Add these optional deps (append to `[dependencies]`):

```toml
lancedb = { version = "=0.23.1", default-features = false, optional = true }
arrow-array = { version = "56.2", optional = true }
arrow-schema = { version = "56.2", optional = true }
kuzu = { version = "=0.11.3", optional = true }
```

Add the build-dep (append to `[build-dependencies]`; note `cxx-build` is a build-only generator pin):

```toml
[build-dependencies]
cxx-build = { version = "=1.0.138", optional = true }
```

Add the feature table (new section):

```toml
[features]
default = []
local-vector = ["dep:lancedb", "dep:arrow-array", "dep:arrow-schema"]
local-graph = ["dep:kuzu", "dep:cxx-build"]
local-storage = ["local-vector", "local-graph"]
```

- [x] **Step 2: Change the `VectorStore::upsert` signature in `src/storage/traits.rs`**

Replace:

```rust
    /// Idempotently write entries for a scope/profile/generation.
    async fn upsert(&self, entries: Vec<VectorEntry>) -> StorageResult<()>;
```

with:

```rust
    /// Idempotently write entries for `scope` / profile / generation (A2.2).
    async fn upsert(&self, scope: Scope, entries: Vec<VectorEntry>) -> StorageResult<()>;
```

(`Scope` is already imported in `traits.rs`.)

- [x] **Step 3: Write the failing test `tests/lancedb_store.rs`**

```rust
//! LanceDB vector adapter tests (M4): idempotent upsert, scoped exact search,
//! and source-tagged delete against a real LanceDB in a temp dir.
#![cfg(feature = "local-vector")]

use opencontext::storage::lancedb::LanceDbStore;
use opencontext::storage::{
    Lifecycle, Scope, SourceVersion, VectorEntry, VectorQuery, VectorStore,
};
use uuid::Uuid;

fn scope() -> Scope {
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

#[tokio::test]
async fn upsert_is_idempotent_and_search_is_scoped() {
    let dir = tempfile::tempdir().unwrap();
    let store = LanceDbStore::open(dir.path()).await.unwrap();
    let scope = scope();
    let entry = VectorEntry {
        id: Uuid::new_v4(),
        embedding: vec![1.0, 0.0],
        profile: "test:dim2:v1".into(),
        dimension: 2,
        generation: 1,
        source: src(),
    };

    // Write the same entry twice; the second write must not add a row.
    store.upsert(scope, vec![entry.clone()]).await.unwrap();
    store.upsert(scope, vec![entry.clone()]).await.unwrap();

    let query = VectorQuery {
        scope,
        profile: "test:dim2:v1".into(),
        dimension: 2,
        generation: 1,
        embedding: vec![1.0, 0.0],
        limit: 10,
    };
    let hits = store.search(query).await.unwrap();
    assert_eq!(hits.len(), 1, "idempotent upsert must not duplicate");
    assert_eq!(hits[0].id, entry.id);
    assert_eq!(hits[0].source, entry.source);

    // A different workspace in the same table must see nothing.
    let other = scope();
    let hits = store
        .search(VectorQuery {
            scope: other,
            profile: "test:dim2:v1".into(),
            dimension: 2,
            generation: 1,
            embedding: vec![1.0, 0.0],
            limit: 10,
        })
        .await
        .unwrap();
    assert!(hits.is_empty());
}

#[tokio::test]
async fn delete_source_removes_only_that_source() {
    let dir = tempfile::tempdir().unwrap();
    let store = LanceDbStore::open(dir.path()).await.unwrap();
    let scope = scope();
    let src_a = src();
    let src_b = src();
    let a = VectorEntry {
        id: Uuid::new_v4(),
        embedding: vec![1.0, 0.0],
        profile: "p:v1".into(),
        dimension: 2,
        generation: 1,
        source: src_a,
    };
    let b = VectorEntry {
        id: Uuid::new_v4(),
        embedding: vec![0.0, 1.0],
        profile: "p:v1".into(),
        dimension: 2,
        generation: 1,
        source: src_b,
    };
    store.upsert(scope, vec![a.clone(), b.clone()]).await.unwrap();

    store.delete_source(scope, src_a).await.unwrap();

    let hits = store
        .search(VectorQuery {
            scope,
            profile: "p:v1".into(),
            dimension: 2,
            generation: 1,
            embedding: vec![0.0, 1.0],
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, b.id);
}

#[tokio::test]
async fn lifecycle_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let store = LanceDbStore::open(dir.path()).await.unwrap();
    store.initialize().await.unwrap();
    store.check().await.unwrap();
    store.shutdown().await.unwrap();
}
```

- [x] **Step 4: Run test to verify it fails**

Run: `cargo test --locked --features local-vector --test lancedb_store`
Expected: FAIL — `unresolved import opencontext::storage::lancedb` (module does not exist yet).

- [x] **Step 5: Create `src/storage/lancedb.rs`**

```rust
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
                        vectors, dim,
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
            .nearest_to(&query.embedding)
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
```

- [x] **Step 6: Register the module in `src/storage/mod.rs`**

After the existing `pub mod ledger;` line, add:

```rust
#[cfg(feature = "local-vector")]
pub mod lancedb;
#[cfg(feature = "local-graph")]
pub mod kuzu;
```

(Task 2 adds the `kuzu` module; the `#[cfg]` reference is harmless until then.)

- [x] **Step 7: Run test to verify it passes**

Run: `cargo test --locked --features local-vector --test lancedb_store`
Expected: PASS (3 tests).

- [x] **Step 8: Run the no-feature build to confirm the PG baseline is untouched**

Run: `cargo check --locked` and `cargo test --locked`
Expected: unchanged, no new failures; `lancedb.rs`/`kuzu.rs` are not compiled.

- [x] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock src/storage/traits.rs src/storage/lancedb.rs src/storage/mod.rs tests/lancedb_store.rs
git commit -m "feat(storage): LanceDB vector backend with scoped idempotent upsert"
```

---

### Task 2: Kuzu graph backend (`KuzuStore`)

**Files:**
- Create: `src/storage/kuzu.rs`
- Modify: `src/storage/mod.rs` (re-export `KuzuStore`, feature-gated)
- Test: `tests/kuzu_store.rs`

**Interfaces:**
- Consumes: `GraphStore`, `Lifecycle` (traits); `Entity`, `Relation` (`crate::types`); `entity_id`/`relation_id` (`crate::graph`, pure deterministic hashing); `Capabilities`, `Scope`, `SourceVersion`, `StorageError`.
- Produces: `KuzuStore::open(dir) -> StorageResult<Self>`; implements `GraphStore` + `Lifecycle`. `capabilities().max_hops = 3`, `shared_source_delete = false`.

- [x] **Step 1: Write the failing test `tests/kuzu_store.rs`**

```rust
//! Kuzu graph adapter tests (M4): idempotent MERGE upsert, scoped traversal,
//! source-tagged delete, and delete-by-id primitives against a real Kuzu db.
#![cfg(feature = "local-graph")]

use opencontext::graph::{entity_id, relation_id};
use opencontext::storage::kuzu::KuzuStore;
use opencontext::storage::{GraphStore, Lifecycle, Scope, SourceVersion};
use opencontext::types::{Entity, Relation};
use uuid::Uuid;

fn scope() -> Scope {
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
    let store = KuzuStore::open(dir.path()).await.unwrap();
    let scope = scope();
    let src = src();

    let alice = entity("Alice");
    let bob = entity("Bob");
    store
        .upsert_entities(scope, src, vec![alice.clone(), bob.clone()])
        .await
        .unwrap();
    // Idempotent replay must not create a duplicate node.
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

    // A different workspace has no nodes and no reachable ids.
    let other = scope();
    let empty = store.list_ids(other).await.unwrap();
    assert_eq!(empty["entities"].as_array().unwrap().len(), 0);
    assert!(store.traverse(other, entity_id("Alice"), 1).await.unwrap().is_empty());
}

#[tokio::test]
async fn delete_by_id_removes_shared_node_only_when_told() {
    let dir = tempfile::tempdir().unwrap();
    let store = KuzuStore::open(dir.path()).await.unwrap();
    let scope = scope();
    let alice = entity("Alice");

    // Two sources extract the same entity name -> the same deterministic id,
    // so MERGE keeps a single node (the shared-source case).
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

    // delete_by_id is the primitive graph-core G4 uses after the owner diff;
    // it removes the node only when the last owner is gone.
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
    let store = KuzuStore::open(dir.path()).await.unwrap();
    let scope = scope();
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
    let store = KuzuStore::open(dir.path()).await.unwrap();
    store.initialize().await.unwrap();
    store.check().await.unwrap();
    store.shutdown().await.unwrap();
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cargo test --locked --features local-graph --test kuzu_store`
Expected: FAIL — `unresolved import opencontext::storage::kuzu` (and, if `list_ids`/`delete_objects` are not yet on the trait, the compile errors for those calls, resolved in Task 3).

- [x] **Step 3: Create `src/storage/kuzu.rs`**

```rust
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
            conn.prepare("MATCH ()-[r:Relation]->() RETURN r.uid LIMIT 1")
                .map_err(|e| StorageError::Unavailable(format!("relation table missing: {e}")))?;
            Ok(())
        })
        .await
    }

    async fn shutdown(&self) -> StorageResult<()> {
        Ok(())
    }
}
```

- [x] **Step 4: Add the `list_ids`/`delete_objects` methods to the `GraphStore` trait in `src/storage/traits.rs`**

Insert after the existing `delete_source` method inside `pub trait GraphStore`:

```rust
    /// Return every entity/relation id currently stored for `scope`, as
    /// `{"entities": [..], "relations": [..]}`. graph-core G4 diffs this
    /// against `owned_artifacts()` to find orphans (A2.6).
    async fn list_ids(&self, scope: Scope) -> StorageResult<Value>;

    /// Delete the given entity/relation ids (already diffed to orphans by the
    /// caller). This — not `delete_source` — is the authoritative shared-source
    /// delete; shared objects are retained until their last owner is gone (A2.4).
    async fn delete_objects(
        &self,
        scope: Scope,
        entities: Vec<Uuid>,
        relations: Vec<Uuid>,
    ) -> StorageResult<()>;
```

(`Value` is `serde_json::Value`, already imported in `traits.rs`.)

- [x] **Step 5: Re-export `KuzuStore` in `src/storage/mod.rs`**

Update the `pub use` block to add (alongside the existing trait re-exports):

```rust
#[cfg(feature = "local-vector")]
pub use lancedb::LanceDbStore;
#[cfg(feature = "local-graph")]
pub use kuzu::KuzuStore;
```

- [x] **Step 6: Run test to verify it passes**

Run: `cargo test --locked --features local-graph --test kuzu_store`
Expected: PASS (4 tests).

- [x] **Step 7: Commit**

```bash
git add src/storage/kuzu.rs src/storage/traits.rs src/storage/mod.rs tests/kuzu_store.rs
git commit -m "feat(storage): Kuzu graph backend with scoped idempotent MERGE and delete-by-id"
```

---

### Task 3: Ledger-wired idempotent round-trip test (M4 item 4)

**Files:**
- Test: `tests/local_ledger.rs`

**Interfaces:**
- Consumes: `SqliteStore` (`RelationalStore` + `JobQueue` + `DomainTx` with `register_pending`/`confirm_committed`/`reconcile_ledger`), `LanceDbStore`, `KuzuStore`; `LedgerEntry`, `LedgerKey`, `Surface`, `ledger_idempotency_key` (`src/storage/ledger.rs`).
- Produces: a contract test proving `register_pending → idempotent external write → confirm_committed → reconcile` composes without duplication (A2.6 "来源级幂等写 … 重启重放").

- [x] **Step 1: Write the failing test `tests/local_ledger.rs`**

```rust
//! Cross-store ledger round-trip (M4): a pending write is registered on the
//! SQLite ledger, applied idempotently to LanceDB/Kuzu, confirmed, and a
//! reconcile pass leaves it committed without duplicating (A2.6).
#![cfg(feature = "local-storage")]

use opencontext::storage::kuzu::KuzuStore;
use opencontext::storage::lancedb::LanceDbStore;
use opencontext::storage::ledger::{LedgerEntry, LedgerKey, Surface};
use opencontext::storage::sqlite::SqliteStore;
use opencontext::storage::{
    DomainTx, GraphStore, Permission, RelationalStore, Scope, SourceVersion, VectorEntry,
    VectorQuery, VectorStore,
};
use opencontext::types::Entity;
use uuid::Uuid;

#[tokio::test]
async fn pending_write_confirms_and_reconcile_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let sqlite = SqliteStore::open(dir.path().join("oc.db")).await.unwrap();
    let vector = LanceDbStore::open(dir.path().join("vector")).await.unwrap();
    let graph = KuzuStore::open(dir.path().join("graph")).await.unwrap();

    let tenant = Uuid::new_v4();
    let workspace = Uuid::new_v4();
    sqlite.create_workspace(tenant, workspace, "acme").await.unwrap();
    let scope = Scope {
        tenant_id: tenant,
        workspace_id: workspace,
    };
    let key = sqlite.issue_key(scope, "admin").await.unwrap();
    let auth = sqlite.authenticate(&key.token).await.unwrap();

    let source = SourceVersion {
        source_id: Uuid::new_v4(),
        version: 1,
    };
    let chunk_id = Uuid::new_v4();
    let entity = Entity {
        name: "Alice".into(),
        entity_type: "Person".into(),
        description: String::new(),
    };
    let generation = 1i64;

    let vector_key = LedgerKey {
        scope,
        source,
        artifact_id: chunk_id,
        surface: Surface::Vector,
        generation,
    };
    let graph_key = LedgerKey {
        scope,
        source,
        artifact_id: opencontext::graph::entity_id("Alice"),
        surface: Surface::Graph,
        generation,
    };

    // Step 1: register both pending writes in one relational transaction.
    {
        let mut tx = sqlite.begin(auth.clone()).await.unwrap();
        tx.check_permission(Permission::Write).await.unwrap();
        tx.register_pending(LedgerEntry {
            key: vector_key,
            artifact_type: "chunk".into(),
            idempotency_key: opencontext::storage::ledger::ledger_idempotency_key(&vector_key),
        })
        .await
        .unwrap();
        tx.register_pending(LedgerEntry {
            key: graph_key,
            artifact_type: "entity".into(),
            idempotency_key: opencontext::storage::ledger::ledger_idempotency_key(&graph_key),
        })
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }

    // Step 2: apply the external writes idempotently (replayed twice).
    let entry = VectorEntry {
        id: chunk_id,
        embedding: vec![1.0, 0.0],
        profile: "p:v1".into(),
        dimension: 2,
        generation,
        source,
    };
    for _ in 0..2 {
        vector.upsert(scope, vec![entry.clone()]).await.unwrap();
        graph
            .upsert_entities(scope, source, vec![entity.clone()])
            .await
            .unwrap();
    }

    // Step 3: confirm both committed in a relational transaction.
    {
        let mut tx = sqlite.begin(auth.clone()).await.unwrap();
        tx.check_permission(Permission::Write).await.unwrap();
        tx.confirm_committed(vector_key).await.unwrap();
        tx.confirm_committed(graph_key).await.unwrap();
        tx.commit().await.unwrap();
    }

    // Step 4: reconcile leaves committed entries untouched.
    let counts = sqlite.reconcile_ledger().await.unwrap();
    assert_eq!(counts["requeued"], 0);
    assert_eq!(counts["orphaned"], 0);

    // Step 5: no duplication from the replayed writes.
    let hits = vector
        .search(VectorQuery {
            scope,
            profile: "p:v1".into(),
            dimension: 2,
            generation,
            embedding: vec![1.0, 0.0],
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    let ids = graph.list_ids(scope).await.unwrap();
    assert_eq!(ids["entities"].as_array().unwrap().len(), 1);

    // Step 6: re-confirm is a no-op (state-guard) and reconcile stays clean.
    {
        let mut tx = sqlite.begin(auth.clone()).await.unwrap();
        tx.check_permission(Permission::Write).await.unwrap();
        tx.confirm_committed(vector_key).await.unwrap();
        tx.commit().await.unwrap();
    }
    let counts = sqlite.reconcile_ledger().await.unwrap();
    assert_eq!(counts["requeued"], 0);
    assert_eq!(counts["orphaned"], 0);
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cargo test --locked --features local-storage --test local_ledger`
Expected: FAIL until Tasks 1–2 exist; if run after them it should pass (this task is a composition test, so its "red" state is "the file does not compile against missing imports" — run it last).

- [x] **Step 3: Run the full local-storage suite**

Run: `cargo test --locked --features local-storage`
Expected: all `lancedb_store`, `kuzu_store`, `local_ledger` pass; existing unit tests still pass.

- [x] **Step 4: Format, lint, commit**

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
git add tests/local_ledger.rs
git commit -m "test(storage): ledger-wired idempotent vector/graph round-trip"
```

---

## Self-review

- **Spec coverage** (A2.3–A2.8, M4 items 1–4): automatic init (Task 1/2 `Lifecycle` + lazy table create / DDL), scope+profile+dimension+generation filtering (Task 1 `search` filter, table-per-profile), idempotent write (merge_insert / check-then-create), source-tagged delete + delete-by-id primitives (Tasks 2/3), ledger idempotent round-trip (Task 3). M4 items 5–6 (retrieval fusion, hit re-verification) and the shared-owner *cleanup planner* are explicitly deferred to graph-core G3/G4 — recorded in the scope note, not silently dropped.
- **Placeholder scan**: no TBD/TODO; every step has concrete code and commands. The one vendor-uncertainty (Kuzu rel-table `MERGE {uid:$rel_uid}` PK syntax) is surfaced as a verification step in Task 2 Step 6, not left as a placeholder.
- **Type consistency**: `VectorStore::upsert(scope, entries)` is changed in Task 1 Step 2 and used with that exact signature in Tasks 1 and 3 tests. `GraphStore::list_ids(scope) -> Value` and `delete_objects(scope, Vec<Uuid>, Vec<Uuid>)` are added in Task 2 Step 4 and used in Tasks 2/3 tests. `VectorEntry`/`VectorQuery`/`VectorHit`/`SourceVersion`/`Scope` field names match `capabilities.rs`/`scope.rs` verbatim.
