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
