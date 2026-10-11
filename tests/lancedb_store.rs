//! LanceDB vector adapter tests (M4): idempotent upsert, scoped exact search,
//! and source-tagged delete against a real LanceDB in a temp dir.
#![cfg(feature = "local-vector")]

use origence::storage::lancedb::LanceDbStore;
use origence::storage::{
    Lifecycle, Scope, SourceVersion, StorageError, VectorEntry, VectorQuery, VectorStore,
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
        artifact_ids: None,
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
            artifact_ids: None,
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
            artifact_ids: None,
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

fn entry() -> VectorEntry {
    VectorEntry {
        id: Uuid::new_v4(),
        embedding: vec![1.0, 0.0],
        profile: "p:v1".into(),
        dimension: 2,
        generation: 1,
        source: src(),
    }
}

fn query(scope: Scope, entry: &VectorEntry) -> VectorQuery {
    VectorQuery {
        artifact_ids: None,
        scope,
        profile: entry.profile.clone(),
        dimension: entry.dimension,
        generation: entry.generation,
        embedding: entry.embedding.clone(),
        limit: 10,
    }
}

#[tokio::test]
async fn invalid_vectors_are_rejected_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let store = LanceDbStore::open(dir.path()).await.unwrap();
    let scope = make_scope();
    let valid = entry();
    for (dimension, embedding) in [
        (0, vec![]),
        (3, vec![1.0, 0.0]),
        (2, vec![f32::NAN, 0.0]),
        (2, vec![f32::INFINITY, 0.0]),
        (usize::MAX, vec![1.0]),
    ] {
        let invalid = VectorEntry {
            dimension,
            embedding,
            profile: "invalid".into(),
            ..valid.clone()
        };
        assert!(matches!(
            store
                .upsert(scope, vec![valid.clone(), invalid.clone()])
                .await,
            Err(StorageError::Conflict(_))
        ));
        assert!(store.search(query(scope, &valid)).await.unwrap().is_empty());
        assert!(matches!(
            store.search(query(scope, &invalid)).await,
            Err(StorageError::Conflict(_))
        ));
    }
}

#[tokio::test]
async fn profile_dimension_is_checked_on_write_and_search() {
    let dir = tempfile::tempdir().unwrap();
    let store = LanceDbStore::open(dir.path()).await.unwrap();
    let scope = make_scope();
    let valid = entry();
    store.upsert(scope, vec![valid.clone()]).await.unwrap();
    let wrong = VectorEntry {
        dimension: 3,
        embedding: vec![1.0, 0.0, 0.0],
        ..valid.clone()
    };
    assert!(matches!(
        store.upsert(scope, vec![wrong.clone()]).await,
        Err(StorageError::Conflict(_))
    ));
    assert!(matches!(
        store.search(query(scope, &wrong)).await,
        Err(StorageError::Conflict(_))
    ));
    let mut mismatch = query(scope, &valid);
    mismatch.dimension = 3;
    assert!(matches!(
        store.search(mismatch).await,
        Err(StorageError::Conflict(_))
    ));
    let mut zero = query(scope, &valid);
    zero.limit = 0;
    assert!(store.search(zero).await.unwrap().is_empty());
    assert_eq!(store.search(query(scope, &valid)).await.unwrap().len(), 1);
}

#[tokio::test]
async fn replay_preserves_generation_source_version_profile_and_scope() {
    let dir = tempfile::tempdir().unwrap();
    let store = LanceDbStore::open(dir.path()).await.unwrap();
    let scope = make_scope();
    let a = entry();
    let newer = VectorEntry {
        generation: 2,
        ..a.clone()
    };
    let version = VectorEntry {
        source: SourceVersion {
            version: 2,
            ..a.source
        },
        ..a.clone()
    };
    let profile = VectorEntry {
        profile: "other".into(),
        ..a.clone()
    };
    let other_tenant = Scope {
        tenant_id: Uuid::new_v4(),
        ..scope
    };
    let other_workspace = Scope {
        workspace_id: Uuid::new_v4(),
        ..scope
    };
    for _ in 0..2 {
        store
            .upsert(
                scope,
                vec![a.clone(), newer.clone(), version.clone(), profile.clone()],
            )
            .await
            .unwrap();
        store.upsert(other_tenant, vec![a.clone()]).await.unwrap();
        store
            .upsert(other_workspace, vec![a.clone()])
            .await
            .unwrap();
    }
    drop(store);
    let store = LanceDbStore::open(dir.path()).await.unwrap();
    assert_eq!(store.search(query(scope, &a)).await.unwrap().len(), 2);
    assert_eq!(store.search(query(scope, &newer)).await.unwrap().len(), 1);
    assert_eq!(store.search(query(scope, &profile)).await.unwrap().len(), 1);
    for _ in 0..2 {
        store.delete_source(scope, a.source).await.unwrap();
    }
    let remaining = store.search(query(scope, &a)).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].source, version.source);
    assert!(store.search(query(scope, &newer)).await.unwrap().is_empty());
    assert!(
        store
            .search(query(scope, &profile))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store.search(query(other_tenant, &a)).await.unwrap().len(),
        1
    );
    assert_eq!(
        store
            .search(query(other_workspace, &a))
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn relational_candidates_filter_before_top_k() {
    let dir = tempfile::tempdir().unwrap();
    let store = LanceDbStore::open(dir.path()).await.unwrap();
    let scope = make_scope();
    let hidden = entry();
    let mut visible = entry();
    visible.embedding = vec![0.0, 1.0];
    store
        .upsert(scope, vec![hidden.clone(), visible.clone()])
        .await
        .unwrap();
    let mut q = query(scope, &hidden);
    q.limit = 1;
    q.artifact_ids = Some(vec![visible.id]);
    let hits = store.search(q.clone()).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, visible.id);
    q.artifact_ids = Some(vec![]);
    assert!(store.search(q).await.unwrap().is_empty());
}

