//! The historical identity command inspects the current format without repair.
use origence::storage::native_turso as db;
use origence::storage::{
    DomainTx, Lifecycle, Permission, RelationalStore, Scope, StorageError,
    sqlite::{MemoryIdentityUpgrade, SqliteStore},
};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn identity_inspection_preserves_current_versions_permissions_and_sources() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oc.db");
    let store = SqliteStore::open(&path).await.unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "current format")
        .await
        .unwrap();
    let key = store.issue_key(scope, "admin").await.unwrap();
    let reader_key = store.issue_key(scope, "reader").await.unwrap();
    let auth = store.authenticate(&key.token).await.unwrap();
    let mut tx = store.begin(auth).await.unwrap();
    tx.check_permission(Permission::Write).await.unwrap();
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
    store.shutdown().await.unwrap();
    drop(store);

    for dry_run in [true, false, false] {
        assert_eq!(
            SqliteStore::upgrade_memory_identity(&path, dry_run)
                .await
                .unwrap(),
            MemoryIdentityUpgrade::AlreadyEnabled
        );
    }
    let store = SqliteStore::open(&path).await.unwrap();
    let auth = store.authenticate(&key.token).await.unwrap();
    let reader = store.authenticate(&reader_key.token).await.unwrap();
    assert_eq!(auth.scope, scope);
    assert_eq!(auth.role, "admin");
    assert_eq!(reader.scope, scope);
    assert_eq!(reader.role, "reader");
    let mut tx = store.begin_read(reader).await.unwrap();
    tx.check_permission(Permission::Read).await.unwrap();
    assert!(matches!(
        tx.check_permission(Permission::Write).await,
        Err(StorageError::Forbidden)
    ));
    tx.rollback().await.unwrap();
    let mut tx = store.begin(auth).await.unwrap();
    tx.check_permission(Permission::Write).await.unwrap();
    let view = tx.asset_view(asset, Some(1)).await.unwrap();
    assert_eq!(view["content"], "approval");
    assert_eq!(view["source_event_id"], source.to_string());
    assert_eq!(view["identity"], json!(null));
    assert_eq!(
        tx.event_content(source, true).await.unwrap().as_deref(),
        Some("approval")
    );
    assert_eq!(tx.slot("legacy.release").await.unwrap(), (asset, Some(1)));
    let identity = serde_json::from_value(json!({"subject":{"kind":"service","stable_id":"billing"},"predicate":"release.approval","context":{}})).unwrap();
    assert_ne!(tx.identity_slot(&identity).await.unwrap().0, asset);
    tx.rollback().await.unwrap();
    for (table, expected) in [
        ("oc_versions", 1),
        ("oc_events", 1),
        ("oc_assets", 1),
        ("oc_memory_identities", 0),
    ] {
        let count: i64 = db::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(count, expected, "{table}");
    }
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn missing_database_is_not_created_by_upgrade() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing/context.db");
    for dry_run in [true, false] {
        assert!(
            SqliteStore::upgrade_memory_identity(&path, dry_run)
                .await
                .is_err()
        );
        assert!(!path.parent().unwrap().exists());
    }
}

#[tokio::test]
async fn unmarked_nonempty_database_is_rejected_without_installing_a_format() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unmarked.db");
    let raw = db::Pool::open(&path).await.unwrap();
    db::raw_sql("CREATE TABLE sentinel(value TEXT NOT NULL); INSERT INTO sentinel VALUES('preserve original');")
        .execute(&raw).await.unwrap();
    raw.shutdown().await.unwrap();
    drop(raw);
    for dry_run in [true, false] {
        assert!(matches!(
            SqliteStore::upgrade_memory_identity(&path, dry_run).await,
            Err(StorageError::Unavailable(_))
        ));
    }
    assert!(matches!(
        SqliteStore::open(&path).await,
        Err(StorageError::Unavailable(_))
    ));
    let raw = db::Pool::open_read_only(&path).await.unwrap();
    let value: String = db::query_scalar("SELECT value FROM sentinel")
        .fetch_one(&raw)
        .await
        .unwrap();
    assert_eq!(value, "preserve original");
    let application_tables: i64 =
        db::query_scalar("SELECT count(*) FROM sqlite_master WHERE name GLOB 'oc_*'")
            .fetch_one(&raw)
            .await
            .unwrap();
    assert_eq!(application_tables, 0);
    raw.close().await;
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
    store.shutdown().await.unwrap();
    drop(store);
    assert_eq!(
        SqliteStore::upgrade_memory_identity(&path, false)
            .await
            .unwrap(),
        MemoryIdentityUpgrade::AlreadyEnabled
    );
}

