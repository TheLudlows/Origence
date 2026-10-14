//! Durable, scoped feedback snapshots, immutable plans and learning provenance.
use super::*;
use crate::learning::{
    FrozenFeedback, FrozenSnapshot, FrozenTurn, ImproveInput, ImproveRun, LearningCandidate,
    MAX_CANDIDATES, MAX_SNAPSHOT_BYTES, SkippedFeedback,
};
use crate::session::{ArchivedEvidence, FeedbackInput};

pub(super) const SCHEMA: &str = include_str!("../learning-schema.sql");

pub(super) async fn available(conn: &mut Connection) -> StorageResult<bool> {
    // All three reads are intentional: an earlier true arm must not hide a
    // missing-table/backend error that the original statement had to prepare.
    let schema: bool = native_turso::query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name IN ('oc_improve_cursors','oc_improve_runs','oc_learnings','oc_learning_support'))")
        .fetch_one(&mut *conn).await.map_err(turso_err)?;
    let events: bool = native_turso::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_events WHERE kind='learning')")
        .fetch_one(&mut *conn).await.map_err(turso_err)?;
    let jobs: bool = native_turso::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_jobs WHERE operation='improve')")
        .fetch_one(&mut *conn).await.map_err(turso_err)?;
    Ok(schema || events || jobs)
}

fn decode<T: serde::de::DeserializeOwned>(body: &str) -> StorageResult<T> {
    serde_json::from_str(body)
        .map_err(|_| StorageError::Unavailable("invalid learning record".into()))
}

