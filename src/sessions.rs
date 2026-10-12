//! Session archival and feedback are synchronous relational commands, not jobs.
use crate::{
    error::{AppError, Result},
    service::Service,
    session::{FeedbackInput, SessionTurnInput},
    storage::{DomainTx, Permission, hash},
    types::AuthContext,
};
use serde_json::{Value, json};
use uuid::Uuid;

fn session_id(id: Uuid) -> Result<()> {
    if id.is_nil() {
        return Err(AppError::Invalid("session_id must be non-nil".into()));
    }
    Ok(())
}

impl Service {
    pub async fn archive_turn(
        &self,
        a: &AuthContext,
        key: &str,
        session: Uuid,
        input: SessionTurnInput,
    ) -> Result<Value> {
        session_id(session)?;
        input.validate().map_err(|e| AppError::Invalid(e.into()))?;
        let request = json!({"session_id":session,"input":input});
        let (mut tx, cached) = self.command(a, Permission::Write, "session.turn.archive", key, &request).await?;
        if let Some(receipt) = cached {
            // Command caches contain IDs only. Never replay success for a withdrawn archive.
            tx.require_turn_visible(session, input.turn_id).await?;
            tx.commit().await?;
            return Ok(receipt);
        }
        let receipt = tx.archive_turn(session, &input, &hash(request.to_string())).await?;
        Self::finish(tx, receipt).await
    }

    pub async fn session_turn(&self, a: &AuthContext, session: Uuid, turn: Uuid) -> Result<Value> {
        session_id(session)?;
        let mut tx = self.read(a, Permission::Write).await?;
        let value = tx.session_turn(session, turn).await?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn record_feedback(
        &self,
        a: &AuthContext,
        key: &str,
        session: Uuid,
        turn: Uuid,
        input: FeedbackInput,
    ) -> Result<Value> {
        session_id(session)?;
        if turn.is_nil() { return Err(AppError::Invalid("turn_id must be non-nil".into())); }
        input.validate().map_err(|e| AppError::Invalid(e.into()))?;
        let request = json!({"session_id":session,"turn_id":turn,"input":input});
        let (mut tx, cached) = self.command(a, Permission::Write, "session.feedback.record", key, &request).await?;
        if let Some(receipt) = cached {
            tx.require_turn_visible(session, turn).await?;
            tx.commit().await?;
            return Ok(receipt);
        }
        let receipt = tx.record_feedback(session, turn, &input, &hash(request.to_string())).await?;
        Self::finish(tx, receipt).await
    }

    pub async fn session_feedback(&self, a: &AuthContext, session: Uuid, turn: Uuid) -> Result<Value> {
        session_id(session)?;
        let mut tx = self.read(a, Permission::Write).await?;
        let value = tx.session_feedback(session, turn).await?;
        tx.commit().await?;
        Ok(value)
    }
}
