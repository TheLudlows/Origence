//! Explicit, bounded Improve requests use the existing durable worker.
use crate::{
    error::{AppError, Result},
    jobs::JobPayload,
    learning::ImproveInput,
    service::Service,
    storage::{DomainTx, Permission, WorkItem, hash},
    types::AuthContext,
};
use serde_json::{Value, json};
use uuid::Uuid;

impl Service {
    pub async fn improve(
        &self,
        a: &AuthContext,
        key: &str,
        session: Uuid,
        input: ImproveInput,
    ) -> Result<Value> {
        if session.is_nil() {
            return Err(AppError::Invalid("session_id must be non-nil".into()));
        }
        input.validate().map_err(|e| AppError::Invalid(e.into()))?;
        let request = json!({"session_id":session,"input":input});
        let (mut tx, cached) = self
            .command(a, Permission::Write, "session.improve", key, &request)
            .await?;
        if cached.is_some() {
            let value = tx.improve_view(input.improve_id).await?;
            tx.commit().await?;
            return Ok(value);
        }
        let job_id = Uuid::new_v4();
        let run = tx
            .accept_improve(session, &input, &hash(request.to_string()), job_id)
            .await?;
        if run.job_id == job_id {
            let extraction_profile = self.models.improve_extraction_profile().ok_or_else(|| {
                AppError::Unavailable("extraction model is not configured".into())
            })?;
            let extraction_model = self.models.summary_model().ok_or_else(|| {
                AppError::Unavailable("extraction model is not configured".into())
            })?;
            let (kind, payload) = JobPayload::Improve {
                session_id: session,
                improve_id: input.improve_id,
                extraction_profile,
                extraction_model: extraction_model.to_owned(),
                embedding_profile: self.models.profile.clone(),
            }
            .encode()?;
            tx.enqueue(WorkItem {
                job_id,
                kind: kind.as_str().into(),
                payload,
                asset: None,
                source: None,
            })
            .await?;
            tx.audit("session.improve_requested", input.improve_id, json!({"session_id":session,"job_id":job_id}))
                .await?;
        }
        let receipt = tx.improve_view(input.improve_id).await?;
        Self::finish(tx, receipt).await
    }

    pub async fn improvement(&self, a: &AuthContext, id: Uuid) -> Result<Value> {
        let mut tx = self.read(a, Permission::Write).await?;
        let value = tx.improve_view(id).await?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn learning(&self, a: &AuthContext, id: Uuid) -> Result<Value> {
        let mut tx = self.read(a, Permission::Read).await?;
        let value = tx.learning_view(id).await?;
        tx.commit().await?;
        Ok(value)
    }
}