#[tokio::test]
async fn long_lived_reader_sees_other_connection_commits() {
    let dir = tempfile::tempdir().unwrap();
    let reader = LanceDbStore::open(dir.path()).await.unwrap();
    let writer = LanceDbStore::open(dir.path()).await.unwrap();
    let scope = make_scope();
    let other = Scope {
        workspace_id: Uuid::new_v4(),
        ..scope
    };
    let mut value = entry();
    let q = query(scope, &value);
    assert!(reader.search(q.clone()).await.unwrap().is_empty());

    value.embedding = vec![0.0, 1.0];
    writer.upsert(scope, vec![value.clone()]).await.unwrap();
    writer.upsert(other, vec![value.clone()]).await.unwrap();
    let first = reader.search(q.clone()).await.unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].source, value.source);
    assert!((first[0].score - 1.0 / 3.0).abs() < 1e-6);

    value.embedding = q.embedding.clone();
    writer.upsert(scope, vec![value.clone()]).await.unwrap();
    let updated = reader.search(q.clone()).await.unwrap();
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].id, value.id);
    assert_eq!(updated[0].score, 1.0);

    let next = VectorEntry {
        generation: 2,
        ..value.clone()
    };
    writer.upsert(scope, vec![next.clone()]).await.unwrap();
    assert_eq!(reader.search(q.clone()).await.unwrap().len(), 1);
    assert_eq!(reader.search(query(scope, &next)).await.unwrap().len(), 1);
    writer.delete_source(scope, value.source).await.unwrap();
    assert!(reader.search(q).await.unwrap().is_empty());
    assert!(reader.search(query(scope, &next)).await.unwrap().is_empty());
    assert_eq!(reader.search(query(other, &value)).await.unwrap().len(), 1);
}

#[tokio::test]
async fn offline_compaction_preserves_results_versions_and_rejects_live_host() {
    use origence::storage::{SqliteStore, maintenance::compact_vectors};
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing");
    assert!(matches!(
        compact_vectors(&missing, false).await,
        Err(StorageError::NotFound)
    ));
    assert!(!missing.exists());
    let relational = SqliteStore::open(dir.path().join("context.db"))
        .await
        .unwrap();
    let store = LanceDbStore::open(dir.path().join("vectors"))
        .await
        .unwrap();
    let scope = make_scope();
    let base = entry();
    let q = VectorQuery {
        limit: 100,
        embedding: vec![-1.0, 0.0],
        ..query(scope, &base)
    };
    for i in 0..16 {
        let value = VectorEntry {
            embedding: vec![i as f32, 0.0],
            ..entry()
        };
        store.upsert(scope, vec![value]).await.unwrap();
    }
    let expected = serde_json::to_value(store.search(q.clone()).await.unwrap()).unwrap();
    assert!(matches!(
        compact_vectors(dir.path(), true).await,
        Err(StorageError::Conflict(_))
    ));
    drop(store);
    relational.shutdown().await.unwrap();
    drop(relational);
    let preview = compact_vectors(dir.path(), true).await.unwrap();
    assert_eq!(preview.tables.len(), 1);
    assert_eq!(preview.tables[0].before.rows, 16);
    assert!(preview.tables[0].before.fragments > 1);
    assert!(preview.tables[0].after.is_none());
    let same = compact_vectors(dir.path(), true).await.unwrap();
    assert_eq!(preview.tables[0].before, same.tables[0].before);
    let compacted = compact_vectors(dir.path(), false).await.unwrap();
    let after = compacted.tables[0].after.as_ref().unwrap();
    assert_eq!(after.rows, 16);
    assert_eq!(after.fragments, 1);
    assert!(compacted.historical_versions_retained);
    let repeated = compact_vectors(dir.path(), false).await.unwrap();
    assert_eq!(repeated.tables[0].after.as_ref().unwrap(), after);
    let store = LanceDbStore::open(dir.path().join("vectors"))
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(store.search(q).await.unwrap()).unwrap(),
        expected
    );
    let db = lancedb::connect(dir.path().join("vectors").to_str().unwrap())
        .execute()
        .await
        .unwrap();
    let names = db.table_names().execute().await.unwrap();
    let table = db.open_table(&names[0]).execute().await.unwrap();
    table
        .checkout(preview.tables[0].before.version)
        .await
        .unwrap();
    assert_eq!(table.count_rows(None).await.unwrap(), 16);
    store.upsert(scope, vec![entry()]).await.unwrap();
    drop(store);
    table.checkout_latest().await.unwrap();
    let unmodified_version = table.version().await.unwrap();
    // Validate all tables before compacting any: an unexpected table must block the run.
    db.create_empty_table(
        "zz_foreign_table",
        std::sync::Arc::new(arrow_schema::Schema::empty()),
    )
    .execute()
    .await
    .unwrap();
    assert!(matches!(
        compact_vectors(dir.path(), false).await,
        Err(StorageError::InvalidArgument(_))
    ));
    table.checkout_latest().await.unwrap();
    assert_eq!(table.version().await.unwrap(), unmodified_version);
}
