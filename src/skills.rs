//! Explicit Skill review workflow. Candidate generation never publishes.
use crate::{
    error::Result,
    service::Service,
    skill::{SkillCandidateInput, SkillPackage, SkillReviewInput, SkillReviewPacket},
    storage::{DomainTx, Permission},
    types::AuthContext,
};
use serde_json::{Value, json};
use uuid::Uuid;

impl Service {
    pub async fn save_skill_candidate(
        &self,
        auth: &AuthContext,
        id: Uuid,
        expected_revision: u32,
        input: SkillCandidateInput,
    ) -> Result<Value> {
        let mut tx = self.write(auth, Permission::Write).await?;
        let revision = tx
            .save_skill_candidate(id, expected_revision, &input)
            .await?;
        tx.commit().await?;
        Ok(json!({"candidate_id":id,"revision":revision}))
    }
    pub async fn skill_candidate(&self, auth: &AuthContext, id: Uuid) -> Result<Value> {
        let mut tx = self.read(auth, Permission::Write).await?;
        let result = tx.skill_candidate(id).await?;
        tx.commit().await?;
        Ok(result)
    }
    pub async fn submit_skill_review(
        &self,
        auth: &AuthContext,
        candidate: Uuid,
        review: Uuid,
        input: SkillReviewInput,
    ) -> Result<SkillReviewPacket> {
        let mut tx = self.write(auth, Permission::Write).await?;
        let result = tx.submit_skill_review(candidate, review, &input).await?;
        tx.commit().await?;
        Ok(result)
    }
    pub async fn skill_review(&self, auth: &AuthContext, review: Uuid) -> Result<Value> {
        let mut tx = self.read(auth, Permission::Read).await?;
        let result = tx.skill_review(review).await?;
        tx.commit().await?;
        Ok(result)
    }
    pub async fn decide_skill_review(
        &self,
        auth: &AuthContext,
        review: Uuid,
        digest: &str,
        approve: bool,
    ) -> Result<Value> {
        let mut tx = self.write(auth, Permission::Publish).await?;
        tx.decide_skill_review(review, digest, approve).await?;
        tx.commit().await?;
        Ok(json!({"review_id":review,"approved":approve}))
    }
    pub async fn publish_skill(
        &self,
        auth: &AuthContext,
        review: Uuid,
        digest: &str,
    ) -> Result<SkillPackage> {
        let mut tx = self.write(auth, Permission::Publish).await?;
        let result = tx.publish_skill(review, digest).await?;
        tx.commit().await?;
        Ok(result)
    }
    pub async fn skill_package(
        &self,
        auth: &AuthContext,
        id: Uuid,
        version: u32,
    ) -> Result<SkillPackage> {
        let mut tx = self.read(auth, Permission::Read).await?;
        let result = tx.skill_package(id, version).await?;
        tx.commit().await?;
        Ok(result)
    }
    pub async fn withdraw_skill(
        &self,
        auth: &AuthContext,
        id: Uuid,
        version: u32,
    ) -> Result<Value> {
        let mut tx = self.write(auth, Permission::Publish).await?;
        tx.withdraw_skill(id, version).await?;
        tx.commit().await?;
        Ok(json!({"skill_id":id,"version":version,"withdrawn":true}))
    }
}
