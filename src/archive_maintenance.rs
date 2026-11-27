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
        // Bind logical fences and the allocator without depending on when the
        // worker physically deletes withdrawn records.
        proofs.push((
            collection.clone(),
            entry.last_sequence,
            session,
            Uuid::nil(),
            Uuid::nil(),
            String::new(),
            hash(json!(entry.withdrawn_turns).to_string()),
            if entry.withdrawn {
                "withdrawn_session"
            } else {
                "session"
            },
        ));
        if entry.withdrawn {
            continue;
        }
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
                if row.sequence > entry.last_sequence {
                    return Err(AppError::Unavailable(
                        "native archive sequence is ahead of its authority allocator".into(),
                    ));
                }
                if entry.withdrawn_turns.contains(&row.turn) {
                    continue;
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
        if !entry.expected.is_empty() {
            return Err(AppError::Unavailable(
                "native archive referenced by a recorded dependency proof is missing".into(),
            ));
        }
    }
    proofs.sort_unstable();
    Ok(json!({
        "format":"origence-private-archive-restore-v1",
        "anyseek_version":backend.archive_backend_version().await?,
        "archived":native_archived,
        "directory_sha256":hash(json!(proofs).to_string()),
    }))
}
