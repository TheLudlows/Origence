use origence::storage::{
    AuthorizedScope, DomainTx, Permission, RelationalStore, Scope, StorageError,
    sqlite::SqliteStore,
};
use serde_json::json;
use uuid::Uuid;

async fn provision(store: &SqliteStore, role: &str) -> AuthorizedScope {
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "evidence")
        .await
        .unwrap();
    let key = store.issue_key(scope, role).await.unwrap();
    store.authenticate(&key.token).await.unwrap()
}

#[tokio::test]
async fn source_visibility_is_scoped_and_retraction_blocks_reads() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("oc.db")).await.unwrap();
    let auth = provision(&store, "admin").await;
    let mut tx = store.begin(auth.clone()).await.unwrap();
    let source = tx
        .create_event("capture", "original source", None)
        .await
        .unwrap();
    assert_eq!(
        tx.event_view(source).await.unwrap()["content"],
        "original source"
    );
    tx.commit().await.unwrap();
    let other = provision(&store, "admin").await;
    let mut tx = store.begin(other).await.unwrap();
    assert!(matches!(
        tx.event_view(source).await,
        Err(StorageError::NotFound)
    ));
    tx.rollback().await.unwrap();
    let mut tx = store.begin(auth).await.unwrap();
    tx.retract_event(source).await.unwrap();
    assert!(matches!(
        tx.event_view(source).await,
        Err(StorageError::NotFound)
    ));
    tx.commit().await.unwrap();
    let reader = provision(&store, "reader").await;
    let mut tx = store.begin(reader).await.unwrap();
    assert!(matches!(
        tx.check_permission(Permission::Write).await,
        Err(StorageError::Forbidden)
    ));
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn deleted_source_file_blocks_original_event_view() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("oc.db")).await.unwrap();
    let auth = provision(&store, "admin").await;
    let mut tx = store.begin(auth).await.unwrap();
    let file = tx
        .insert_file("source.txt", "text/plain", "hash", 4)
        .await
        .unwrap();
    let source = tx
        .create_event("knowledge", "text", Some(file))
        .await
        .unwrap();
    assert_eq!(
        tx.event_view(source).await.unwrap()["file_id"],
        file.to_string()
    );
    tx.delete_file(file).await.unwrap();
    assert!(matches!(
        tx.event_view(source).await,
        Err(StorageError::NotFound)
    ));
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn normalization_status_does_not_claim_semantic_truth() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("oc.db")).await.unwrap();
    let auth = provision(&store, "admin").await;
    let mut tx = store.begin(auth).await.unwrap();
    let legacy = tx.slot("legacy").await.unwrap().0;
    let identity = serde_json::from_value(json!({"subject":{"kind":"service","stable_id":"billing"},"predicate":"release.approval","context":{}})).unwrap();
    let explicit = tx.identity_slot(&identity).await.unwrap().0;
    let knowledge = tx.insert_knowledge_asset("knowledge").await.unwrap();
    let source = tx.create_event("structured", "source", None).await.unwrap();
    for (asset, status) in [
        (legacy, "legacy_unidentified"),
        (explicit, "explicit_identity"),
        (knowledge, "not_applicable"),
    ] {
        tx.insert_version(asset, 1, "source", "hash", source, None, None)
            .await
            .unwrap();
        tx.update_asset_version(asset, 1, None).await.unwrap();
        assert_eq!(
            tx.asset_view(asset, None).await.unwrap()["normalization_status"],
            status
        );
    }
    tx.rollback().await.unwrap();
}
