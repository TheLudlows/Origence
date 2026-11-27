//! Offline archive copy, verification and explicit removal of legacy bodies.
use crate::{
    error::{AppError, Result},
    models::Models,
    service::Service,
    storage::{SqliteStore, anyseek::AnySeekConfig, hash},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Copy)]
pub enum Stage {
    Copy { limit: usize },
    Verify,
    Purge,
}

#[cfg(feature = "embedded-anyseek")]
pub async fn run_embedded(dir: &Path, models: Models, stage: Stage) -> Result<Value> {
    run(dir, models, AnySeekConfig::embedded(), stage).await
}

pub async fn run(dir: &Path, models: Models, config: AnySeekConfig, stage: Stage) -> Result<Value> {
    let limit = match stage {
        Stage::Copy { limit } if (1..=1024).contains(&limit) => {
            SqliteStore::prepare_archive_migration(&dir.join("context.db")).await?;
            limit
        }
        Stage::Copy { .. } => return Err(AppError::Invalid("copy limit must be 1..1024".into())),
        _ => 0,
    };
    if !tokio::fs::metadata(dir.join("context.db"))
        .await
        .is_ok_and(|m| m.is_file())
    {
        return Err(AppError::Invalid(
            "private archive maintenance requires an existing authority database".into(),
        ));
    }
    let service = Service::open(dir, models, config, true).await?;
    let result = reconcile(&service, stage, limit).await;
    let shutdown = service.engine.shutdown().await;
    let value = result?;
    shutdown?;
    Ok(value)
}

async fn reconcile(service: &Service, stage: Stage, limit: usize) -> Result<Value> {
    let store = service.engine.relational();
    let backend = service.engine.vector().private_archive();
    let phase = store.archive_migration_phase().await?;
    let can_copy = phase.as_deref() == Some("copying");
    let mut copied = 0;
    let mut unconfirmed = 0;
    let mut archived = 0;
    let mut withdrawn = 0;
    let mut staged = 0;
    let mut proofs = Vec::new();
    let mut watermarks = BTreeMap::<String, u64>::new();
    let mut confirmed = BTreeMap::<String, u64>::new();
    for (domain, session, turn, source, sequence) in store.archive_migration_directory().await? {
        let copy = can_copy && copied < limit;
        let item = store
            .migration_archive(domain, session, turn, source, sequence, can_copy, copy)
            .await?;
        let locator = &item.locator;
        let mut state = item.state.as_str();
        let mut receipt = item.confirmed_seq;
        let native = match state {
            "archived" => backend.archive_read(locator, false).await?,
            "pending" => {
                let mut native = backend.archive_read(locator, true).await?;
                if copy {
                    if native.is_none() {
                        let body = item.old_body.as_ref().ok_or_else(|| {
                            AppError::Unavailable("legacy archive body unavailable".into())
                        })?;
                        service.write_archive_locator(locator, body).await?;
                        native = backend.archive_read(locator, false).await?;
                    }
                    let (_, seq) = native.as_ref().ok_or_else(|| {
                        AppError::Unavailable("private archive write could not be verified".into())
                    })?;
                    store.confirm_migration_archive(&item, *seq).await?;
                    state = "archived";
                    receipt = Some(*seq);
                    copied += 1;
                }
                native
            }
            "withdrawn" | "legacy" => None,
            _ => {
                return Err(AppError::Unavailable(
                    "invalid private archive migration state".into(),
                ));
            }
        };
        if let Some((_, watermark)) = native {
            if state == "archived" {
                let receipt = receipt
                    .filter(|seq| *seq > 0 && *seq <= watermark)
                    .ok_or_else(|| {
                        AppError::Unavailable(
                            "private archive visible watermark is below authority confirmation"
                                .into(),
                        )
                    })?;
                confirmed
                    .entry(domain.collection())
                    .and_modify(|n| *n = (*n).max(receipt))
                    .or_insert(receipt);
            }
            watermarks
                .entry(domain.collection())
                .and_modify(|n| *n = (*n).max(watermark))
                .or_insert(watermark);
        }
        staged += usize::from(state != "legacy");
        match state {
            "archived" => archived += 1,
            "withdrawn" => withdrawn += 1,
            _ => unconfirmed += 1,
        }
        proofs.push((
            domain.collection(),
            item.sequence,
            session,
            turn,
            source,
            locator.group(),
            locator.body_hash.clone(),
            state.to_owned(),
        ));
    }
    if store.archive_directory_count().await? != staged {
        return Err(AppError::Unavailable(
            "private archive directory contains unmatched rows".into(),
        ));
    }
    if matches!(stage, Stage::Purge) {
        if phase.is_none() || unconfirmed != 0 {
            return Err(AppError::Conflict(
                "private archive cutover requires a fully verified legacy copy".into(),
            ));
        }
        store.purge_legacy_archives().await?;
    }
    proofs.sort_unstable();
    Ok(json!({
        "format":"origence-private-archive-v1",
        "anyseek_version":backend.archive_backend_version().await?,
        "phase":store.archive_migration_phase().await?,
        "archived":archived,"withdrawn":withdrawn,"unconfirmed":unconfirmed,"copied":copied,
        "directory_sha256":hash(json!(proofs).to_string()),
        "sqlite_confirmed_sequences":confirmed,
        "anyseek_visible_sequences":watermarks,
    }))
}
