//! The identity check inspects the current format read-only without repair.
use origence::storage::native_turso as db;
use origence::storage::{
    DomainTx, Lifecycle, Permission, RelationalStore, Scope, StorageError, sqlite::SqliteStore,
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

    let bytes = std::fs::read(&path).unwrap();
    for _ in 0..2 {
        SqliteStore::check_memory_identity(&path).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
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
    for (table, expected) in [("oc_versions", 1), ("oc_events", 1), ("oc_assets", 1)] {
        let count: i64 = db::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(count, expected, "{table}");
    }
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn missing_database_is_not_created_by_check() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing/context.db");
    assert!(SqliteStore::check_memory_identity(&path).await.is_err());
    assert!(!path.parent().unwrap().exists());
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
    let bytes = std::fs::read(&path).unwrap();
    assert!(matches!(
        SqliteStore::check_memory_identity(&path).await,
        Err(StorageError::Unavailable(_))
    ));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
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
async fn open_store_blocks_check_in_the_same_process() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oc.db");
    let store = SqliteStore::open(&path).await.unwrap();
    assert!(matches!(
        SqliteStore::check_memory_identity(&path).await,
        Err(StorageError::Conflict(_))
    ));
    store.shutdown().await.unwrap();
    drop(store);
    SqliteStore::check_memory_identity(&path).await.unwrap();
}

#[tokio::test]
async fn incomplete_or_incompatible_identity_schema_is_not_repaired() {
    for replacement in [
        "",
        "CREATE INDEX oc_memory_identity_slot ON oc_assets(identity_key)",
        "CREATE TABLE oc_memory_identities(tenant_id BLOB,workspace_id BLOB,identity_key TEXT,asset_id BLOB,identity_json TEXT,created_at INTEGER)",
        "CREATE VIEW oc_memory_identities AS SELECT 1 AS sentinel",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("oc.db");
        let store = SqliteStore::open(&path).await.unwrap();
        db::query("DROP INDEX oc_memory_identity_slot")
            .execute(store.pool())
            .await
            .unwrap();
        if !replacement.is_empty() {
            db::query(replacement).execute(store.pool()).await.unwrap();
        }
        let before: Vec<(String, String)> =
            db::query_as("SELECT type,sql FROM sqlite_master WHERE name IN ('oc_memory_identities','oc_memory_identity_slot') ORDER BY name")
                .fetch_all(store.pool())
                .await
                .unwrap();
        assert!(matches!(
            store.check().await,
            Err(StorageError::Unavailable(_))
        ));
        store.shutdown().await.unwrap();
        drop(store);
        let bytes = std::fs::read(&path).unwrap();
        assert!(matches!(
            SqliteStore::check_memory_identity(&path).await,
            Err(StorageError::Unavailable(_))
        ));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(matches!(
            SqliteStore::open(&path).await,
            Err(StorageError::Unavailable(_))
        ));
        let raw = db::Pool::open_read_only(&path).await.unwrap();
        let after: Vec<(String, String)> =
            db::query_as("SELECT type,sql FROM sqlite_master WHERE name IN ('oc_memory_identities','oc_memory_identity_slot') ORDER BY name")
                .fetch_all(&raw)
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
    db::query("ALTER TABLE oc_events RENAME COLUMN content TO incompatible_content")
        .execute(store.pool())
        .await
        .unwrap();
    store.shutdown().await.unwrap();
    drop(store);
    let bytes = std::fs::read(&path).unwrap();
    assert!(matches!(
        SqliteStore::check_memory_identity(&path).await,
        Err(StorageError::Unavailable(_))
    ));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(matches!(
        SqliteStore::open(&path).await,
        Err(StorageError::Unavailable(_))
    ));
    let raw = db::Pool::open_read_only(&path).await.unwrap();
    let identities: i64 =
        db::query_scalar("SELECT count(*) FROM sqlite_master WHERE name='oc_memory_identities'")
            .fetch_one(&raw)
            .await
            .unwrap();
    assert_eq!(identities, 0);
    let schema: String = db::query_scalar("SELECT sql FROM sqlite_master WHERE name='oc_events'")
        .fetch_one(&raw)
        .await
        .unwrap();
    assert!(schema.contains("incompatible_content"));
    raw.close().await;
}

#[tokio::test]
async fn pre_merge_schema_is_rejected_without_rewriting_data_or_sidecars() {
    // Preserve the exact prior asset and binding DDL as an incompatible fixture.
    const OLD_ASSETS: &str = r#"CREATE TABLE oc_assets (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, id BLOB NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('memory','knowledge')),
    title TEXT NOT NULL, fact_key TEXT, current_version INTEGER,
    deleted INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL,
    PRIMARY KEY (tenant_id, workspace_id, id),
    FOREIGN KEY (tenant_id, workspace_id) REFERENCES oc_workspaces(tenant_id, id)
);
CREATE UNIQUE INDEX oc_memory_fact_slot ON oc_assets(tenant_id, workspace_id, fact_key)
WHERE kind = 'memory';

"#;
    const OLD_IDENTITIES: &str = r#"CREATE TABLE oc_memory_identities (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL,
    identity_key TEXT NOT NULL, asset_id BLOB NOT NULL,
    identity_json TEXT NOT NULL, created_at INTEGER NOT NULL,
    PRIMARY KEY (tenant_id, workspace_id, identity_key),
    UNIQUE (tenant_id, workspace_id, asset_id),
    FOREIGN KEY (tenant_id, workspace_id, asset_id)
        REFERENCES oc_assets(tenant_id, workspace_id, id)
);
"#;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oc.db");
    let store = SqliteStore::open(&path).await.unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "preserved workspace")
        .await
        .unwrap();
    let key = store.issue_key(scope, "admin").await.unwrap();
    let auth = store.authenticate(&key.token).await.unwrap();
    let mut tx = store.begin(auth).await.unwrap();
    let source = tx
        .create_event("structured", "preserved source", None)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    db::query("DROP TABLE oc_assets")
        .execute(store.pool())
        .await
        .unwrap();
    db::raw_sql(OLD_ASSETS).execute(store.pool()).await.unwrap();
    db::raw_sql(OLD_IDENTITIES)
        .execute(store.pool())
        .await
        .unwrap();
    let asset = Uuid::new_v4();
    let identity: origence::memory_identity::MemoryIdentity = serde_json::from_value(json!({
        "subject": {"kind": "service", "stable_id": "billing"},
        "predicate": "release.approval", "context": {}
    }))
    .unwrap();
    let identity_key = identity.key(scope).unwrap();
    db::query("INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,fact_key,created_at) VALUES(?,?,?,'memory','old memory',?,1)")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(asset).bind(&identity_key)
        .execute(store.pool()).await.unwrap();
    db::query("INSERT INTO oc_memory_identities(tenant_id,workspace_id,identity_key,asset_id,identity_json,created_at) VALUES(?,?,?,?,?,1)")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(&identity_key).bind(asset)
        .bind(serde_json::to_string(&identity).unwrap())
        .execute(store.pool()).await.unwrap();
    assert!(matches!(
        store.check().await,
        Err(StorageError::Unavailable(_))
    ));
    store.shutdown().await.unwrap();
    drop(store);
    let snapshot = || {
        let mut files = std::collections::BTreeMap::new();
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                files.insert(entry.file_name(), std::fs::read(entry.path()).unwrap());
            }
        }
        files
    };
    let before = snapshot();
    assert!(matches!(
        SqliteStore::check_memory_identity(&path).await,
        Err(StorageError::Unavailable(_))
    ));
    assert_eq!(snapshot(), before);
    assert!(matches!(
        SqliteStore::open(&path).await,
        Err(StorageError::Unavailable(_))
    ));
    assert_eq!(snapshot(), before);
    let raw = db::Pool::open_read_only(&path).await.unwrap();
    let content: String = db::query_scalar("SELECT content FROM oc_events WHERE id=?")
        .bind(source)
        .fetch_one(&raw)
        .await
        .unwrap();
    assert_eq!(content, "preserved source");
    let binding: Uuid =
        db::query_scalar("SELECT asset_id FROM oc_memory_identities WHERE identity_key=?")
            .bind(&identity_key)
            .fetch_one(&raw)
            .await
            .unwrap();
    assert_eq!(binding, asset);
    let role: String =
        db::query_scalar("SELECT role FROM oc_api_keys WHERE tenant_id=? AND workspace_id=?")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .fetch_one(&raw)
            .await
            .unwrap();
    assert_eq!(role, "admin");
    raw.close().await;
}

