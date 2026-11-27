//! Offline verification of paired private archive backups.
use crate::{
    error::{AppError, Result},
    models::Models,
    service::Service,
    storage::{anyseek::AnySeekConfig, hash},
};
use serde_json::{Value, json};
use std::path::Path;
use uuid::Uuid;

#[cfg(feature = "embedded-anyseek")]
pub async fn run_embedded(dir: &Path, models: Models) -> Result<Value> {
    run(dir, models, AnySeekConfig::embedded()).await
}

pub async fn run(dir: &Path, models: Models, config: AnySeekConfig) -> Result<Value> {
    let authority = dir.join("context.db");
    if !tokio::fs::metadata(&authority)
        .await
        .is_ok_and(|m| m.is_file())
    {
        return Err(AppError::Invalid(
            "private archive maintenance requires an existing authority database".into(),
        ));
    }
    let service = Service::open(dir, models, config, false).await?;
    let result = verify(&service).await;
    let shutdown = service.engine.shutdown().await;
    let value = result?;
    shutdown?;
    Ok(value)
}

async fn verify(service: &Service) -> Result<Value> {
    let store = service.engine.relational();
    let backend = service.engine.vector().private_archive();
    let mut proofs = Vec::new();
    let mut native_archived = 0;
    for mut entry in store.archive_restore_sessions().await? {
        let domain = entry.domain;
        let session = entry.session;
        let collection = domain.collection();
        // Bind the allocator even if the live archive is empty.
        proofs.push((
            collection.clone(),
            entry.last_sequence,
            session,
            Uuid::nil(),
            Uuid::nil(),
            String::new(),
            String::new(),
            "session",
        ));
        if !backend.native_archive_collection(domain, false).await? {
            return Err(AppError::Unavailable(
                "private archive collection missing".into(),
            ));
        }
        let mut after = 0;
        loop {
            let (next, rows) = backend
                .native_archive_page(domain, session, after, 32)
                .await?;
            for row in rows {
                if entry.registered.remove(&row.turn) != Some(row.sequence) {
                    return Err(AppError::Unavailable(
                        "native archive sequence does not match its registration".into(),
                    ));
                }
                if let Some((source, digest)) = entry.expected.remove(&row.turn)
                    && (source != row.source || digest != row.request_hash)
                {
                    return Err(AppError::Unavailable(
                        "native archive does not match its recorded dependency proof".into(),
                    ));
                }
                native_archived += 1;
                proofs.push((
                    collection.clone(),
                    row.sequence,
                    session,
                    row.turn,
                    row.source,
                    row.request_hash.clone(),
                    hash(json!(row).to_string()),
                    "archived",
                ));
            }
            match next {
                Some(sequence) if sequence > after => after = sequence,
                Some(_) => {
                    return Err(AppError::Unavailable(
                        "native archive cursor did not advance".into(),
                    ));
                }
                None => break,
            }
        }
        for (turn, sequence) in entry.registered {
            let version = backend
                .absent_archive_version(domain, session, turn)
                .await?;
            proofs.push((
                collection.clone(),
                sequence,
                session,
                turn,
                Uuid::nil(),
                String::new(),
                hash(json!(version).to_string()),
                "absent",
            ));
        }
        if !entry.expected.is_empty() {
            return Err(AppError::Unavailable(
                "native archive referenced by a recorded dependency proof is missing".into(),
            ));
        }
    }
    proofs.sort_unstable();
    Ok(json!({
        "format":"origence-private-archive-restore-v2",
        "anyseek_version":backend.archive_backend_version().await?,
        "archived":native_archived,
        "directory_sha256":hash(json!(proofs).to_string()),
    }))
}