fn encode<T: serde::Serialize>(value: &T) -> StorageResult<String> {
    serde_json::to_string(value)
        .map_err(|_| StorageError::Unavailable("invalid learning record".into()))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalPlan {
    candidates: Vec<LearningCandidate>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct VerificationPlan {
    decisions: Vec<Decision>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Decision {
    candidate: usize,
    accepted: bool,
    reason: String,
}

fn fingerprint(candidate: &LearningCandidate) -> StorageResult<String> {
    let canonical = encode(&(&candidate.statement, &candidate.conditions))?;
    Ok(format!("{:x}", Sha256::digest(canonical.as_bytes())))
}

fn proposals(run: &ImproveRun, value: &Value) -> StorageResult<ProposalPlan> {
    let body = encode(value)?;
    if body.len() > MAX_SNAPSHOT_BYTES {
        return Err(StorageError::InvalidArgument(
            "learning proposal plan exceeds 131072 bytes",
        ));
    }
    let plan: ProposalPlan = decode(&body)?;
    if plan.candidates.len() > MAX_CANDIDATES {
        return Err(StorageError::InvalidArgument(
            "at most 8 learning candidates are allowed",
        ));
    }
    for (index, candidate) in plan.candidates.iter().enumerate() {
        candidate
            .validate(&run.snapshot, &run.conditions)
            .map_err(StorageError::InvalidArgument)?;
        if plan.candidates[..index].contains(candidate) {
            return Err(StorageError::InvalidArgument(
                "duplicate learning candidate",
            ));
        }
    }
    Ok(plan)
}

fn verification(
    run: &ImproveRun,
    value: &Value,
) -> StorageResult<(ProposalPlan, VerificationPlan)> {
    let proposal = run
        .proposal_plan
        .as_ref()
        .ok_or_else(|| StorageError::Conflict("proposal plan is not saved".into()))?;
    let proposal = proposals(run, proposal)?;
    let body = encode(value)?;
    if body.len() > MAX_SNAPSHOT_BYTES {
        return Err(StorageError::InvalidArgument(
            "learning verification plan exceeds 131072 bytes",
        ));
    }
    let plan: VerificationPlan = decode(&body)?;
    if plan.decisions.len() != proposal.candidates.len() {
        return Err(StorageError::InvalidArgument(
            "verification must account for every candidate",
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for decision in &plan.decisions {
        if decision.candidate >= proposal.candidates.len()
            || !seen.insert(decision.candidate)
            || ![
                "supported",
                "unsupported",
                "conditions_mismatch",
                "conflicting",
                "unsafe",
                "duplicate",
            ]
            .contains(&decision.reason.as_str())
            || decision.accepted != (decision.reason == "supported")
        {
            return Err(StorageError::InvalidArgument(
                "invalid learning verification decision",
            ));
        }
    }
    Ok((proposal, plan))
}

impl TursoTx {
    async fn require_improve(&mut self) -> StorageResult<()> {
        if !available(&mut self.tx).await? {
            return Err(StorageError::Unavailable(
                "feedback learning requires a new database".into(),
            ));
        }
        Ok(())
    }

    async fn frozen_turn(&mut self, session: Uuid, turn: Uuid) -> StorageResult<FrozenTurn> {
        let source = self.require_turn_visible(session, turn).await?;
        let content = self
            .event_content(source, true)
            .await?
            .ok_or(StorageError::NotFound)?;
        let body: Value = decode(&content)?;
        let question = body["question"]
            .as_str()
            .ok_or_else(|| StorageError::Unavailable("invalid archived turn".into()))?;
        let answer = body["answer"]
            .as_str()
            .ok_or_else(|| StorageError::Unavailable("invalid archived turn".into()))?;
        let rows: Vec<String> = native_turso::query_scalar("SELECT evidence FROM oc_session_turn_evidence WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=? ORDER BY ordinal")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
            .fetch_all(&mut *self.tx).await.map_err(turso_err)?;
        let used_evidence: Vec<ArchivedEvidence> = rows
            .iter()
            .map(|row| decode(row))
            .collect::<StorageResult<_>>()?;
        Ok(FrozenTurn {
            session_id: session,
            turn_id: turn,
            source_event_id: source,
            question: question.into(),
            answer: answer.into(),
            used_evidence,
        })
    }

    /// Accept and freeze in the same transaction as registration of `job_id`.
    /// A deferred FK ensures neither the run nor its parent job can commit alone.
    pub async fn accept_improve(
        &mut self,
        session: Uuid,
        input: &ImproveInput,
        request_hash: &str,
        job_id: Uuid,
    ) -> StorageResult<ImproveRun> {
        self.check_permission(Permission::Write).await?;
        self.require_improve().await?;
        input.validate().map_err(StorageError::InvalidArgument)?;
        if session.is_nil() || job_id.is_nil() {
            return Err(StorageError::InvalidArgument(
                "session and job identifiers must be non-nil",
            ));
        }
        let existing: Option<(Uuid, String, String)> = native_turso::query_as("SELECT session_id,request_hash,request FROM oc_improve_runs WHERE tenant_id=? AND workspace_id=? AND improve_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(input.improve_id)
            .fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        if let Some((stored_session, hash, request)) = existing {
            if stored_session != session
                || hash != request_hash
                || decode::<ImproveInput>(&request)? != *input
            {
                return Err(StorageError::Conflict(
                    "improve request is immutable".into(),
                ));
            }
            let run = self.improve_run(session, input.improve_id).await?;
            self.require_improve_snapshot_visible(&run).await?;
            return Ok(run);
        }
        let session_exists: bool = native_turso::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_session_turns WHERE tenant_id=? AND workspace_id=? AND session_id=?)")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session)
            .fetch_one(&mut *self.tx).await.map_err(turso_err)?;
        if !session_exists {
            return Err(StorageError::NotFound);
        }
        let unfinished: bool = native_turso::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_improve_runs WHERE tenant_id=? AND workspace_id=? AND session_id=? AND status IN ('accepted','publishing'))")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session)
            .fetch_one(&mut *self.tx).await.map_err(turso_err)?;
        if unfinished {
            return Err(StorageError::Conflict(
                "session already has an unfinished improve run".into(),
            ));
        }
        native_turso::query("INSERT INTO oc_improve_cursors(tenant_id,workspace_id,session_id,feedback_sequence) VALUES(?,?,?,0) ON CONFLICT DO NOTHING")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session)
            .execute(&mut *self.tx).await.map_err(turso_err)?;
        let lower: i64 = native_turso::query_scalar("SELECT feedback_sequence FROM oc_improve_cursors WHERE tenant_id=? AND workspace_id=? AND session_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session)
            .fetch_one(&mut *self.tx).await.map_err(turso_err)?;
        let rows: Vec<(i64, Uuid, Uuid, Uuid, String)> = native_turso::query_as("SELECT feedback_sequence,principal_id,feedback_id,turn_id,feedback FROM oc_feedback WHERE tenant_id=? AND workspace_id=? AND session_id=? AND feedback_sequence>? ORDER BY feedback_sequence LIMIT ?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session)
            .bind(lower).bind(input.max_feedback as i64)
            .fetch_all(&mut *self.tx).await.map_err(turso_err)?;
        let mut snapshot = FrozenSnapshot::default();
        let mut target = lower;
        for (sequence, principal_id, feedback_id, turn_id, body) in rows {
            match self.frozen_turn(session, turn_id).await {
                Ok(turn) => {
                    let feedback: FeedbackInput = decode(&body)?;
                    if feedback.feedback_id != feedback_id {
                        return Err(StorageError::Unavailable(
                            "invalid frozen feedback identity".into(),
                        ));
                    }
                    snapshot.items.push(FrozenFeedback {
                        sequence,
                        principal_id,
                        feedback,
                        turn,
                    });
                    if encode(&snapshot)?.len() > MAX_SNAPSHOT_BYTES
                        || encode(
                            &json!({"conditions":input.conditions,"feedback":snapshot.items}),
                        )?
                        .len()
                            > MAX_SNAPSHOT_BYTES
                    {
                        snapshot.items.pop();
                        if snapshot.items.is_empty() {
                            return Err(StorageError::InvalidArgument(
                                "first feedback snapshot exceeds 131072 bytes",
                            ));
                        }
                        break;
                    }
                }
                Err(StorageError::NotFound) => {
                    snapshot.skipped.push(SkippedFeedback {
                        sequence,
                        principal_id,
                        feedback_id,
                        turn_id,
                        reason: "source_not_visible".into(),
                    });
                    if encode(&snapshot)?.len() > MAX_SNAPSHOT_BYTES
                        || encode(
                            &json!({"conditions":input.conditions,"feedback":snapshot.items}),
                        )?
                        .len()
                            > MAX_SNAPSHOT_BYTES
                    {
                        snapshot.skipped.pop();
                        break;
                    }
                }
                Err(error) => return Err(error),
            }
            target = sequence;
        }
        native_turso::query("INSERT INTO oc_improve_runs(tenant_id,workspace_id,improve_id,session_id,job_id,request_hash,request,snapshot,lower_sequence,target_sequence,created_by,created_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(input.improve_id).bind(session).bind(job_id)
            .bind(request_hash).bind(encode(input)?).bind(encode(&snapshot)?).bind(lower).bind(target)
            .bind(self.principal_id).bind(now_ms()).execute(&mut *self.tx).await.map_err(turso_err)?;
        self.audit("session.improve_accepted", input.improve_id,
            json!({"session_id":session,"job_id":job_id,"lower_sequence":lower,"target_sequence":target,"feedback_count":snapshot.items.len(),"skipped_count":snapshot.skipped.len()})).await?;
        self.improve_run(session, input.improve_id).await
    }

    pub async fn lookup_improve(&mut self, improve_id: Uuid) -> StorageResult<ImproveRun> {
        self.check_permission(Permission::Write).await?;
        self.require_improve().await?;
        let row: Option<(Uuid, Uuid, String, i64, i64, String, Option<String>, Option<String>, String)> = native_turso::query_as("SELECT session_id,job_id,request,lower_sequence,target_sequence,snapshot,proposal_plan,verification_plan,status FROM oc_improve_runs WHERE tenant_id=? AND workspace_id=? AND improve_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(improve_id)
            .fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        let (
            session_id,
            job_id,
            request,
            lower_sequence,
            target_sequence,
            snapshot,
            proposal,
            verification,
            status,
        ) = row.ok_or(StorageError::NotFound)?;
        let input: ImproveInput = decode(&request)?;
        Ok(ImproveRun {
            improve_id,
            session_id,
            job_id,
            conditions: input.conditions,
            lower_sequence,
            target_sequence,
            snapshot: decode(&snapshot)?,
            proposal_plan: proposal.as_deref().map(decode).transpose()?,
            verification_plan: verification.as_deref().map(decode).transpose()?,
            status,
        })
    }

    pub async fn improve_run(&mut self, session: Uuid, improve: Uuid) -> StorageResult<ImproveRun> {
        let run = self.lookup_improve(improve).await?;
        if run.session_id != session {
            return Err(StorageError::NotFound);
        }
        Ok(run)
    }

    /// Recheck live support before every saved plan, staging and publication.
    pub async fn require_improve_snapshot_visible(
        &mut self,
        run: &ImproveRun,
    ) -> StorageResult<()> {
        self.check_permission(Permission::Write).await?;
        for item in &run.snapshot.items {
            if item.turn.session_id != run.session_id
                || self.frozen_turn(run.session_id, item.turn.turn_id).await? != item.turn
            {
                return Err(StorageError::NotFound);
            }
            let stored: Option<(i64, String)> = native_turso::query_as("SELECT feedback_sequence,feedback FROM oc_feedback WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=? AND principal_id=? AND feedback_id=?")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(run.session_id)
                .bind(item.turn.turn_id).bind(item.principal_id).bind(item.feedback.feedback_id)
                .fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
            let Some((sequence, body)) = stored else {
                return Err(StorageError::NotFound);
            };
            if sequence != item.sequence || decode::<FeedbackInput>(&body)? != item.feedback {
                return Err(StorageError::NotFound);
            }
        }
        Ok(())
    }

    pub async fn save_improve_proposal(
        &mut self,
        session: Uuid,
        improve: Uuid,
        plan: &Value,
    ) -> StorageResult<()> {
        let run = self.improve_run(session, improve).await?;
        self.require_improve_snapshot_visible(&run).await?;
        proposals(&run, plan)?;
        if let Some(existing) = run.proposal_plan {
            return if existing == *plan {
                Ok(())
            } else {
                Err(StorageError::Conflict("proposal plan is immutable".into()))
            };
        }
        if run.status != "accepted" {
            return Err(StorageError::Conflict(
                "improve run is no longer accepting a plan".into(),
            ));
        }
        native_turso::query("UPDATE oc_improve_runs SET proposal_plan=? WHERE tenant_id=? AND workspace_id=? AND improve_id=? AND proposal_plan IS NULL")
            .bind(encode(plan)?).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(improve)
            .execute(&mut *self.tx).await.map_err(turso_err)?;
        Ok(())
    }

    pub async fn save_improve_verification(
        &mut self,
        session: Uuid,
        improve: Uuid,
        plan: &Value,
    ) -> StorageResult<()> {
        let run = self.improve_run(session, improve).await?;
        self.require_improve_snapshot_visible(&run).await?;
        verification(&run, plan)?;
        if let Some(existing) = run.verification_plan {
            return if existing == *plan {
                Ok(())
            } else {
                Err(StorageError::Conflict(
                    "verification plan is immutable".into(),
                ))
            };
        }
        if run.status != "accepted" {
            return Err(StorageError::Conflict(
                "improve run is no longer accepting verification".into(),
            ));
        }
        native_turso::query("UPDATE oc_improve_runs SET verification_plan=? WHERE tenant_id=? AND workspace_id=? AND improve_id=? AND verification_plan IS NULL")
            .bind(encode(plan)?).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(improve)
            .execute(&mut *self.tx).await.map_err(turso_err)?;
        Ok(())
    }

    /// Register a verified candidate after the caller stages its ordinary knowledge
    /// source, asset and child ingest job in this same transaction.
    pub async fn create_learning(
        &mut self,
        session: Uuid,
        improve: Uuid,
        learning_id: Uuid,
        asset: Uuid,
        source: Uuid,
        ingest_job: Uuid,
        candidate: &LearningCandidate,
    ) -> StorageResult<Value> {
        let run = self.improve_run(session, improve).await?;
        self.require_improve_snapshot_visible(&run).await?;
        if [learning_id, asset, source, ingest_job]
            .iter()
            .any(Uuid::is_nil)
        {
            return Err(StorageError::InvalidArgument(
                "learning identifiers must be non-nil",
            ));
        }
        let verification_value = run
            .verification_plan
            .as_ref()
            .ok_or_else(|| StorageError::Conflict("verification plan is not saved".into()))?;
        let (proposal, verification) = verification(&run, verification_value)?;
        let candidate_index = proposal
            .candidates
            .iter()
            .position(|item| item == candidate)
            .ok_or_else(|| {
                StorageError::Conflict("learning is outside the saved proposal".into())
            })?;
        if !verification
            .decisions
            .iter()
            .any(|decision| decision.candidate == candidate_index && decision.accepted)
        {
            return Err(StorageError::Conflict(
                "learning was not accepted by verification".into(),
            ));
        }
        let existing: Option<(Uuid, Uuid, Uuid, Uuid, String)> = native_turso::query_as("SELECT id,asset_id,source_event_id,ingest_job_id,candidate FROM oc_learnings WHERE tenant_id=? AND workspace_id=? AND improve_id=? AND candidate_index=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(improve).bind(candidate_index as i64)
            .fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        if let Some((id, stored_asset, stored_source, stored_job, body)) = existing {
            if (id, stored_asset, stored_source, stored_job)
                != (learning_id, asset, source, ingest_job)
                || decode::<LearningCandidate>(&body)? != *candidate
            {
                return Err(StorageError::Conflict(
                    "learning publication identities are immutable".into(),
                ));
            }
            return Ok(
                json!({"learning_id":id,"asset_id":asset,"source_event_id":source,"ingest_job_id":ingest_job}),
            );
        }
        if !["accepted", "publishing"].contains(&run.status.as_str()) {
            return Err(StorageError::Conflict("improve run is terminal".into()));
        }
        // Verification can be saved before another session publishes or reserves
        // the same statement. Serialize the final novelty check with registration
        // in this existing write transaction; do not mutate the frozen decision.
        if self
            .learning_registration_duplicate(candidate)
            .await?
            .is_some()
        {
            return Err(StorageError::Conflict(
                "learning already registered; cancel run and request again".into(),
            ));
        }
        let valid: bool = native_turso::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_assets a JOIN oc_events e ON e.tenant_id=a.tenant_id AND e.workspace_id=a.workspace_id JOIN oc_jobs j ON j.tenant_id=a.tenant_id AND j.workspace_id=a.workspace_id WHERE a.tenant_id=? AND a.workspace_id=? AND a.id=? AND a.kind='knowledge' AND a.deleted=0 AND e.id=? AND e.kind='learning' AND e.state='active' AND j.id=? AND j.asset_id=a.id AND j.source_event_id=e.id AND j.operation='ingest' AND j.cancel_requested=0 AND j.state IN ('pending','processing','retry_wait'))")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(asset).bind(source).bind(ingest_job)
            .fetch_one(&mut *self.tx).await.map_err(turso_err)?;
        if !valid {
            return Err(StorageError::Conflict(
                "learning requires its registered knowledge ingestion".into(),
            ));
        }
        native_turso::query("INSERT INTO oc_learnings(tenant_id,workspace_id,id,improve_id,session_id,candidate_index,asset_id,source_event_id,ingest_job_id,candidate,fingerprint,created_by,created_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(learning_id).bind(improve).bind(session).bind(candidate_index as i64)
            .bind(asset).bind(source).bind(ingest_job).bind(encode(candidate)?).bind(fingerprint(candidate)?).bind(self.principal_id).bind(now_ms())
            .execute(&mut *self.tx).await.map_err(turso_err)?;
        for (ordinal, support) in candidate.supports.iter().enumerate() {
            native_turso::query("INSERT INTO oc_learning_support(tenant_id,workspace_id,learning_id,session_id,turn_id,support_ordinal,principal_id,feedback_id) VALUES(?,?,?,?,?,?,?,?)")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(learning_id).bind(session).bind(support.turn_id)
                .bind(ordinal as i64).bind(support.principal_id).bind(support.feedback_id)
                .execute(&mut *self.tx).await.map_err(turso_err)?;
        }
        native_turso::query("UPDATE oc_improve_runs SET status='publishing' WHERE tenant_id=? AND workspace_id=? AND improve_id=? AND status='accepted'")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(improve)
            .execute(&mut *self.tx).await.map_err(turso_err)?;
        self.audit("learning.accepted", learning_id, json!({"improve_id":improve,"asset_id":asset,"source_event_id":source,"ingest_job_id":ingest_job})).await?;
        Ok(
            json!({"learning_id":learning_id,"asset_id":asset,"source_event_id":source,"ingest_job_id":ingest_job,"status":"accepted"}),
        )
    }

    /// Cursor consumption is atomic with the last successful child publication.
    /// Transient, cancelled or incomplete work never advances the cursor.
    pub async fn complete_improve(&mut self, session: Uuid, improve: Uuid) -> StorageResult<bool> {
        let run = self.improve_run(session, improve).await?;
        if run.status == "completed" {
            return Ok(true);
        }
        if run.status == "cancelled" {
            return Ok(false);
        }
        let Some(value) = run.verification_plan.as_ref() else {
            return Ok(false);
        };
        let (_, plan) = verification(&run, value)?;
        let accepted = plan
            .decisions
            .iter()
            .filter(|decision| decision.accepted)
            .count() as i64;
        let (count, published): (i64, i64) = native_turso::query_as("SELECT count(*),COALESCE(SUM(status='published'),0) FROM oc_learnings WHERE tenant_id=? AND workspace_id=? AND improve_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(improve)
            .fetch_one(&mut *self.tx).await.map_err(turso_err)?;
        if count != accepted || published != accepted {
            return Ok(false);
        }
        let parent_active: bool = native_turso::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_jobs WHERE tenant_id=? AND workspace_id=? AND id=? AND cancel_requested=0 AND state IN ('processing','completed'))")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(run.job_id)
            .fetch_one(&mut *self.tx).await.map_err(turso_err)?;
        if !parent_active {
            return Ok(false);
        }
        self.require_improve_snapshot_visible(&run).await?;
        let affected = native_turso::query("UPDATE oc_improve_cursors SET feedback_sequence=? WHERE tenant_id=? AND workspace_id=? AND session_id=? AND feedback_sequence=?")
            .bind(run.target_sequence).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(run.lower_sequence)
            .execute(&mut *self.tx).await.map_err(turso_err)?.rows_affected();
        if affected != 1 {
            return Err(StorageError::Conflict("improve cursor changed".into()));
        }
        native_turso::query("UPDATE oc_improve_runs SET status='completed',completed_at=? WHERE tenant_id=? AND workspace_id=? AND improve_id=?")
            .bind(now_ms()).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(improve)
            .execute(&mut *self.tx).await.map_err(turso_err)?;
        self.audit("session.improve_completed", improve,
            json!({"session_id":session,"target_sequence":run.target_sequence,"published_count":published,"skipped_count":run.snapshot.skipped.len()})).await?;
        Ok(true)
    }

    /// The normal ingestion publisher calls this under its existing job guard.
    pub async fn mark_learning_published(&mut self, asset: Uuid) -> StorageResult<()> {
        if !available(&mut self.tx).await? {
            return Ok(());
        }
        let row: Option<(Uuid, Uuid, Uuid, Uuid)> = native_turso::query_as("SELECT id,session_id,improve_id,source_event_id FROM oc_learnings WHERE tenant_id=? AND workspace_id=? AND asset_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(asset)
            .fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        let Some((id, session, improve, source)) = row else {
            return Ok(());
        };
        self.guard_learning_publication(Some(asset), Some(source))
            .await?;
        let ready: bool = native_turso::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_learnings l JOIN oc_assets a ON a.tenant_id=l.tenant_id AND a.workspace_id=l.workspace_id AND a.id=l.asset_id JOIN oc_versions v ON v.tenant_id=a.tenant_id AND v.workspace_id=a.workspace_id AND v.asset_id=a.id AND v.version=a.current_version AND v.source_event_id=l.source_event_id JOIN oc_jobs j ON j.tenant_id=l.tenant_id AND j.workspace_id=l.workspace_id AND j.id=l.ingest_job_id WHERE l.tenant_id=? AND l.workspace_id=? AND l.id=? AND j.state='completed' AND j.outcome='published' AND j.cancel_requested=0 AND EXISTS(SELECT 1 FROM oc_chunks c WHERE c.tenant_id=a.tenant_id AND c.workspace_id=a.workspace_id AND c.asset_id=a.id AND c.version=a.current_version))")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(id)
            .fetch_one(&mut *self.tx).await.map_err(turso_err)?;
        if !ready {
            return Err(StorageError::Conflict(
                "learning ingestion has not completed publication".into(),
            ));
        }
        let changed = native_turso::query("UPDATE oc_learnings SET status='published',published_at=COALESCE(published_at,?) WHERE tenant_id=? AND workspace_id=? AND id=? AND status='accepted'")
            .bind(now_ms()).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(id)
            .execute(&mut *self.tx).await.map_err(turso_err)?.rows_affected();
        if changed == 1 {
            self.audit(
                "learning.published",
                id,
                json!({"improve_id":improve,"asset_id":asset,"source_event_id":source}),
            )
            .await?;
        }
        self.complete_improve(session, improve).await?;
        Ok(())
    }

    pub async fn require_not_learning_asset(&mut self, asset: Uuid) -> StorageResult<()> {
        if self.is_learning_asset(asset).await? {
            return Err(StorageError::Conflict(
                "learning assets cannot be updated or restored".into(),
            ));
        }
        Ok(())
    }

    pub async fn improve_view(&mut self, improve: Uuid) -> StorageResult<Value> {
        let run = self.lookup_improve(improve).await?;
        let rows: Vec<(Uuid, Uuid, Uuid, Uuid, String)> = native_turso::query_as("SELECT id,asset_id,source_event_id,ingest_job_id,status FROM oc_learnings WHERE tenant_id=? AND workspace_id=? AND improve_id=? ORDER BY candidate_index")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(improve)
            .fetch_all(&mut *self.tx).await.map_err(turso_err)?;
        let mut learnings = Vec::with_capacity(rows.len());
        for (id, asset, source, job, status) in rows {
            let job_view = self.job_view(job).await?;
            learnings.push(json!({"learning_id":id,"asset_id":asset,"source_event_id":source,
                "ingest_job_id":job,"status":status,"job_state":job_view["state"],"error_code":job_view["error_code"]}));
        }
        let parent_job = self.job_view(run.job_id).await?;
        let mut accepted_count = 0;
        let mut rejected_count = 0;
        let mut duplicate_count = 0;
        if let Some(value) = &run.verification_plan {
            let (_, plan) = verification(&run, value)?;
            for decision in plan.decisions {
                if decision.accepted {
                    accepted_count += 1;
                } else {
                    rejected_count += 1;
                }
                if decision.reason == "duplicate" {
                    duplicate_count += 1;
                }
            }
        }
        Ok(
            json!({"improve_id":improve,"session_id":run.session_id,"job_id":run.job_id,"status":run.status,
            "watermark_before":run.lower_sequence,"watermark_target":run.target_sequence,
            "feedback_count":run.snapshot.items.len(),"skipped_count":run.snapshot.skipped.len(),"learnings":learnings,
            "proposal_saved":run.proposal_plan.is_some(),"verification_saved":run.verification_plan.is_some(),
            "accepted_count":accepted_count,"rejected_count":rejected_count,"duplicate_count":duplicate_count,
            "job_state":parent_job["state"],"error_code":parent_job["error_code"]}),
        )
    }

    pub async fn learning_view(&mut self, learning: Uuid) -> StorageResult<Value> {
        self.check_permission(Permission::Read).await?;
        self.require_improve().await?;
        let row: Option<(Uuid, Uuid, Uuid, Uuid, Uuid, String, String)> = native_turso::query_as("SELECT improve_id,session_id,asset_id,source_event_id,ingest_job_id,status,candidate FROM oc_learnings WHERE tenant_id=? AND workspace_id=? AND id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(learning)
            .fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        let (improve, session, asset, source, job, status, candidate) =
            row.ok_or(StorageError::NotFound)?;
        self.require_learning_asset_visible(asset).await?;
        let candidate: LearningCandidate = decode(&candidate)?;
        let supports: Vec<Value> = candidate.supports.iter().map(|support| json!({
            "turn_id":support.turn_id,"field":support.field,"byte_start":support.byte_start,
            "byte_end":support.byte_end,"feedback_id":support.feedback_id,"principal_id":support.principal_id
        })).collect();
        let mut result = json!({"statement":candidate.statement,"why":candidate.why,
            "conditions":candidate.conditions,"supports":supports});
        let object = result
            .as_object_mut()
            .ok_or_else(|| StorageError::Unavailable("invalid learning record".into()))?;
        object.extend(serde_json::Map::from_iter([
            ("learning_id".into(), json!(learning)),
            ("improve_id".into(), json!(improve)),
            ("session_id".into(), json!(session)),
            ("asset_id".into(), json!(asset)),
            ("source_event_id".into(), json!(source)),
            ("ingest_job_id".into(), json!(job)),
            ("status".into(), json!(status)),
        ]));
        Ok(result)
    }
}

impl TursoTx {
    /// Live accepted rows reserve their exact statement until publication or an
    /// explicit cancellation. This runs under the same write lock as insertion,
    /// closing both cross-session queue ordering and saved-verification races.
    async fn learning_registration_duplicate(
        &mut self,
        candidate: &LearningCandidate,
    ) -> StorageResult<Option<Uuid>> {
        let rows: Vec<(Uuid, Uuid, Uuid, String)> = native_turso::query_as("SELECT id,asset_id,source_event_id,candidate FROM oc_learnings WHERE tenant_id=? AND workspace_id=? AND fingerprint=? AND status IN ('accepted','published') ORDER BY created_at,id")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(fingerprint(candidate)?)
            .fetch_all(&mut *self.tx).await.map_err(turso_err)?;
        for (id, asset, source, body) in rows {
            let stored: LearningCandidate = decode(&body)?;
            if stored.statement != candidate.statement || stored.conditions != candidate.conditions
            {
                continue;
            }
            match self
                .guard_learning_publication(Some(asset), Some(source))
                .await
            {
                Ok(()) => return Ok(Some(id)),
                Err(StorageError::NotFound) => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }

    /// Exact novelty check only: semantic similarity never merges statements.
    pub async fn learning_duplicate(
        &mut self,
        candidate: &LearningCandidate,
    ) -> StorageResult<Option<Uuid>> {
        self.check_permission(Permission::Write).await?;
        self.require_improve().await?;
        let rows: Vec<(Uuid, Uuid, String)> = native_turso::query_as("SELECT id,asset_id,candidate FROM oc_learnings WHERE tenant_id=? AND workspace_id=? AND fingerprint=? AND status='published' ORDER BY created_at,id")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(fingerprint(candidate)?)
            .fetch_all(&mut *self.tx).await.map_err(turso_err)?;
        for (id, asset, body) in rows {
            let stored: LearningCandidate = decode(&body)?;
            if stored.statement != candidate.statement || stored.conditions != candidate.conditions
            {
                continue;
            }
            match self.require_learning_asset_visible(asset).await {
                Ok(()) => return Ok(Some(id)),
                Err(StorageError::NotFound) => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }

    /// Sanitized provenance attached to each ordinary knowledge chunk locator.
    pub async fn learning_publication_metadata(
        &mut self,
        asset: Uuid,
    ) -> StorageResult<Option<Value>> {
        if !available(&mut self.tx).await? {
            return Ok(None);
        }
        let row: Option<(Uuid, Uuid, String)> = native_turso::query_as("SELECT id,source_event_id,candidate FROM oc_learnings WHERE tenant_id=? AND workspace_id=? AND asset_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(asset)
            .fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        let Some((id, source, body)) = row else {
            return Ok(None);
        };
        self.guard_learning_publication(Some(asset), Some(source))
            .await?;
        let candidate: LearningCandidate = decode(&body)?;
        let mut support_turns: Vec<Uuid> = candidate
            .supports
            .iter()
            .map(|support| support.turn_id)
            .collect();
        support_turns.sort_unstable();
        support_turns.dedup();
        Ok(Some(
            json!({"learning_id":id,"conditions":candidate.conditions,"support_turns":support_turns}),
        ))
    }

    /// Explicit cancellation releases the per-session lock without consuming any
    /// feedback. Already-published knowledge keeps its independently valid source.
    pub async fn cancel_improve_job(&mut self, job: Uuid) -> StorageResult<bool> {
        self.check_permission(Permission::Write).await?;
        if !available(&mut self.tx).await? {
            return Ok(false);
        }
        let row: Option<(Uuid, Uuid, String)> = native_turso::query_as("SELECT improve_id,session_id,status FROM oc_improve_runs WHERE tenant_id=? AND workspace_id=? AND job_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(job)
            .fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        let Some((improve, session, status)) = row else {
            return Ok(false);
        };
        if !["accepted", "publishing"].contains(&status.as_str()) {
            return Ok(false);
        }
        let children: Vec<Uuid> = native_turso::query_scalar("SELECT j.id FROM oc_learnings l JOIN oc_jobs j ON j.tenant_id=l.tenant_id AND j.workspace_id=l.workspace_id AND j.id=l.ingest_job_id WHERE l.tenant_id=? AND l.workspace_id=? AND l.improve_id=? AND l.status!='published' AND j.state NOT IN ('completed','cancelled','superseded')")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(improve)
            .fetch_all(&mut *self.tx).await.map_err(turso_err)?;
        for child in children {
            self.set_job_action(child, "cancel").await?;
        }
        native_turso::query("UPDATE oc_improve_runs SET status='cancelled',completed_at=? WHERE tenant_id=? AND workspace_id=? AND improve_id=?")
            .bind(now_ms()).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(improve)
            .execute(&mut *self.tx).await.map_err(turso_err)?;
        self.audit(
            "session.improve_cancelled",
            improve,
            json!({"session_id":session,"job_id":job}),
        )
        .await?;
        Ok(true)
    }

    pub async fn require_improve_job_retry_allowed(&mut self, job: Uuid) -> StorageResult<()> {
        if !available(&mut self.tx).await? {
            return Ok(());
        }
        let cancelled: bool = native_turso::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_improve_runs r WHERE r.tenant_id=? AND r.workspace_id=? AND r.status='cancelled' AND (r.job_id=? OR EXISTS(SELECT 1 FROM oc_learnings l WHERE l.tenant_id=r.tenant_id AND l.workspace_id=r.workspace_id AND l.improve_id=r.improve_id AND l.ingest_job_id=?)))")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(job).bind(job)
            .fetch_one(&mut *self.tx).await.map_err(turso_err)?;
        if cancelled {
            return Err(StorageError::Conflict(
                "cancelled improve runs require a new improve_id".into(),
            ));
        }
        Ok(())
    }
}