#[tokio::test]
async fn renamed_merged_identity_column_is_rejected_without_repair() {
    for column in ["identity_key", "identity_json"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("oc.db");
        let store = SqliteStore::open(&path).await.unwrap();
        db::query(&format!(
            "ALTER TABLE oc_assets RENAME COLUMN {column} TO incompatible_identity"
        ))
        .execute(store.pool())
        .await
        .unwrap();
        assert!(matches!(
            store.check().await,
            Err(StorageError::Unavailable(_))
        ));
        store.shutdown().await.unwrap();
        drop(store);
        let bytes = std::fs::read(&path).unwrap();
        assert!(matches!(
            SqliteStore::check_memory_identity(&path).await,
            Err(StorageError::Unavailable(_))
        ));
        assert!(matches!(
            SqliteStore::open(&path).await,
            Err(StorageError::Unavailable(_))
        ));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[tokio::test]
async fn weakened_identity_check_is_rejected_without_repair() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oc.db");
    let store = SqliteStore::open(&path).await.unwrap();
    // Same columns and indexes, but a weakened CHECK admits one-sided identities.
    db::raw_sql(
        "DROP TABLE oc_assets;
         CREATE TABLE oc_assets (
             tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, id BLOB NOT NULL,
             kind TEXT NOT NULL CHECK (kind IN ('memory','knowledge')),
             title TEXT NOT NULL, fact_key TEXT, current_version INTEGER,
             deleted INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL,
             identity_key TEXT, identity_json TEXT,
             CHECK (identity_key IS NULL OR kind = 'memory'),
             PRIMARY KEY (tenant_id, workspace_id, id),
             FOREIGN KEY (tenant_id, workspace_id) REFERENCES oc_workspaces(tenant_id, id)
         );
         CREATE UNIQUE INDEX oc_memory_fact_slot ON oc_assets(tenant_id, workspace_id, fact_key)
         WHERE kind = 'memory';
         CREATE UNIQUE INDEX oc_memory_identity_slot ON oc_assets(tenant_id, workspace_id, identity_key)
         WHERE identity_key IS NOT NULL;",
    )
    .execute(store.pool())
    .await
    .unwrap();
    assert!(matches!(
        store.check().await,
        Err(StorageError::Unavailable(_))
    ));
    store.shutdown().await.unwrap();
    drop(store);
    let bytes = std::fs::read(&path).unwrap();
    assert!(matches!(
        SqliteStore::check_memory_identity(&path).await,
        Err(StorageError::Unavailable(_))
    ));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(matches!(
        SqliteStore::open(&path).await,
        Err(StorageError::Unavailable(_))
    ));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}
