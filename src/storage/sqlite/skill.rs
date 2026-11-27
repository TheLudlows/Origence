//! Scoped, immutable Skill revisions and explicitly selected review packets.
//! All checks and writes use the caller's transaction; no IO or model calls.
use super::*;
use crate::skill::{
    SkillCandidateInput, SkillPackage, SkillReviewInput, SkillReviewPacket, SkillSource, decode,
    encode,
};

pub(super) const SCHEMA: &str = include_str!("../skill-schema.sql");

impl SqliteTx {
    async fn skill_head(&mut self, id: Uuid) -> StorageResult<(Uuid, u32)> {
        sqlite_db::query_as("SELECT owner_key_id,current_revision FROM oc_skill_candidates WHERE tenant_id=? AND workspace_id=? AND id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(id)
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?.ok_or(StorageError::NotFound)
    }
    async fn skill_revision(
        &mut self,
        id: Uuid,
        revision: u32,
    ) -> StorageResult<SkillCandidateInput> {
        let content: String = sqlite_db::query_scalar("SELECT content FROM oc_skill_revisions WHERE tenant_id=? AND workspace_id=? AND candidate_id=? AND revision=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(id).bind(i64::from(revision))
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?.ok_or(StorageError::NotFound)?;
        decode(&content)
    }
    async fn check_skill_sources(
        &mut self,
        owner: Uuid,
        sources: &[SkillSource],
    ) -> StorageResult<()> {
        for source in sources {
            match source {
                SkillSource::Asset {
                    asset_id,
                    version,
                    content_sha256,
                } => {
                    // Reuse exact-version source/semantic/Learning guards; never bypass
                    // withdrawn supporting evidence by checking just a cached digest.
                    let value = self.asset_view_inner(*asset_id, Some(*version)).await?;
                    if value["content_hash"].as_str() != Some(content_sha256) {
                        return Err(StorageError::NotFound);
                    }
                }
                SkillSource::Run {
                    run_id,
                    through_sequence,
                    digest,
                } => {
                    if self
                        .run_prefix_digest_for_owner(owner, *run_id, *through_sequence)
                        .await?
                        != *digest
                    {
                        return Err(StorageError::NotFound);
                    }
                }
            }
        }
        Ok(())
    }
    /// Save a private candidate revision. Expected zero creates; edits append,
    /// never overwrite, and make approvals for older revisions unusable.
    pub async fn save_skill_candidate(
        &mut self,
        id: Uuid,
        expected_revision: u32,
        input: &SkillCandidateInput,
    ) -> StorageResult<u32> {
        self.check_permission(Permission::Write).await?;
        let owner = self.private_key_id().await?;
        input.validate()?;
        if id.is_nil() {
            return Err(StorageError::InvalidArgument("invalid Skill candidate ID"));
        }
        let next = expected_revision
            .checked_add(1)
            .ok_or(StorageError::InvalidArgument("Skill revision exhausted"))?;
        let old = match self.skill_head(id).await {
            Ok(v) => Some(v),
            Err(StorageError::NotFound) => None,
            Err(e) => return Err(e),
        };
        if let Some((stored_owner, current)) = old {
            if owner != stored_owner {
                return Err(StorageError::NotFound);
            }
            if current == next && self.skill_revision(id, current).await? == *input {
                self.check_skill_sources(owner, &input.sources).await?;
                return Ok(current);
            }
            if current != expected_revision {
                return Err(StorageError::Conflict("Skill revision changed".into()));
            }
        } else if expected_revision != 0 {
            return Err(StorageError::Conflict("Skill candidate missing".into()));
        }
        self.check_skill_sources(owner, &input.sources).await?;
        if old.is_none() {
            sqlite_db::query("INSERT INTO oc_skill_candidates(tenant_id,workspace_id,id,owner_key_id,current_revision) VALUES(?,?,?,?,?)")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(id).bind(owner).bind(i64::from(next))
                .execute(&mut *self.tx).await.map_err(sqlite_err)?;
        } else {
            sqlite_db::query("UPDATE oc_skill_candidates SET current_revision=? WHERE tenant_id=? AND workspace_id=? AND id=?")
                .bind(i64::from(next)).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(id)
                .execute(&mut *self.tx).await.map_err(sqlite_err)?;
        }
        sqlite_db::query("INSERT INTO oc_skill_revisions(tenant_id,workspace_id,candidate_id,revision,content,created_at) VALUES(?,?,?,?,?,?)")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(id).bind(i64::from(next)).bind(encode(input)?).bind(now_ms())
            .execute(&mut *self.tx).await.map_err(sqlite_err)?;
        Ok(next)
    }
    /// Candidate content and private notes are owner-only, including for admins.
    pub async fn skill_candidate(&mut self, id: Uuid) -> StorageResult<Value> {
        self.check_permission(Permission::Write).await?;
        let owner = self.private_key_id().await?;
        let (stored_owner, revision) = self.skill_head(id).await?;
        if owner != stored_owner {
            return Err(StorageError::NotFound);
        }
        let input = self.skill_revision(id, revision).await?;
        self.check_skill_sources(owner, &input.sources).await?;
        Ok(json!({"candidate_id":id,"revision":revision,"input":input}))
    }
    /// Owner explicitly discloses only the package and selected basis to one
    /// credential-scoped reviewer. Rotation requires a new review appointment.
    pub async fn submit_skill_review(
        &mut self,
        candidate: Uuid,
        review: Uuid,
        input: &SkillReviewInput,
    ) -> StorageResult<SkillReviewPacket> {
        self.check_permission(Permission::Write).await?;
        let owner = self.private_key_id().await?;
        let (stored_owner, current) = self.skill_head(candidate).await?;
        if stored_owner != owner {
            return Err(StorageError::NotFound);
        }
        if current != input.revision {
            return Err(StorageError::Conflict("Skill revision changed".into()));
        }
        if review.is_nil()
            || !Permission::Write
                .granted_by(&self.live_role_for_principal(input.reviewer_id).await?)
        {
            return Err(StorageError::Forbidden);
        }
        let content = self.skill_revision(candidate, current).await?;
        self.check_skill_sources(owner, &content.sources).await?;
        if crate::skill_candidate::selected_basis(&content)?
            .is_some_and(|basis| basis != input.basis)
        {
            return Err(StorageError::Conflict(
                "edit the candidate revision to change selected basis".into(),
            ));
        }
        let package = content.draft.package(&input.target, self.scope)?;
        let packet = SkillReviewPacket::build(review, input, package)?;
        let old:Option<(Uuid,u32,Uuid,String)>=sqlite_db::query_as("SELECT candidate_id,revision,reviewer_id,packet FROM oc_skill_reviews WHERE tenant_id=? AND workspace_id=? AND id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(review)
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        if let Some((id, revision, reviewer, stored)) = old {
            if id != candidate
                || revision != current
                || reviewer != input.reviewer_id
                || decode::<SkillReviewPacket>(&stored)? != packet
            {
                return Err(StorageError::Conflict("Skill review ID reused".into()));
            }
            return Ok(packet);
        }
        sqlite_db::query("INSERT INTO oc_skill_reviews(tenant_id,workspace_id,id,candidate_id,revision,reviewer_id,packet,digest,created_at) VALUES(?,?,?,?,?,?,?,?,?)")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(review).bind(candidate).bind(i64::from(current)).bind(input.reviewer_id)
            .bind(encode(&packet)?).bind(&packet.digest).bind(now_ms()).execute(&mut *self.tx).await.map_err(sqlite_err)?;
        Ok(packet)
    }
    async fn skill_review_row(
        &mut self,
        review: Uuid,
    ) -> StorageResult<(Uuid, u32, Uuid, String, String, String)> {
        sqlite_db::query_as("SELECT candidate_id,revision,reviewer_id,packet,digest,decision FROM oc_skill_reviews WHERE tenant_id=? AND workspace_id=? AND id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(review)
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?.ok_or(StorageError::NotFound)
    }
    /// Selected packet only; publication roles do not grant access to the archive.
    pub async fn skill_review(&mut self, review: Uuid) -> StorageResult<Value> {
        self.check_permission(Permission::Read).await?;
        let (candidate, revision, reviewer, packet, _, decision) =
            self.skill_review_row(review).await?;
        let (owner, current) = self.skill_head(candidate).await?;
        if self.principal_id != reviewer && self.private_key_id().await? != owner {
            return Err(StorageError::NotFound);
        }
        if current != revision {
            return Err(StorageError::Conflict("Skill revision changed".into()));
        }
        let content = self.skill_revision(candidate, revision).await?;
        self.check_skill_sources(owner, &content.sources).await?;
        Ok(json!({"packet":decode::<SkillReviewPacket>(&packet)?,"decision":decision}))
    }
    /// Explicit user decision over exactly the packet displayed for review.
    pub async fn decide_skill_review(
        &mut self,
        review: Uuid,
        digest: &str,
        approve: bool,
    ) -> StorageResult<()> {
        self.check_permission(Permission::Write).await?;
        let (candidate, revision, reviewer, _, stored, decision) =
            self.skill_review_row(review).await?;
        if self.principal_id != reviewer {
            return Err(StorageError::Forbidden);
        }
        if stored != digest {
            return Err(StorageError::Conflict("Skill review digest changed".into()));
        }
        let (owner, current) = self.skill_head(candidate).await?;
        if current != revision {
            return Err(StorageError::Conflict("Skill revision changed".into()));
        }
        let content = self.skill_revision(candidate, revision).await?;
        self.check_skill_sources(owner, &content.sources).await?;
        let next = if approve { "approved" } else { "rejected" };
        if decision == next {
            return Ok(());
        }
        if decision != "pending" {
            return Err(StorageError::Conflict(
                "Skill decision is immutable; submit a new review".into(),
            ));
        }
        sqlite_db::query("UPDATE oc_skill_reviews SET decision=?,decided_at=? WHERE tenant_id=? AND workspace_id=? AND id=?")
            .bind(next).bind(now_ms()).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(review)
            .execute(&mut *self.tx).await.map_err(sqlite_err)?;
        Ok(())
    }
    /// Publication is separate from approval, with fresh permissions, source and
    /// revision checks. Only approved artifacts become workspace-readable.
    pub async fn publish_skill(
        &mut self,
        review: Uuid,
        digest: &str,
    ) -> StorageResult<SkillPackage> {
        self.check_permission(Permission::Write).await?;
        let (candidate, revision, reviewer, packet, stored, decision) =
            self.skill_review_row(review).await?;
        if decision != "approved" || stored != digest {
            return Err(StorageError::Conflict(
                "exact Skill approval required".into(),
            ));
        }
        if !Permission::Write.granted_by(&self.live_role_for_principal(reviewer).await?) {
            return Err(StorageError::Forbidden);
        }
        let (owner, current) = self.skill_head(candidate).await?;
        if revision != current {
            return Err(StorageError::Conflict("Skill revision changed".into()));
        }
        let content = self.skill_revision(candidate, revision).await?;
        self.check_skill_sources(owner, &content.sources).await?;
        let packet: SkillReviewPacket = decode(&packet)?;
        let package = content.draft.package(&packet.target, self.scope)?;
        if package != packet.package {
            return Err(StorageError::Conflict("Skill artifacts changed".into()));
        }
        let target = &packet.target;
        let previous:Option<(Uuid,bool)>=sqlite_db::query_as("SELECT review_id,withdrawn=1 FROM oc_skill_packages WHERE tenant_id=? AND workspace_id=? AND skill_id=? AND version=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(target.skill_id).bind(i64::from(target.version))
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        if let Some((old, withdrawn)) = previous {
            if old != review || withdrawn {
                return Err(StorageError::Conflict("Skill version is immutable".into()));
            }
            return Ok(package);
        }
        let latest:i64=sqlite_db::query_scalar("SELECT COALESCE(MAX(version),0) FROM oc_skill_packages WHERE tenant_id=? AND workspace_id=? AND skill_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(target.skill_id).fetch_one(&mut *self.tx).await.map_err(sqlite_err)?;
        if i64::from(target.version) != latest + 1 {
            return Err(StorageError::Conflict(
                "Skill version must follow published history".into(),
            ));
        }
        sqlite_db::query("INSERT INTO oc_skill_packages(tenant_id,workspace_id,skill_id,version,review_id,owner_key_id,sources,package,created_at) VALUES(?,?,?,?,?,?,?,?,?)")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(target.skill_id).bind(i64::from(target.version)).bind(review).bind(owner)
            .bind(encode(&content.sources)?).bind(encode(&package)?).bind(now_ms()).execute(&mut *self.tx).await.map_err(sqlite_err)?;
        Ok(package)
    }
    /// Live-authorized distribution; no private provenance or review basis returned.
    pub async fn skill_package(&mut self, id: Uuid, version: u32) -> StorageResult<SkillPackage> {
        self.check_permission(Permission::Read).await?;
        let (owner,sources,package):(Uuid,String,String)=sqlite_db::query_as("SELECT owner_key_id,sources,package FROM oc_skill_packages WHERE tenant_id=? AND workspace_id=? AND skill_id=? AND version=? AND NOT withdrawn")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(id).bind(i64::from(version))
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?.ok_or(StorageError::NotFound)?;
        self.check_skill_sources(owner, &decode::<Vec<SkillSource>>(&sources)?)
            .await?;
        let package: SkillPackage = decode(&package)?;
        package.validate()?;
        Ok(package)
    }
    pub async fn withdraw_skill(&mut self, id: Uuid, version: u32) -> StorageResult<()> {
        self.check_permission(Permission::Write).await?;
        let result=sqlite_db::query("UPDATE oc_skill_packages SET withdrawn=1 WHERE tenant_id=? AND workspace_id=? AND skill_id=? AND version=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(id).bind(i64::from(version))
            .execute(&mut *self.tx).await.map_err(sqlite_err)?;
        if result.rows_affected() == 0 {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }
}
