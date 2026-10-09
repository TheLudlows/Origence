//! Explicit identity feature installation never rewrites legacy business data.
use origence::storage::{
    DomainTx, Lifecycle, RelationalStore, Scope, StorageError,
    sqlite::{MemoryIdentityUpgrade, SqliteStore},
};
use serde_json::json;
use sqlx::Connection;
use std::path::Path;
use uuid::Uuid;

async fn remove_identity_table(path: &Path) {
    let store = SqliteStore::open(path).await.unwrap();
    sqlx::query("DROP TABLE oc_memory_identities")
        .execute(store.pool())
        .await
        .unwrap();
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn explicit_upgrade_is_idempotent_and_preserves_legacy_versions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oc.db");
    let store = SqliteStore::open(&path).await.unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "legacy")
        .await
        .unwrap();
    let key = store.issue_key(scope, "admin").await.unwrap();
    let auth = store.authenticate(&key.token).await.unwrap();
    let mut tx = store.begin(auth.clone()).await.unwrap();
    let (asset, _) = tx.slot("legacy.release").await.unwrap();
    let source = tx
        .create_event("structured", "approval", None)
        .await
        .unwrap();
    tx.insert_version(asset, 1, "approval", "hash", source, None, None)
        .await
        .unwrap();
    tx.update_asset_version(asset, 1, None).await.unwrap();
    tx.commit().await.unwrap();
    sqlx::query("DROP TABLE oc_memory_identities")
        .execute(store.pool())
        .await
        .unwrap();
    store.shutdown().await.unwrap();
    drop(store);

    assert_eq!(
        SqliteStore::upgrade_memory_identity(&path, true)
            .await
            .unwrap(),
        MemoryIdentityUpgrade::Required
    );
    let store = SqliteStore::open(&path).await.unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE name='oc_memory_identities'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(count, 0);
    store.shutdown().await.unwrap();
    drop(store);
    assert_eq!(
        SqliteStore::upgrade_memory_identity(&path, false)
            .await
            .unwrap(),
        MemoryIdentityUpgrade::Enabled
    );
    assert_eq!(
        SqliteStore::upgrade_memory_identity(&path, false)
            .await
            .unwrap(),
        MemoryIdentityUpgrade::AlreadyEnabled
    );

    let store = SqliteStore::open(&path).await.unwrap();
    let mut tx = store.begin(auth).await.unwrap();
    let view = tx.asset_view(asset, Some(1)).await.unwrap();
    assert_eq!(view["content"], "approval");
    assert_eq!(view["source_event_id"], source.to_string());
    assert_eq!(view["identity"], json!(null));
    assert_eq!(tx.slot("legacy.release").await.unwrap(), (asset, Some(1)));
    let identity = serde_json::from_value(json!({"subject":{"kind":"service","stable_id":"billing"},"predicate":"release.approval","context":{}})).unwrap();
    assert_ne!(tx.identity_slot(&identity).await.unwrap().0, asset);
    tx.rollback().await.unwrap();
    let versions: i64 = sqlx::query_scalar("SELECT count(*) FROM oc_versions")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(versions, 1);
    let mappings: i64 = sqlx::query_scalar("SELECT count(*) FROM oc_memory_identities")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(mappings, 0);
}

#[tokio::test]
async fn missing_database_is_not_created_by_upgrade() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing/context.db");
    assert!(
        SqliteStore::upgrade_memory_identity(&path, false)
            .await
            .is_err()
    );
    assert!(!path.parent().unwrap().exists());
}

#[tokio::test]
async fn open_store_blocks_upgrade_in_the_same_process() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oc.db");
    let store = SqliteStore::open(&path).await.unwrap();
    assert!(matches!(
        SqliteStore::upgrade_memory_identity(&path, false).await,
        Err(StorageError::Conflict(_))
    ));
    drop(store);
    assert_eq!(
        SqliteStore::upgrade_memory_identity(&path, false)
            .await
            .unwrap(),
        MemoryIdentityUpgrade::AlreadyEnabled
    );
}

#[tokio::test]
async fn incompatible_base_is_not_repaired() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oc.db");
    remove_identity_table(&path).await;
    let store = SqliteStore::open(&path).await.unwrap();
    sqlx::query("ALTER TABLE oc_events RENAME COLUMN content TO incompatible_content")
        .execute(store.pool())
        .await
        .unwrap();
    store.shutdown().await.unwrap();
    drop(store);
    assert!(matches!(
        SqliteStore::upgrade_memory_identity(&path, false).await,
        Err(StorageError::Unavailable(_))
    ));
    let mut conn = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(&path),
    )
    .await
    .unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE name='oc_memory_identities'")
            .fetch_one(&mut conn)
            .await
            .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn identity_constraints_are_checked_without_repair() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oc.db");
    remove_identity_table(&path).await;
    let store = SqliteStore::open(&path).await.unwrap();
    sqlx::query("CREATE TABLE oc_memory_identities(tenant_id BLOB,workspace_id BLOB,identity_key TEXT,asset_id BLOB,identity_json TEXT,created_at INTEGER)")
        .execute(store.pool()).await.unwrap();
    assert!(matches!(
        store.check().await,
        Err(StorageError::Unavailable(_))
    ));
    store.shutdown().await.unwrap();
    drop(store);
    assert!(matches!(
        SqliteStore::upgrade_memory_identity(&path, false).await,
        Err(StorageError::Unavailable(_))
    ));
}

#[tokio::test]
async fn conflicting_view_is_not_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oc.db");
    remove_identity_table(&path).await;
    let store = SqliteStore::open(&path).await.unwrap();
    sqlx::query("CREATE VIEW oc_memory_identities AS SELECT 1 AS sentinel")
        .execute(store.pool())
        .await
        .unwrap();
    store.shutdown().await.unwrap();
    drop(store);
    assert!(matches!(
        SqliteStore::upgrade_memory_identity(&path, true).await,
        Err(StorageError::Unavailable(_))
    ));
    assert!(matches!(
        SqliteStore::upgrade_memory_identity(&path, false).await,
        Err(StorageError::Unavailable(_))
    ));
    let store = SqliteStore::open(&path).await.unwrap();
    let value: i64 = sqlx::query_scalar("SELECT sentinel FROM oc_memory_identities")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(value, 1);
}
