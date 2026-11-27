//! Authorized Run operations stay in one relational transaction.
use crate::{
    error::{AppError, Result},
    evidence::{EvidenceBundle, EvidenceProvenance},
    run::{RunEventInput, RunInput},
    service::Service,
    session::ArchivedTurnBody,
    storage::{DomainTx, Permission, anyseek::private_archive::NativeArchiveTurn},
    types::AuthContext,
};
use serde_json::Value;
use uuid::Uuid;
impl Service {
    pub async fn create_run(&self, a: &AuthContext, input: RunInput) -> Result<Value> {
        input.validate().map_err(|e| AppError::Invalid(e.into()))?;
        let mut tx = self.write(a, Permission::Write).await?;
        let value = tx.create_private_run(&input).await?;
        tx.commit().await?;
        Ok(value)
    }
    pub async fn append_run_event(
        &self,
        a: &AuthContext,
        run: Uuid,
        input: RunEventInput,
    ) -> Result<Value> {
        input.validate().map_err(|e| AppError::Invalid(e.into()))?;
        let mut tx = self.write(a, Permission::Write).await?;
        let value = tx.append_private_run_event(run, &input).await?;
        tx.commit().await?;
        Ok(value)
    }
    pub async fn run_trace(
        &self,
        a: &AuthContext,
        run: Uuid,
        after: i64,
        limit: usize,
    ) -> Result<Value> {
        let mut tx = self.read(a, Permission::Read).await?;
        let value = tx.private_run_trace(run, after, limit).await?;
        tx.commit().await?;
        Ok(value)
    }
}

impl Service {
    pub(crate) async fn record_run_context_failed(
        &self,
        a: &AuthContext,
        run: Uuid,
        event_id: Uuid,
        request_digest: &str,
        code: &'static str,
        observations: &crate::retrieval_diagnostics::RetrievalDiagnostics,
    ) -> Result<Option<Value>> {
        let mut tx = self.write(a, Permission::Write).await?;
        let replay = tx
            .record_run_context_failed(run, event_id, request_digest, code, observations)
            .await?;
        tx.commit().await?;
        match replay {
            Some(value) => Ok(Some(
                self.restore_archive_run_bundle(a, run, event_id, request_digest, value)
                    .await?,
            )),
            None => Ok(None),
        }
    }

    pub(crate) async fn replay_run_context_prepared(
        &self,
        a: &AuthContext,
        run: Uuid,
        event_id: Uuid,
        request_digest: &str,
    ) -> Result<Option<Value>> {
        let mut tx = self.read(a, Permission::Write).await?;
        let replay = tx
            .replay_run_context_prepared(run, event_id, request_digest)
            .await?;
        tx.commit().await?;
        match replay {
            Some(value) => Ok(Some(
                self.restore_archive_run_bundle(a, run, event_id, request_digest, value)
                    .await?,
            )),
            None => Ok(None),
        }
    }

    pub(crate) async fn check_run_owner(&self, a: &AuthContext, run: Uuid) -> Result<()> {
        let mut tx = self.read(a, Permission::Read).await?;
        tx.owned_run(run).await?;
        tx.commit().await?;
        Ok(())
    }
    pub(crate) async fn record_run_context_prepared(
        &self,
        a: &AuthContext,
        run: Uuid,
        event_id: Uuid,
        request_digest: &str,
        bundle: &EvidenceBundle,
    ) -> Result<Value> {
        let mut archives = Vec::new();
        for item in &bundle.items {
            if let Some(archive) = self.native_run_archive(a, &item.provenance).await? {
                if item.content != run_archive_content(&archive.body) {
                    return Err(AppError::Unavailable(
                        "private archive changed during Run preparation".into(),
                    ));
                }
                archives.push(archive);
            }
        }
        let mut tx = self.write(a, Permission::Write).await?;
        for archive in archives {
            tx.attach_native_archive_proof(&archive).await?;
        }
        let result = tx
            .record_run_context_prepared(run, event_id, request_digest, bundle)
            .await?;
        tx.commit().await?;
        self.restore_archive_run_bundle(a, run, event_id, request_digest, result)
            .await
    }

