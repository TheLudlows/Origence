//! Atomic message acceptance and read-only observation of the original job.
use super::{
    AcceptedMessageSpec, INGEST_OPERATION, IngestReceipt, MessageBatch, PublicationStatus,
};
use crate::{
    error::{AppError, Result},
    jobs::JobPayload,
    service::{Service, decode},
    storage::{DomainTx, Permission, hash},
    types::AuthContext,
};
use serde_json::json;
use std::time::Duration;

impl Service {
    /// The only command operation using workspace-wide deduplication. It never
    /// changes the original creator on replay, even after key rotation.
    pub async fn submit_message_batch(
        &self,
        auth: &AuthContext,
        key: &str,
        batch: MessageBatch,
        accepted_spec: AcceptedMessageSpec,
    ) -> Result<IngestReceipt> {
        if key.is_empty() || key.len() > 200 || !key.is_ascii() {
            return Err(AppError::Invalid(
                "Idempotency-Key must contain 1..200 ASCII characters".into(),
            ));
        }
        let (batch, source, accepted_spec, request_hash) =
            tokio::task::spawn_blocking(move || -> Result<_> {
                batch.validate()?;
                accepted_spec.validate_bounds()?;
                let source = serde_json::to_string(&batch).map_err(anyhow::Error::from)?;
                if source.len() > crate::parsing::MAX_FILE {
                    return Err(AppError::Invalid("message batch source too large".into()));
                }
                let request_hash =
                    hash(json!({"batch":batch,"accepted_spec":accepted_spec}).to_string());
                Ok((batch, source, accepted_spec, request_hash))
            })
            .await
            .map_err(|error| AppError::Internal(error.into()))??;
        let mut tx = self.write(auth, Permission::Write).await?;
        if let Some(cached) = tx
            .begin_command(INGEST_OPERATION, key, &request_hash)
            .await?
        {
            let receipt = decode(cached)?;
            tx.commit().await?;
            return Ok(receipt);
        }
        // Validate supported versions only after replay consistency, so a changed
        // parser/mode/profile under an existing key is always a conflict.
        accepted_spec.validate()?;
        if self.models.profile.as_deref() != Some(accepted_spec.embedding_profile.as_str()) {
            return Err(AppError::Unavailable(
                "message batch embedding profile is not configured".into(),
            ));
        }
        let asset_id = tx.insert_knowledge_asset("Message batch").await?;
        let source_event_id = tx.create_event(super::SOURCE_KIND, &source, None).await?;
        let job_id = Self::enqueue(
            &mut tx,
            JobPayload::MessageBatchIngest {
                expected_version: None,
                accepted_spec: accepted_spec.clone(),
            },
            Some(asset_id),
            Some(source_event_id),
        )
        .await?;
        let receipt = IngestReceipt {
            batch_id: batch.batch_id,
            session_id: batch.session_id,
            asset_id,
            source_event_id,
            job_id,
            target_version: 1,
            accepted_spec,
        };
        tx.audit("message_batch.accepted", asset_id, json!({"job_id":job_id}))
            .await?;
        tx.finish_command(json!(receipt)).await?;
        tx.commit().await?;
        Ok(receipt)
    }

    /// No current model defaults are consulted. Current access is reauthorized;
    /// original job ownership and the accepted processing contract remain intact.
    pub async fn message_batch_status(
        &self,
        auth: &AuthContext,
        receipt: &IngestReceipt,
    ) -> Result<PublicationStatus> {
        let mut tx = self.read(auth, Permission::Write).await?;
        let status = tx.message_batch_status(receipt).await?;
        tx.commit().await?;
        Ok(status)
    }

    /// Wait only for this durable receipt. Timeout/disconnect leaves its single
    /// Worker task untouched; callers choose their own protocol-specific deadline.
    pub async fn wait_message_batch(
        &self,
        auth: &AuthContext,
        receipt: &IngestReceipt,
        wait: Duration,
    ) -> Result<PublicationStatus> {
        if wait.is_zero() {
            return self.message_batch_status(auth, receipt).await;
        }
        match tokio::time::timeout(wait, async {
            loop {
                let status = self.message_batch_status(auth, receipt).await?;
                if status != PublicationStatus::Pending {
                    return Ok(status);
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        {
            Ok(status) => status,
            Err(_) => self.message_batch_status(auth, receipt).await,
        }
    }
}
