//! Native Turso open/initialize/check/shutdown; never migrate existing databases.
use super::locking::{acquire_lock, acquire_path_lock};
use super::schema::{check_conn, exec_schema, reject_legacy_aml_schema, schema_present};
use super::*;
use std::path::Path;

impl TursoStore {
    /// Open a fresh or same-version native Turso database. Existing unmarked
    /// databases are inspected read-only and rejected before any writable open.
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
        let (lock, opening) = acquire_lock(&path)?;
        let lock = std::sync::Arc::new(lock);
        let _serialized = opening.lock().await;
        if path.exists() && tokio::fs::metadata(&path).await?.len() != 0 {
            let pool = Pool::open_read_only(&path).await.map_err(turso_err)?;
            pool.hold_lifetime_guard(lock.clone());
            let outcome = async {
                let mut conn = pool.acquire().await.map_err(turso_err)?;
                native_turso::check_format(&mut conn, "authority")
                    .await
                    .map_err(turso_err)?;
                check_conn(&mut conn).await
            }
            .await;
            pool.close().await;
            outcome?;
        }
        let pool = Pool::open(&path).await.map_err(turso_err)?;
        pool.hold_lifetime_guard(lock.clone());
        let store = Self {
            pool,
            path,
            _lock: lock,
        };
        store.initialize().await?;
        Ok(store)
    }

    /// Historical API retained only for same-version inspection. Native Turso
    /// databases always contain identity support; incompatible files are rejected.
    pub async fn upgrade_memory_identity(
        path: impl AsRef<Path>,
        _dry_run: bool,
    ) -> StorageResult<MemoryIdentityUpgrade> {
        let path = tokio::fs::canonicalize(path.as_ref()).await?;
        let (lock, opening) = acquire_path_lock(&path, false)?;
        let lock = std::sync::Arc::new(lock);
        let _serialized = opening.lock().await;
        let pool = Pool::open_read_only(&path).await.map_err(turso_err)?;
        pool.hold_lifetime_guard(lock);
        let outcome = async {
            let mut conn = pool.acquire().await.map_err(turso_err)?;
            native_turso::check_format(&mut conn, "authority")
                .await
                .map_err(turso_err)?;
            check_conn(&mut conn).await?;
            Ok(MemoryIdentityUpgrade::AlreadyEnabled)
        }
        .await;
        pool.close().await;
        outcome
    }

    /// Native Turso leases for storage adapters and diagnostic tests. Domain
    /// transactions never expose the pool through the vendor-neutral trait.
    pub fn pool(&self) -> &Pool {
        &self.pool
    }
    #[cfg(feature = "local-storage")]
    pub(crate) fn native_lifetime_guard(&self) -> std::sync::Arc<dyn Send + Sync> {
        self._lock.clone()
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Lifecycle for TursoStore {
    async fn initialize(&self) -> StorageResult<()> {
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(turso_err)?;
        reject_legacy_aml_schema(&mut tx).await?;
        if !schema_present(&mut tx).await? {
            let existing: i64 = native_turso::query_scalar(
                "SELECT count(*) FROM sqlite_master WHERE name NOT GLOB 'sqlite_*'",
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(turso_err)?;
            if existing != 0 {
                return Err(StorageError::Unavailable(
                    "incompatible database; no migration was attempted".into(),
                ));
            }
            exec_schema(&mut tx).await?;
            native_turso::install_format(&mut tx, "authority")
                .await
                .map_err(turso_err)?;
        }
        native_turso::check_format(&mut tx, "authority")
            .await
            .map_err(turso_err)?;
        check_conn(&mut tx).await?;
        tx.commit().await.map_err(turso_err)
    }
    async fn check(&self) -> StorageResult<()> {
        let mut conn = self.pool.acquire().await.map_err(turso_err)?;
        native_turso::check_format(&mut conn, "authority")
            .await
            .map_err(turso_err)?;
        check_conn(&mut conn).await
    }
    async fn shutdown(&self) -> StorageResult<()> {
        self.pool.shutdown().await.map_err(turso_err)
    }
}
