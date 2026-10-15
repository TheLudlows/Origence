//! Fresh schema and read-only legacy rejection; no migration or data repair.
use origence::storage::native_turso as db;
use origence::storage::{Lifecycle, StorageError, sqlite::SqliteStore};

#[tokio::test]
async fn fresh_database_has_no_protocol_specific_tables() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("fresh.db"))
        .await
        .unwrap();
    let count: i64 =
        db::query_scalar("SELECT count(*) FROM sqlite_master WHERE name GLOB 'oc_aml_*'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(count, 0);
    store.check().await.unwrap();
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn legacy_schema_is_rejected_before_wal_or_domain_schema_writes() {
    for has_base in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.db");
        let raw_pool = db::Pool::open(&path).await.unwrap();
        let mut conn = raw_pool.acquire().await.unwrap();
        if has_base {
            db::raw_sql(include_str!("../src/storage/sqlite-schema.sql"))
                .execute(&mut *conn)
                .await
                .unwrap();
        }
        db::raw_sql(
            "CREATE TABLE oc_aml_adds(payload TEXT NOT NULL);\
             INSERT INTO oc_aml_adds VALUES('preserve legacy data');",
        )
        .execute(&mut *conn)
        .await
        .unwrap();
        drop(conn);
        raw_pool.close().await;
        let before = tokio::fs::read(&path).await.unwrap();
        let result = SqliteStore::open(&path).await;
        assert!(
            matches!(result, Err(StorageError::Unavailable(ref message)) if message.contains("legacy AML schema") || message.contains("format"))
        );
        assert_eq!(tokio::fs::read(&path).await.unwrap(), before);
        let database_path = &path;
        let raw_pool = db::Pool::open_read_only(database_path).await.unwrap();
        let mut conn = raw_pool.acquire().await.unwrap();
        let mode: String = db::query_scalar("PRAGMA journal_mode")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(mode.to_ascii_lowercase(), "wal");
        let content: String = db::query_scalar("SELECT payload FROM oc_aml_adds")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(content, "preserve legacy data");
        let base_count: i64 =
            db::query_scalar("SELECT count(*) FROM sqlite_master WHERE name='oc_workspaces'")
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        assert_eq!(base_count, if has_base { 1 } else { 0 });
        drop(conn);
        raw_pool.close().await;
    }
}

#[tokio::test]
async fn an_existing_protocol_table_cannot_hide_behind_the_generic_schema() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("mixed.db"))
        .await
        .unwrap();
    db::query("CREATE TABLE oc_aml_users(legacy_id TEXT)")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(
        matches!(store.check().await, Err(StorageError::Unavailable(ref message)) if message.contains("legacy AML schema") || message.contains("format"))
    );
    let count: i64 =
        db::query_scalar("SELECT count(*) FROM sqlite_master WHERE name='oc_aml_users'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(count, 1);
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn old_ledger_retry_metadata_schema_is_rejected_without_repair() {
    // Exact prior ledger layout: removed columns must not be silently discarded.
    const OLD_LEDGER: &str = r#"CREATE TABLE oc_artifact_ledger (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL,
    source_id BLOB NOT NULL, version INTEGER NOT NULL,
    artifact_type TEXT NOT NULL CHECK (artifact_type IN ('chunk','summary','entity','relation')),
    artifact_id BLOB NOT NULL,
    surface TEXT NOT NULL CHECK (surface IN ('vector','graph','full_text')),
    generation INTEGER NOT NULL DEFAULT 1,
    idempotency_key TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','committed','retry_wait','orphan')),
    attempt INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    next_retry_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (tenant_id, workspace_id, source_id, version, artifact_id, surface, generation)
);
CREATE INDEX oc_ledger_state ON oc_artifact_ledger(state, next_retry_at);
"#;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old-ledger.db");
    let snapshot = || {
        let mut files = std::collections::BTreeMap::new();
        // The host .lock file is intentionally held with an OS-exclusive lock;
        // Windows forbids reading it while the store is open. Snapshot only the
        // persistent database and native sidecars, never suppress their errors.
        for name in ["old-ledger.db", "old-ledger.db-wal", "old-ledger.db-shm"] {
            match std::fs::read(dir.path().join(name)) {
                Ok(bytes) => {
                    files.insert(name, bytes);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => panic!("snapshot {name}: {error}"),
            }
        }
        files
    };
    let store = SqliteStore::open(&path).await.unwrap();
    let schema: String =
        db::query_scalar("SELECT sql FROM sqlite_master WHERE name='oc_artifact_ledger'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert!(!schema.contains("attempt"));
    assert!(!schema.contains("last_error"));
    assert!(schema.contains("next_retry_at"));
    assert!(schema.contains("idempotency_key"));
    store.check().await.unwrap();
    db::query("DROP TABLE oc_artifact_ledger")
        .execute(store.pool())
        .await
        .unwrap();
    db::raw_sql(OLD_LEDGER).execute(store.pool()).await.unwrap();
    db::query("INSERT INTO oc_artifact_ledger(tenant_id,workspace_id,source_id,version,artifact_type,artifact_id,surface,generation,idempotency_key,state,attempt,last_error,next_retry_at,created_at,updated_at) VALUES(?,?,?,1,'chunk',?,'vector',2,'preserved-key','retry_wait',4,'preserved-error',123,1,2)")
        .bind(uuid::Uuid::new_v4())
        .bind(uuid::Uuid::new_v4())
        .bind(uuid::Uuid::new_v4())
        .bind(uuid::Uuid::new_v4())
        .execute(store.pool())
        .await
        .unwrap();
    let before_check = snapshot();
    assert!(matches!(
        store.check().await,
        Err(StorageError::Unavailable(_))
    ));
    assert_eq!(snapshot(), before_check);
    store.shutdown().await.unwrap();
    drop(store);
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
    let preserved: (String, i64, String, i64, String) = db::query_as(
        "SELECT state,attempt,last_error,next_retry_at,idempotency_key FROM oc_artifact_ledger",
    )
    .fetch_one(&raw)
    .await
    .unwrap();
    assert_eq!(
        preserved,
        (
            "retry_wait".into(),
            4,
            "preserved-error".into(),
            123,
            "preserved-key".into()
        )
    );
    raw.close().await;
    assert_eq!(snapshot(), before);
}
