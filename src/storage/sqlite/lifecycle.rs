//! Store open/initialize/check/shutdown and the existing explicit identity tool.
use super::locking::{acquire_lock, acquire_path_lock};
use super::schema::{
    IDENTITY_SCHEMA, check_conn, check_identity_schema, exec_schema, schema_present,
};
use super::*;
use sqlx::Connection;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::path::Path;
use std::time::Duration;

impl SqliteStore {
    /// Open the store at `path`, creating the schema when the file is new.
    pub async fn open(path: impl AsRef<Path>) -> StorageResult<Self> {
        let requested = path.as_ref();
        let parent = requested
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        tokio::fs::create_dir_all(parent).await?;
        let path = if requested.exists() {
            tokio::fs::canonicalize(requested).await?
        } else {
            tokio::fs::canonicalize(parent).await?.join(
                requested
                    .file_name()
                    .ok_or_else(|| StorageError::Conflict("database filename required".into()))?,
            )
        };
        // Take the single-writer lock before touching the database: a second
        // process that opens the same path fails fast here instead of racing
        // the running worker (A2.5). Reentrant within this process.
        let (lock, opening) = acquire_lock(&path)?;
        // Same-process opens serialize end to end: the OS lock only rejects
        // other processes, so a reopen would otherwise race the first open's
        // WAL transition and schema transaction (SQLITE_BUSY under load).
        let _serialized = opening.lock().await;
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5))
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .map_err(sqlite_err)?;
        let store = Self {
            pool,
            path,
            _lock: lock,
        };
        store.initialize().await?;
        Ok(store)
    }

    /// Enable identity writes in a compatible existing database. Requires an
    /// exclusive lock, validates before changing anything, and never creates a
    /// missing database or reinterprets legacy assets. Dry-run only reports.
    pub async fn upgrade_memory_identity(
        path: impl AsRef<Path>,
        dry_run: bool,
    ) -> StorageResult<MemoryIdentityUpgrade> {
        let path = tokio::fs::canonicalize(path.as_ref()).await?;
        if !path.is_file() {
            return Err(StorageError::Conflict(
                "existing database file required".into(),
            ));
        }
        let (_lock, opening) = acquire_path_lock(&path, false)?;
        let _serialized = opening.lock().await;
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(false)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let mut conn = SqliteConnection::connect_with(&options)
            .await
            .map_err(sqlite_err)?;
        let mut tx = conn
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(sqlite_err)?;
        check_conn(&mut tx).await?;
        let status = if identity_table_present(&mut tx).await? {
            MemoryIdentityUpgrade::AlreadyEnabled
        } else {
            let objects: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM sqlite_master WHERE name='oc_memory_identities'",
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(sqlite_err)?;
            if objects != 0 {
                return Err(StorageError::Unavailable(
                    "memory identity schema name is occupied".into(),
                ));
            }
            if dry_run {
                MemoryIdentityUpgrade::Required
            } else {
                sqlx::raw_sql(IDENTITY_SCHEMA)
                    .execute(&mut *tx)
                    .await
                    .map_err(sqlite_err)?;
                check_identity_schema(&mut tx).await?;
                MemoryIdentityUpgrade::Enabled
            }
        };
        tx.commit().await.map_err(sqlite_err)?;
        conn.close().await.map_err(sqlite_err)?;
        Ok(status)
    }

    /// The shared connection pool. Domain operations borrow short-lived
    /// connections; model/file IO never holds one (A2.5).
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Lifecycle for SqliteStore {
    async fn initialize(&self) -> StorageResult<()> {
        let mut conn = self.pool.acquire().await.map_err(sqlite_err)?;
        // BEGIN IMMEDIATE serializes concurrent initializers: a second one
        // waits on busy_timeout, then observes the now-present schema instead
        // of racing the first (A2.3).
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *conn)
            .await
            .map_err(sqlite_err)?;
        let outcome = Self::init_inner(&mut conn).await;
        match outcome {
            Ok(()) => {
                sqlx::query("COMMIT")
                    .execute(&mut *conn)
                    .await
                    .map_err(sqlite_err)?;
                Ok(())
            }
            Err(e) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
                Err(e)
            }
        }
    }

    async fn check(&self) -> StorageResult<()> {
        let mut conn = self.pool.acquire().await.map_err(sqlite_err)?;
        check_conn(&mut conn).await
    }

    async fn shutdown(&self) -> StorageResult<()> {
        self.pool.close().await;
        Ok(())
    }
}

impl SqliteStore {
    async fn init_inner(conn: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>) -> StorageResult<()> {
        if !schema_present(&mut *conn).await? {
            exec_schema(&mut *conn).await?;
        }
        check_conn(&mut *conn).await
    }
}
