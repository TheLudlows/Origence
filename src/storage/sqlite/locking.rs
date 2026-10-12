//! Process lock ownership, reentrant open serialization and maintenance exclusion.
#[cfg(feature = "local-vector")]
use super::schema::check_conn;
use super::*;
use std::collections::HashMap;
use std::fs::File;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};
use fs2::FileExt;
#[cfg(feature = "local-vector")]
use sqlx::Connection;
#[cfg(feature = "local-vector")]
use sqlx::sqlite::SqliteConnectOptions;

/// Holds the existing host lock without opening/initializing an application store.
#[cfg(feature = "local-vector")]
pub(in crate::storage) struct MaintenanceLock {
    _guard: LockGuard,
}

#[cfg(feature = "local-vector")]
pub(in crate::storage) async fn lock_for_maintenance(path: &Path) -> StorageResult<MaintenanceLock> {
    let path = tokio::fs::canonicalize(path).await?;
    if !path.is_file() {
        return Err(StorageError::NotFound);
    }
    let (guard, opening) = acquire_path_lock(&path, false)?;
    let _serialized = opening.lock().await;
    let options = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(false)
        .read_only(true);
    let mut conn = SqliteConnection::connect_with(&options)
        .await
        .map_err(sqlite_err)?;
    check_conn(&mut conn).await?;
    conn.close().await.map_err(sqlite_err)?;
    Ok(MaintenanceLock { _guard: guard })
}

/// The lock file path bound to a database path (A2.5).
fn lock_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".lock");
    PathBuf::from(s)
}

/// One in-process holder of the OS lock and its open-store count. The OS lock
/// lives on the single `file` handle; in-process reopens bump `refs` without
/// re-taking the OS lock, so the API and Worker (one process) share one holder
/// while a second process is still rejected (A2.5). `opening` serializes
/// same-process opens end to end so a reopen cannot race the first open's
/// connect and initialize (SQLITE_BUSY under load).
struct HeldLock {
    /// Owned solely to hold the OS lock until the last in-process store drops.
    #[allow(dead_code)]
    file: File,
    refs: usize,
    reentrant: bool,
    /// Held across connect + initialize for every in-process open of this path.
    opening: Arc<tokio::sync::Mutex<()>>,
}

/// Locks held by this process, keyed by lock-file path.
static PROCESS_LOCKS: LazyLock<Mutex<HashMap<PathBuf, HeldLock>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Take the single-writer lock for `path`, returning a guard that releases it
/// on drop plus the per-path mutex that serializes in-process opens. Reentrant
/// within this process (A2.5).
pub(super) fn acquire_lock(path: &Path) -> StorageResult<(LockGuard, Arc<tokio::sync::Mutex<()>>)> {
    acquire_path_lock(path, true)
}

pub(super) fn acquire_path_lock(
    path: &Path,
    reentrant: bool,
) -> StorageResult<(LockGuard, Arc<tokio::sync::Mutex<()>>)> {
    let key = lock_path(path);
    let mut locks = PROCESS_LOCKS
        .lock()
        .expect("process lock registry poisoned");
    if let Some(held) = locks.get_mut(&key) {
        if !reentrant || !held.reentrant {
            return Err(StorageError::Conflict(
                "store is already open in this process".into(),
            ));
        }
        held.refs += 1;
        return Ok((LockGuard(key), held.opening.clone()));
    }
    let file = File::create(&key)?;
    file.try_lock_exclusive().map_err(|e| {
        StorageError::Conflict(format!("store lock is held by another process: {e}"))
    })?;
    let opening = Arc::new(tokio::sync::Mutex::new(()));
    locks.insert(
        key.clone(),
        HeldLock {
            file,
            refs: 1,
            reentrant,
            opening: opening.clone(),
        },
    );
    Ok((LockGuard(key), opening))
}

/// Release one in-process reference; the last one drops the OS-locked file.
fn release_lock(key: &Path) {
    let mut locks = PROCESS_LOCKS
        .lock()
        .expect("process lock registry poisoned");
    if let Some(held) = locks.get_mut(key) {
        held.refs -= 1;
        if held.refs == 0 {
            locks.remove(key);
        }
    }
}

/// Guard that releases its in-process lock reference on drop.
pub(super) struct LockGuard(PathBuf);

impl Drop for LockGuard {
    fn drop(&mut self) {
        release_lock(&self.0);
    }
}

#[cfg(all(test, feature = "local-vector"))]
mod maintenance_lock_tests {
    
    #[tokio::test]
    async fn maintenance_guard_blocks_new_in_process_opens_and_releases_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("context.db");
        let store = SqliteStore::open(&path).await.unwrap();
        store.shutdown().await.unwrap();
        drop(store);
        let guard = lock_for_maintenance(&path).await.unwrap();
        assert!(matches!(
            SqliteStore::open(&path).await,
            Err(StorageError::Conflict(_))
        ));
        assert!(matches!(
            lock_for_maintenance(&path).await,
            Err(StorageError::Conflict(_))
        ));
        drop(guard);
        assert!(SqliteStore::open(&path).await.is_ok());
    }
}
