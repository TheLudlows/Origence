//! Submission and moderation of independent, immutable knowledge releases.
use super::deployment::unauthorized_admin;
use super::*;
use serde::Deserialize;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlobalKnowledgeDecision {
    Approve,
    Reject,
    Withdraw,
}

impl GlobalKnowledgeDecision {
    pub(crate) fn transition(self) -> (&'static str, &'static str) {
        match self {
            Self::Approve => ("pending", "approved"),
            Self::Reject => ("pending", "rejected"),
            Self::Withdraw => ("approved", "withdrawn"),
        }
    }
}

pub(crate) struct GlobalKnowledgeChunk {
    pub(crate) content: String,
    pub(crate) terms: String,
    pub(crate) start: i64,
    pub(crate) end: i64,
}

impl Service {
    /// Submit the exact published version from the caller's workspace.
    /// The model, original files, archived turns and source IDs are not exported.
    pub async fn submit_global_knowledge(
        &self,
        auth: &AuthContext,
        asset: Uuid,
        version: i32,
    ) -> Result<Value> {
        if asset.is_nil() || version <= 0 {
            return Err(AppError::Invalid(
                "a published knowledge version is required".into(),
            ));
        }
        let mut tx = self.read(auth, Permission::Write).await?;
        if let Some(existing) = tx.global_submission(asset, version).await? {
            tx.commit().await?;
            return Ok(existing);
        }
        let v = tx.global_knowledge_source(asset, version).await?;
        tx.commit().await?;
        let mut materialized = self.get(auth, asset, Some(version)).await?;
        let content: String = decode(materialized["content"].take())?;
        materialized
            .as_object_mut()
            .expect("asset view")
            .remove("content");
        if v != materialized {
            return Err(AppError::Conflict("knowledge version changed".into()));
        }
        let chunks = tokio::task::spawn_blocking(move || {
            parsing::chunks(&content, "text").map(|chunks| {
                chunks
                    .into_iter()
                    .map(|chunk| GlobalKnowledgeChunk {
                        start: chunk.locator["byte_start"]
                            .as_i64()
                            .expect("valid source range"),
                        end: chunk.locator["byte_end"]
                            .as_i64()
                            .expect("valid source range"),
                        terms: format!(" {} ", parsing::lexical(&chunk.content)),
                        content: chunk.content,
                    })
                    .collect::<Vec<_>>()
            })
        })
        .await
        .map_err(|error| AppError::Internal(error.into()))??;
        let mut tx = self.write(auth, Permission::Write).await?;
        let result = tx
            .submit_global_snapshot(asset, version, &v, &chunks)
            .await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn global_knowledge_catalog(
        &self,
        auth: &AuthContext,
        after: Uuid,
        limit: usize,
    ) -> Result<Value> {
        let mut tx = self.read(auth, Permission::Read).await?;
        let result = tx.global_catalog(None, after, limit).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn global_knowledge(&self, auth: &AuthContext, id: Uuid) -> Result<Value> {
        let mut tx = self.read(auth, Permission::Read).await?;
        let result = tx.global_snapshot(id, false).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn global_review_catalog(
        &self,
        token: &str,
        state: &str,
        after: Uuid,
        limit: usize,
    ) -> Result<Value> {
        if !matches!(state, "pending" | "approved" | "rejected" | "withdrawn") {
            return Err(AppError::Invalid("invalid global knowledge state".into()));
        }
        let mut tx = self
            .engine
            .relational()
            .global_moderation(token, false)
            .await
            .map_err(unauthorized_admin)?;
        let result = tx.global_catalog(Some(state), after, limit).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn global_review_detail(&self, token: &str, id: Uuid) -> Result<Value> {
        let mut tx = self
            .engine
            .relational()
            .global_moderation(token, false)
            .await
            .map_err(unauthorized_admin)?;
        let result = tx.global_snapshot(id, true).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn review_global_knowledge(
        &self,
        token: &str,
        id: Uuid,
        decision: GlobalKnowledgeDecision,
    ) -> Result<Value> {
        let mut tx = self
            .engine
            .relational()
            .global_moderation(token, true)
            .await
            .map_err(unauthorized_admin)?;
        let result = tx.moderate_global_snapshot(id, decision).await?;
        tx.commit().await?;
        Ok(result)
    }
}
