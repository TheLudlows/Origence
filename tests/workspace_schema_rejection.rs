//! Read-only rejection tests for incompatible SQLite authority schemas.
use origence::storage::sqlite_db as db;
use origence::storage::{Lifecycle, StorageError, sqlite::SqliteStore};

#[tokio::test]
async fn unmarked_schema_is_rejected_before_wal_or_domain_schema_writes() {
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
            "CREATE TABLE foreign_archive(payload TEXT NOT NULL);\
             INSERT INTO foreign_archive VALUES('preserve legacy data');",
        )
        .execute(&mut *conn)
        .await
        .unwrap();
        drop(conn);
        raw_pool.close().await;
        let before = tokio::fs::read(&path).await.unwrap();
        let result = SqliteStore::open(&path).await;
        assert!(
            matches!(result, Err(StorageError::Unavailable(ref message)) if message.contains("format"))
        );
        assert_eq!(tokio::fs::read(&path).await.unwrap(), before);
        let database_path = &path;
        let raw_pool = db::Pool::open_read_only(database_path).await.unwrap();
        let mut conn = raw_pool.acquire().await.unwrap();
        let mode: String = db::query_scalar("PRAGMA journal_mode")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        // Immutable admission of a checkpointed file uses no WAL/SHM and
        // reports DELETE internally; the file's persisted header is unchanged.
        assert_eq!(mode.to_ascii_lowercase(), "delete");
        let content: String = db::query_scalar("SELECT payload FROM foreign_archive")
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
async fn an_unexpected_table_is_rejected_without_repair() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mixed.db");
    let store = SqliteStore::open(&path).await.unwrap();
    db::raw_sql(
        "CREATE TABLE foreign_archive(payload TEXT NOT NULL);\
         INSERT INTO foreign_archive VALUES('preserve existing data');",
    )
    .execute(store.pool())
    .await
    .unwrap();
    assert!(
        matches!(store.check().await, Err(StorageError::Unavailable(ref message)) if message.contains("incompatible SQLite schema"))
    );
    store.shutdown().await.unwrap();
    drop(store);

    let before = tokio::fs::read(&path).await.unwrap();
    assert!(
        matches!(SqliteStore::check_memory_identity(&path).await, Err(StorageError::Unavailable(ref message)) if message.contains("incompatible SQLite schema"))
    );
    assert!(
        matches!(SqliteStore::open(&path).await, Err(StorageError::Unavailable(ref message)) if message.contains("incompatible SQLite schema"))
    );
    assert_eq!(tokio::fs::read(&path).await.unwrap(), before);
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
        // persistent database and WAL. SQLite's SHM read marks are volatile
        // coordination, updated even by readers; they are not stored payload.
        for name in ["old-ledger.db", "old-ledger.db-wal"] {
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
    assert!(!schema.contains("idempotency_key"));
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

#[tokio::test]
async fn database_without_run_reverse_index_is_rejected_without_repair() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("authority.db");
    let store = SqliteStore::open(&path).await.unwrap();
    db::query("DROP TABLE oc_run_asset_refs")
        .execute(store.pool())
        .await
        .unwrap();
    store.shutdown().await.unwrap();
    drop(store);
    let before = tokio::fs::read(&path).await.unwrap();
    assert!(SqliteStore::open(&path).await.is_err());
    assert_eq!(tokio::fs::read(&path).await.unwrap(), before);
}

#[tokio::test]
async fn missing_run_verification_table_is_rejected_without_migration() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("incompatible.db");
    let store = SqliteStore::open(&path).await.unwrap();
    db::raw_sql("DROP TABLE oc_run_verifications")
        .execute(store.pool())
        .await
        .unwrap();
    store.shutdown().await.unwrap();
    drop(store);
    let before = tokio::fs::read(&path).await.unwrap();
    assert!(SqliteStore::open(&path).await.is_err());
    assert_eq!(before, tokio::fs::read(&path).await.unwrap());
}

#[tokio::test]
async fn missing_run_statistics_schema_is_rejected_without_repair() {
    for remove in [
        "DROP INDEX oc_runs_owner_created_at",
        "DROP TABLE oc_run_measurements",
        "DROP INDEX oc_run_measurements_owner_created",
        "DROP INDEX oc_run_measurements_workspace_created",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("authority.db");
        let store = SqliteStore::open(&path).await.unwrap();
        db::query(remove).execute(store.pool()).await.unwrap();
        store.shutdown().await.unwrap();
        drop(store);
        let before = tokio::fs::read(&path).await.unwrap();
        assert!(SqliteStore::open(&path).await.is_err(), "{remove}");
        assert_eq!(tokio::fs::read(&path).await.unwrap(), before);
    }
}
