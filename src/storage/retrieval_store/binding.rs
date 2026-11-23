//! Key-free backend binding under the authority's exclusive directory lock.
use crate::storage::{StorageError, StorageResult, anyseek::AnySeekConfig};
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const FILE: &str = "full-text-backend.json";
const MAX_BYTES: usize = 4096;

pub(crate) struct Binding {
    selected: serde_json::Value,
    pub(super) existing: bool,
    dir: PathBuf,
    analysis: tokio::sync::Mutex<Option<String>>,
}

fn mismatch() -> StorageError {
    StorageError::Conflict("retrieval backend differs from the data directory binding".into())
}
fn unavailable() -> StorageError {
    StorageError::Unavailable("retrieval backend binding unavailable".into())
}

/// Validate before opening the retrieval backend. A missing binding must never
/// reinterpret already-published local full-text as a remote projection.
pub(crate) async fn prepare(
    dir: &Path,
    selected: &AnySeekConfig,
    published_full_text: bool,
) -> StorageResult<Binding> {
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
            let mut saved: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
            let fingerprint = match saved
                .as_object_mut()
                .and_then(|v| v.remove("analysis_fingerprint"))
            {
                Some(serde_json::Value::Null) if !published_full_text => None,
                Some(serde_json::Value::String(s)) if crate::retrieval_diagnostics::digest(&s) => {
                    Some(s)
                }
                _ => return Err(mismatch()),
            };
            if expected != saved {
                return Err(mismatch());
            }
            Ok(Binding {
                selected: expected,
                dir: dir.to_owned(),
                existing: true,
                analysis: tokio::sync::Mutex::new(fingerprint),
            })
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            if published_full_text {
                return Err(mismatch());
            }
            let bytes = serde_json::to_vec(&expected).map_err(|_| unavailable())?;
            if bytes.len() > MAX_BYTES {
                return Err(unavailable());
            }
            Ok(Binding {
                selected: expected,
                dir: dir.to_owned(),
                existing: false,
                analysis: tokio::sync::Mutex::new(None),
            })
        }
        Err(_) => Err(unavailable()),
    }
}

impl Binding {
    /// Persist endpoint/routing before serving; analysis is pinned on first use,
    /// before any publication. Null is valid only without committed projections.
    pub(crate) async fn commit_initial(&self) -> StorageResult<()> {
        let analysis = self.analysis.lock().await;
        persist(&self.dir, &self.selected, analysis.as_deref()).await
    }

    pub(crate) async fn check_analysis(&self, analysis: &str) -> StorageResult<()> {
        if !crate::retrieval_diagnostics::digest(analysis) {
            return Err(unavailable());
        }
        let mut saved = self.analysis.lock().await;
        if let Some(expected) = saved.as_ref() {
            return if expected == analysis {
                Ok(())
            } else {
                Err(mismatch())
            };
        }
        persist(&self.dir, &self.selected, Some(analysis)).await?;
        *saved = Some(analysis.to_owned());
        Ok(())
    }
}

async fn persist(
    dir: &Path,
    selected: &serde_json::Value,
    analysis: Option<&str>,
) -> StorageResult<()> {
    let mut selected = selected.clone();
    selected["analysis_fingerprint"] = serde_json::json!(analysis);
    let bytes = serde_json::to_vec(&selected).map_err(|_| unavailable())?;
    if bytes.len() > MAX_BYTES {
        return Err(unavailable());
    }
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