    async fn native_run_archive(
        &self,
        a: &AuthContext,
        citation: &EvidenceProvenance,
    ) -> Result<Option<NativeArchiveTurn>> {
        let EvidenceProvenance::SessionTurn {
            session_id,
            turn_id,
            source_event_id,
            used_evidence,
            evidence_use,
            created_at,
        } = citation
        else {
            return Ok(None);
        };
        let archive = self.load_native_archive(a, *session_id, *turn_id).await?;
        if let Some(archive) = &archive
            && (archive.source != *source_event_id
                || archive.evidence != *used_evidence
                || archive.body.evidence_use != *evidence_use
                || archive.created_at != *created_at)
        {
            return Err(AppError::Unavailable(
                "private archive does not match the recorded Run citation".into(),
            ));
        }
        Ok(archive)
    }

    async fn restore_archive_run_bundle(
        &self,
        a: &AuthContext,
        run: Uuid,
        event_id: Uuid,
        request_digest: &str,
        mut preparation: Value,
    ) -> Result<Value> {
        let Some(proof) = preparation.get("archive_bundle_digest") else {
            return Ok(preparation);
        };
        let invalid = || AppError::Unavailable("invalid prepared archive snapshot".into());
        let mut bundle: EvidenceBundle =
            serde_json::from_value(preparation["recorded_bundle"].clone())
                .map_err(|_| invalid())?;
        let mut legacy = Vec::new();
        for (index, item) in bundle.items.iter_mut().enumerate() {
            if let EvidenceProvenance::SessionTurn {
                session_id,
                turn_id,
                source_event_id,
                ..
            } = item.provenance
            {
                if let Some(archive) = self.native_run_archive(a, &item.provenance).await? {
                    item.content = run_archive_content(&archive.body);
                } else {
                    legacy.push((index, session_id, turn_id, source_event_id));
                }
            }
        }
        let mut legacy_bodies = Vec::new();
        if !legacy.is_empty() {
            let mut tx = self.read(a, Permission::Write).await?;
            let mut entries = Vec::new();
            for (index, session, turn, source) in legacy {
                if tx.require_turn_visible(session, turn).await? != source {
                    return Err(AppError::NotFound);
                }
                let entry = tx.archive_entry(session, turn).await?;
                entries.push((index, entry));
            }
            tx.commit().await?;
            for (index, entry) in entries {
                let body = self.archived_body(&entry).await?;
                legacy_bodies.push((index, entry, body));
            }
        }
        let mut tx = self.read(a, Permission::Write).await?;
        // Any cited workspace asset can be withdrawn while the private bodies
        // are in flight. Recheck the complete prepared event in this snapshot.
        let current = tx
            .replay_run_context_prepared(run, event_id, request_digest)
            .await?;
        if current.as_ref() != Some(&preparation) {
            return Err(invalid());
        }
        for (index, entry, body) in legacy_bodies {
            let fresh = tx.archive_delivery(&entry, body).await?;
            bundle.items[index].content =
                format!("Question: {}\nAnswer: {}", fresh.question, fresh.answer);
        }
        bundle.rendered_context = crate::evidence::restore_rendered_context(&bundle);
        if *proof != crate::storage::hash(serde_json::to_vec(&bundle).map_err(|_| invalid())?) {
            return Err(invalid());
        }
        preparation
            .as_object_mut()
            .ok_or_else(invalid)?
            .remove("archive_bundle_digest");
        preparation["recorded_bundle"] = serde_json::json!(bundle);
        tx.commit().await?;
        Ok(preparation)
    }
}

fn run_archive_content(body: &ArchivedTurnBody) -> String {
    format!("Question: {}\nAnswer: {}", body.question, body.answer)
}

