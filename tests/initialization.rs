use opencontext::storage::{Lifecycle, sqlite::SqliteStore};

#[tokio::test]
async fn sqlite_initializes_once_and_rejects_incompatible() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("oc.db");

    // A fresh file is created with the complete schema.
    let store = SqliteStore::open(&path).await?;
    store.check().await?;

    // Reopening the same file is a no-op: initialize and check stay idempotent.
    let again = SqliteStore::open(&path).await?;
    again.initialize().await?;
    again.check().await?;

    // A missing table is reported as incompatible, never silently repaired.
    sqlx::query("DROP TABLE oc_audit")
        .execute(store.pool())
        .await?;
    assert!(store.check().await.is_err());

    Ok(())
}

#[tokio::test]
async fn sqlite_concurrent_initialization_is_serialized() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("oc.db");
    let (left, right) = tokio::join!(SqliteStore::open(&path), SqliteStore::open(&path));
    left?.check().await?;
    right?.check().await?;
    Ok(())
}

#[tokio::test]
async fn missing_domain_column_is_rejected_without_repair() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("columns.db");
    let store = SqliteStore::open(&path).await?;
    sqlx::query("ALTER TABLE oc_audit RENAME COLUMN details TO incompatible_details")
        .execute(store.pool())
        .await?;
    assert!(store.check().await.is_err());
    store.shutdown().await?;
    drop(store);
    assert!(SqliteStore::open(&path).await.is_err());
    Ok(())
}
