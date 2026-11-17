//! Authorized Run operations stay in one relational transaction.
use crate::{
    error::{AppError, Result},
    run::{RunEventInput, RunInput},
    service::Service,
    storage::{DomainTx, Permission},
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
        bundle: &crate::evidence::EvidenceBundle,
    ) -> Result<Value> {
        let mut tx = self.write(a, Permission::Write).await?;
        let result = tx
            .record_run_context_prepared(run, event_id, bundle)
            .await?;
        tx.commit().await?;
        Ok(result)
    }
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
