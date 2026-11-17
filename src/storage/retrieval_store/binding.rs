//! Key-free backend binding under the authority's exclusive directory lock.
use crate::storage::{StorageError, StorageResult, anyseek::AnySeekConfig};
use std::{io::ErrorKind, path::Path};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const FILE: &str = "full-text-backend.json";
const MAX_BYTES: usize = 4096;

fn mismatch() -> StorageError {
    StorageError::Conflict("retrieval backend differs from the data directory binding".into())
}
fn unavailable() -> StorageError {
    StorageError::Unavailable("retrieval backend binding unavailable".into())
}

/// Validate before opening the remote backend. A missing binding must never
/// reinterpret already-published local full-text as a remote projection.
pub(crate) async fn prepare(
    dir: &Path,
    selected: &AnySeekConfig,
    published_full_text: bool,
) -> StorageResult<Option<Vec<u8>>> {
    let expected = selected.binding_identity();
    match tokio::fs::File::open(dir.join(FILE)).await {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take((MAX_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| unavailable())?;
            if bytes.len() > MAX_BYTES {
                return Err(unavailable());
            }
            let saved: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
            if expected != saved {
                return Err(mismatch());
            }
            Ok(None)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            if published_full_text {
                return Err(mismatch());
            }
            let bytes = serde_json::to_vec(&expected).map_err(|_| unavailable())?;
            if bytes.len() > MAX_BYTES {
                return Err(unavailable());
            }
            Ok(Some(bytes))
        }
        Err(_) => Err(unavailable()),
    }
}

/// Persist only after backend health checks, before jobs or HTTP are admitted.
/// The staging file is private to this attempt; interrupted staging files are
/// not mistaken for a valid binding or permission to overwrite user data.
pub(crate) async fn commit(dir: &Path, pending: Option<Vec<u8>>) -> StorageResult<()> {
    let Some(bytes) = pending else {
        return Ok(());
    };
    let stage = dir.join(format!(".full-text-backend-{}.tmp", uuid::Uuid::new_v4()));
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stage)
        .await
        .map_err(|_| unavailable())?;
    file.write_all(&bytes).await.map_err(|_| unavailable())?;
    file.sync_all().await.map_err(|_| unavailable())?;
    drop(file);
    tokio::fs::rename(&stage, dir.join(FILE))
        .await
        .map_err(|_| unavailable())?;
    #[cfg(unix)]
    {
        let dir = dir.to_owned();
        tokio::task::spawn_blocking(move || std::fs::File::open(dir)?.sync_all())
            .await
            .map_err(|_| unavailable())?
            .map_err(|_| unavailable())?;
    }
    Ok(())
}
