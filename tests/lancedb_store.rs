//! LanceDB vector adapter tests (M4): idempotent upsert, scoped exact search,
//! and source-tagged delete against a real LanceDB in a temp dir.
#![cfg(feature = "local-vector")]

use opencontext::storage::lancedb::LanceDbStore;
use opencontext::storage::{
    Lifecycle, Scope, SourceVersion, VectorEntry, VectorQuery, VectorStore,
};
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

#[tokio::test]
async fn upsert_is_idempotent_and_search_is_scoped() {
    let dir = tempfile::tempdir().unwrap();
    let store = LanceDbStore::open(dir.path()).await.unwrap();
    let scope = make_scope();
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
    let other = make_scope();
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
    let scope = make_scope();
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
    store
        .upsert(scope, vec![a.clone(), b.clone()])
        .await
        .unwrap();

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
