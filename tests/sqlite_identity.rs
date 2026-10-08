//! Structured identity persistence and legacy compatibility.
use opencontext::memory_identity::{MemoryIdentity, MemorySubject, SubjectKind};
use opencontext::storage::{
    AuthorizedScope, DomainTx, Lifecycle, RelationalStore, Scope, StorageError, sqlite::SqliteStore,
};
use sqlx::Connection;
use std::collections::BTreeMap;
use uuid::Uuid;

async fn provision(store: &SqliteStore) -> (AuthorizedScope, Scope) {
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "identity")
        .await
        .unwrap();
    let key = store.issue_key(scope, "admin").await.unwrap();
    (store.authenticate(&key.token).await.unwrap(), scope)
}

fn identity() -> MemoryIdentity {
    MemoryIdentity {
        subject: MemorySubject {
            kind: SubjectKind::Service,
            stable_id: "billing".into(),
        },
        predicate: "release.approval".into(),
        context: BTreeMap::from([("environment".into(), "production".into())]),
    }
}

#[tokio::test]
async fn reuse_publish_and_isolate_identity() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("oc.db")).await.unwrap();
    let (auth, scope) = provision(&store).await;
    let i = identity();
    let mut tx = store.begin(auth.clone()).await.unwrap();
    let (asset, version) = tx.identity_slot(&i).await.unwrap();
    assert_eq!(version, None);
    assert_eq!(tx.identity_slot(&i).await.unwrap(), (asset, None));
    let source = tx
        .create_event("identified", "approval", None)
        .await
        .unwrap();
    tx.insert_version(asset, 1, "approval", "hash", source, None, None)
        .await
        .unwrap();
    tx.update_asset_version(asset, 1, None).await.unwrap();
    tx.commit().await.unwrap();
    let mut tx = store.begin(auth).await.unwrap();
    assert_eq!(tx.identity_slot(&i).await.unwrap(), (asset, Some(1)));
    let view = tx.asset_view(asset, None).await.unwrap();
    assert_eq!(view["identity"], serde_json::to_value(&i).unwrap());
    assert!(matches!(
        tx.slot(&i.key(scope).unwrap()).await,
        Err(StorageError::Conflict(_))
    ));
    let mut different = i.clone();
    different.subject.stable_id = "shipping".into();
    assert_ne!(tx.identity_slot(&different).await.unwrap().0, asset);
    different = i.clone();
    different
        .context
        .insert("environment".into(), "staging".into());
    assert_ne!(tx.identity_slot(&different).await.unwrap().0, asset);
    tx.commit().await.unwrap();
    let (other, _) = provision(&store).await;
    let mut tx = store.begin(other).await.unwrap();
    assert_ne!(tx.identity_slot(&i).await.unwrap().0, asset);
    assert!(matches!(
        tx.asset_view(asset, None).await,
        Err(StorageError::NotFound)
    ));
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn tombstone_blocks_identity_recreation() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("oc.db")).await.unwrap();
    let (auth, _) = provision(&store).await;
    let mut tx = store.begin(auth).await.unwrap();
    let i = identity();
    let (asset, _) = tx.identity_slot(&i).await.unwrap();
    tx.delete_asset(asset).await.unwrap();
    assert!(matches!(
        tx.identity_slot(&i).await,
        Err(StorageError::Conflict(_))
    ));
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn legacy_collision_is_not_reinterpreted() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("oc.db")).await.unwrap();
    let (auth, scope) = provision(&store).await;
    let mut tx = store.begin(auth).await.unwrap();
    let i = identity();
    let key = i.key(scope).unwrap();
    let legacy = tx.slot(&key).await.unwrap();
    assert!(matches!(
        tx.identity_slot(&i).await,
        Err(StorageError::Conflict(_))
    ));
    assert_eq!(tx.slot(&key).await.unwrap(), legacy);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn old_database_is_not_upgraded_implicitly() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oc.db");
    let store = SqliteStore::open(&path).await.unwrap();
    let (auth, _) = provision(&store).await;
    store.shutdown().await.unwrap();
    let mut conn = sqlx::SqliteConnection::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    sqlx::query("DROP TABLE oc_memory_identities")
        .execute(&mut conn)
        .await
        .unwrap();
    drop(conn);
    let store = SqliteStore::open(&path).await.unwrap();
    store.check().await.unwrap();
    let mut tx = store.begin(auth).await.unwrap();
    tx.slot("legacy.fact").await.unwrap();
    assert!(matches!(
        tx.identity_slot(&identity()).await,
        Err(StorageError::Unavailable(_))
    ));
    tx.commit().await.unwrap();
    store.shutdown().await.unwrap();
    let mut conn = sqlx::SqliteConnection::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='oc_memory_identities')",
    )
    .fetch_one(&mut conn)
    .await
    .unwrap();
    assert!(!exists);
}