#[tokio::test]
async fn incomplete_or_incompatible_identity_schema_is_not_repaired() {
    for replacement in [
        "",
        "CREATE TABLE oc_memory_identities(tenant_id BLOB,workspace_id BLOB,identity_key TEXT,asset_id BLOB,identity_json TEXT,created_at INTEGER)",
        "CREATE VIEW oc_memory_identities AS SELECT 1 AS sentinel",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("oc.db");
        let store = SqliteStore::open(&path).await.unwrap();
        db::query("DROP TABLE oc_memory_identities")
            .execute(store.pool())
            .await
            .unwrap();
        if !replacement.is_empty() {
            db::query(replacement).execute(store.pool()).await.unwrap();
        }
        let before: Option<(String, String)> =
            db::query_as("SELECT type,sql FROM sqlite_master WHERE name='oc_memory_identities'")
                .fetch_optional(store.pool())
                .await
                .unwrap();
        assert!(matches!(
            store.check().await,
            Err(StorageError::Unavailable(_))
        ));
        store.shutdown().await.unwrap();
        drop(store);
        for dry_run in [true, false] {
            assert!(matches!(
                SqliteStore::upgrade_memory_identity(&path, dry_run).await,
                Err(StorageError::Unavailable(_))
            ));
        }
        assert!(matches!(
            SqliteStore::open(&path).await,
            Err(StorageError::Unavailable(_))
        ));
        let raw = db::Pool::open_read_only(&path).await.unwrap();
        let after: Option<(String, String)> =
            db::query_as("SELECT type,sql FROM sqlite_master WHERE name='oc_memory_identities'")
                .fetch_optional(&raw)
                .await
                .unwrap();
        assert_eq!(after, before);
        if replacement.starts_with("CREATE VIEW") {
            let sentinel: i64 = db::query_scalar("SELECT sentinel FROM oc_memory_identities")
                .fetch_one(&raw)
                .await
                .unwrap();
            assert_eq!(sentinel, 1);
        }
        raw.close().await;
    }
}

#[tokio::test]
async fn incompatible_base_is_not_repaired() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oc.db");
    let store = SqliteStore::open(&path).await.unwrap();
    db::query("DROP TABLE oc_memory_identities")
        .execute(store.pool())
        .await
        .unwrap();
    db::query("ALTER TABLE oc_events RENAME COLUMN content TO incompatible_content")
        .execute(store.pool())
        .await
        .unwrap();
    store.shutdown().await.unwrap();
    drop(store);
    for dry_run in [true, false] {
        assert!(matches!(
            SqliteStore::upgrade_memory_identity(&path, dry_run).await,
            Err(StorageError::Unavailable(_))
        ));
    }
    assert!(matches!(
        SqliteStore::open(&path).await,
        Err(StorageError::Unavailable(_))
    ));
    let raw = db::Pool::open_read_only(&path).await.unwrap();
    let missing: i64 =
        db::query_scalar("SELECT count(*) FROM sqlite_master WHERE name='oc_memory_identities'")
            .fetch_one(&raw)
            .await
            .unwrap();
    assert_eq!(missing, 0);
    let schema: String = db::query_scalar("SELECT sql FROM sqlite_master WHERE name='oc_events'")
        .fetch_one(&raw)
        .await
        .unwrap();
    assert!(schema.contains("incompatible_content"));
    raw.close().await;
}