impl Service {
    /// Owner-only exact prefix address; never accepts a caller-selected owner.
    pub async fn run_prefix(
        &self,
        a: &AuthContext,
        run: Uuid,
        through_sequence: i64,
    ) -> Result<Value> {
        let mut tx = self.read(a, Permission::Read).await?;
        let digest = tx.private_run_prefix_digest(run, through_sequence).await?;
        tx.commit().await?;
        Ok(serde_json::json!({"run_id":run,"through_sequence":through_sequence,"digest":digest}))
    }
}

impl Service {
    pub async fn record_run_verification(
        &self,
        a: &AuthContext,
        run: Uuid,
        input: crate::run_verification::RunVerificationInput,
    ) -> Result<Value> {
        input.validate().map_err(|e| AppError::Invalid(e.into()))?;
        let mut tx = self.write(a, Permission::Write).await?;
        let result = tx.record_run_verification(run, &input).await?;
        tx.commit().await?;
        Ok(result)
    }
    pub async fn run_verifications(
        &self,
        a: &AuthContext,
        run: Uuid,
        after: Uuid,
        limit: usize,
    ) -> Result<Value> {
        let mut tx = self.read(a, Permission::Read).await?;
        let result = tx.run_verifications(run, after, limit).await?;
        tx.commit().await?;
        Ok(result)
    }
    pub async fn withdraw_run_verification(
        &self,
        a: &AuthContext,
        run: Uuid,
        id: Uuid,
        input: crate::run_verification::WithdrawRunVerificationInput,
    ) -> Result<Value> {
        input.validate().map_err(|e| AppError::Invalid(e.into()))?;
        let mut tx = self.write(a, Permission::Write).await?;
        let result = tx.withdraw_run_verification(run, id, &input).await?;
        tx.commit().await?;
        Ok(result)
    }
}

impl Service {
    /// Prepare context for a private Run and return its immutable recorded result.
    pub async fn prepare_run_context(
        &self,
        caller: &AuthContext,
        run: Uuid,
        event_id: Uuid,
        context: crate::evidence::EvidenceInput,
    ) -> Result<Value> {
        if event_id.is_nil() {
            return Err(AppError::Invalid("event_id must not be nil".into()));
        }
        caller.require("write")?;
        self.check_run_owner(caller, run).await?;
        if context.as_of_ms.is_some() {
            return Err(AppError::Invalid(
                "as_of_ms is unsupported for Run context preparation. Use /v1/context or context_bundle for dated memory.".into(),
            ));
        }
        context
            .validate()
            .map_err(|e| AppError::Invalid(e.into()))?;
        let request_digest = crate::storage::hash(
            serde_json::to_vec(&context)
                .map_err(|_| AppError::Invalid("invalid context request".into()))?,
        );
        if let Some(prepared) = self
            .replay_run_context_prepared(caller, run, event_id, &request_digest)
            .await?
        {
            return prepared_context_response(prepared);
        }
        let (result, observations) = self.observed_evidence_bundle(caller, context).await;
        let bundle = match result {
            Ok(bundle) => bundle,
            Err(error) => {
                if let Some(prepared) = self
                    .record_run_context_failed(
                        caller,
                        run,
                        event_id,
                        &request_digest,
                        error.code(),
                        &observations,
                    )
                    .await?
                {
                    return prepared_context_response(prepared);
                }
                return Err(error);
            }
        };
        let preparation = self
            .record_run_context_prepared(caller, run, event_id, &request_digest, &bundle)
            .await?;
        prepared_context_response(preparation)
    }
}

fn prepared_context_response(mut preparation: Value) -> Result<Value> {
    let bundle = preparation
        .as_object_mut()
        .and_then(|v| v.remove("recorded_bundle"))
        .ok_or_else(|| AppError::Unavailable("invalid prepared context snapshot".into()))?;
    Ok(serde_json::json!({"bundle":bundle,"preparation":preparation}))
}
