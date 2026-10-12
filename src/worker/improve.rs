//! Explicit feedback learning within the existing serialized durable worker.
//!
//! The immutable proposal and verification are committed separately, before
//! any assets or model-driven indexes. A restart reuses each committed stage.
//! Accepted candidates enter the ordinary single-asset Ingest pipeline; only
//! their final publication transactions can account for them and advance the
//! session watermark.
use super::*;
use crate::learning::{FrozenSnapshot, ImproveRun, LearningCandidate, MAX_CANDIDATES};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalPlan {
    candidates: Vec<LearningCandidate>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VerificationDecision {
    candidate: usize,
    accepted: bool,
    reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VerificationPlan {
    decisions: Vec<VerificationDecision>,
}

fn invalid_plan() -> AppError {
    AppError::Invalid("invalid learning plan".into())
}

impl ProposalPlan {
    fn parse(value: Value, snapshot: &FrozenSnapshot, conditions: &[String]) -> Result<Self> {
        if value.to_string().len() > 128 * 1024 {
            return Err(invalid_plan());
        }
        let plan: Self = serde_json::from_value(value).map_err(|_| invalid_plan())?;
        if plan.candidates.len() > MAX_CANDIDATES {
            return Err(invalid_plan());
        }
        for (index, candidate) in plan.candidates.iter().enumerate() {
            candidate
                .validate(snapshot, conditions)
                .map_err(|_| invalid_plan())?;
            if plan.candidates[..index].contains(candidate) {
                return Err(invalid_plan());
            }
        }
        Ok(plan)
    }
}

impl VerificationPlan {
    fn parse(value: Value, candidate_count: usize) -> Result<Self> {
        if value.to_string().len() > 32 * 1024 {
            return Err(invalid_plan());
        }
        let plan: Self = serde_json::from_value(value).map_err(|_| invalid_plan())?;
        if plan.decisions.len() != candidate_count || candidate_count > MAX_CANDIDATES {
            return Err(invalid_plan());
        }
        let mut seen = std::collections::HashSet::new();
        for decision in &plan.decisions {
            if decision.candidate >= candidate_count
                || !seen.insert(decision.candidate)
                || decision.reason.trim().is_empty()
                || decision.reason.len() > 2048
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
                return Err(invalid_plan());
            }
        }
        Ok(plan)
    }
}

pub(super) async fn guard_improve(
    tx: &mut SqliteTx,
    claim: &ClaimedJob,
    payload: &JobPayload,
) -> Result<()> {
    let JobPayload::Improve {
        session_id,
        improve_id,
        ..
    } = payload
    else {
        return Err(invalid_plan());
    };
    let run = tx.improve_run(*session_id, *improve_id).await?;
    if run.job_id != claim.job_id
        || run.status == "cancelled"
        || claim.asset.is_some()
        || claim.source.is_some()
    {
        return Err(AppError::Conflict("learning run binding changed".into()));
    }
    tx.require_improve_snapshot_visible(&run).await?;
    Ok(())
}

fn check_model_profile(service: &Service, payload: &JobPayload) -> Result<()> {
    let JobPayload::Improve {
        extraction_profile,
        extraction_model,
        ..
    } = payload
    else {
        return Err(invalid_plan());
    };
    if service.models.improve_extraction_profile().as_deref() != Some(extraction_profile.as_str())
        || service.models.summary_model() != Some(extraction_model.as_str())
    {
        return Err(AppError::Unavailable(
            "worker extraction profile differs from accepted job".into(),
        ));
    }
    Ok(())
}

fn learning_text(candidate: &LearningCandidate) -> String {
    format!(
        "Conditional learning\nConditions:\n{}\n\nStatement:\n{}\n\nWhy:\n{}",
        candidate.conditions.join("\n"),
        candidate.statement,
        candidate.why,
    )
}

pub(super) async fn process(
    service: &Service,
    claim: &ClaimedJob,
    payload: &JobPayload,
) -> Result<()> {
    let JobPayload::Improve {
        session_id,
        improve_id,
        ..
    } = payload
    else {
        return Err(invalid_plan());
    };
    let a = auth(claim);
    let mut tx = service.read(&a, Permission::Write).await?;
    guard(&mut tx, claim, payload).await?;
    let run: ImproveRun = tx.improve_run(*session_id, *improve_id).await?;
    tx.commit().await?;

    let proposal = if let Some(saved) = run.proposal_plan.clone() {
        ProposalPlan::parse(saved, &run.snapshot, &run.conditions)?
    } else {
        let value = if run.snapshot.items.is_empty() {
            json!({"candidates":[]})
        } else {
            check_model_profile(service, payload)?;
            service
                .models
                .propose_learning(&run.snapshot, &run.conditions)
                .await?
        };
        let proposal = ProposalPlan::parse(value, &run.snapshot, &run.conditions)?;
        let mut tx = service.write(&a, Permission::Write).await?;
        guard(&mut tx, claim, payload).await?;
        tx.save_improve_proposal(*session_id, *improve_id, &json!(proposal))
            .await?;
        tx.audit(
            "learning.proposed",
            *improve_id,
            json!({"job_id":claim.job_id,"candidate_count":proposal.candidates.len()}),
        )
        .await?;
        tx.commit().await?;
        proposal
    };

    let verification = if let Some(saved) = run.verification_plan.clone() {
        VerificationPlan::parse(saved, proposal.candidates.len())?
    } else {
        // Revocation/cancellation after proposing must also stop verification.
        let mut tx = service.read(&a, Permission::Write).await?;
        guard(&mut tx, claim, payload).await?;
        tx.commit().await?;
        let value = if proposal.candidates.is_empty() {
            json!({"decisions":[]})
        } else {
            check_model_profile(service, payload)?;
            service
                .models
                .verify_learning(&run.snapshot, &run.conditions, &json!(proposal))
                .await?
        };
        let mut verification = VerificationPlan::parse(value, proposal.candidates.len())?;
        let mut tx = service.write(&a, Permission::Write).await?;
        guard(&mut tx, claim, payload).await?;
        // Keep the first accepted exact statement/condition pair, regardless of
        // differing explanations or support lists. This is exact novelty only,
        // not semantic deduplication or proof of a factual claim.
        verification.decisions.sort_by_key(|d| d.candidate);
        let mut seen = std::collections::HashSet::new();
        for decision in verification.decisions.iter_mut().filter(|d| d.accepted) {
            let candidate = &proposal.candidates[decision.candidate];
            if !seen.insert((candidate.statement.clone(), candidate.conditions.clone()))
                || tx.learning_duplicate(candidate).await?.is_some()
            {
                decision.accepted = false;
                decision.reason = "duplicate".into();
            }
        }
        tx.save_improve_verification(*session_id, *improve_id, &json!(verification))
            .await?;
        tx.audit(
            "learning.verified",
            *improve_id,
            json!({"job_id":claim.job_id,"accepted_count":verification.decisions.iter().filter(|d| d.accepted).count()}),
        )
        .await?;
        tx.commit().await?;
        verification
    };

    // One local transaction owns all registrations and settlement. Each child
    // is an ordinary Ingest job and publishes atomically by itself; a failed
    // child remains unresolved and can retry its existing durable job ID.
    let mut tx = service.write(&a, Permission::Write).await?;
    guard(&mut tx, claim, payload).await?;
    let mut learnings = Vec::new();
    for decision in verification.decisions.iter().filter(|d| d.accepted) {
        let candidate = &proposal.candidates[decision.candidate];
        let learning_id = graph::entity_id(&format!(
            "learning:{}:{}:{}:{}",
            claim.scope.tenant_id, claim.scope.workspace_id, improve_id, decision.candidate
        ));
        let title = "Conditional learning";
        let source = tx
            .create_event("learning", &learning_text(candidate), None)
            .await?;
        let asset = tx.insert_knowledge_asset(title).await?;
        let child = Service::enqueue(
            &mut tx,
            JobPayload::Ingest {
                format: "text".into(),
                title: title.into(),
                expected_version: None,
                embedding_profile: payload.embedding_profile().map(str::to_owned),
            },
            Some(asset),
            Some(source),
        )
        .await?;
        tx.create_learning(
            *session_id,
            *improve_id,
            learning_id,
            asset,
            source,
            child,
            candidate,
        )
        .await?;
        learnings.push(json!({
            "learning_id":learning_id,"asset_id":asset,"source_event_id":source,
            "job_id":child,"candidate":decision.candidate
        }));
    }
    let stage = if learnings.is_empty() {
        "complete"
    } else {
        "indexing"
    };
    tx.settle_completed(
        claim.job_id,
        stage,
        &json!({
            "improve_id":improve_id,"session_id":session_id,"readiness":stage,
            "learnings":learnings,"candidate_count":proposal.candidates.len(),
            "skipped_count":run.snapshot.skipped.len()
        }),
    )
    .await?;
    tx.complete_improve(*session_id, *improve_id).await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_must_account_for_every_candidate_exactly_once() {
        assert!(VerificationPlan::parse(json!({"decisions":[]}), 0).is_ok());
        assert!(VerificationPlan::parse(json!({"decisions":[]}), 1).is_err());
        assert!(
            VerificationPlan::parse(
                json!({"decisions":[
                    {"candidate":0,"accepted":false,"reason":"unsupported"},
                    {"candidate":0,"accepted":true,"reason":"supported"}
                ]}),
                2
            )
            .is_err()
        );
        assert!(
            VerificationPlan::parse(
                json!({"decisions":[
                    {"candidate":1,"accepted":false,"reason":"unsupported"}
                ]}),
                1
            )
            .is_err()
        );
        assert!(
            VerificationPlan::parse(
                json!({"decisions":[
                    {"candidate":0,"accepted":true,"reason":""}
                ]}),
                1
            )
            .is_err()
        );
        assert!(
            VerificationPlan::parse(
                json!({"decisions":[
                    {"candidate":0,"accepted":true,"reason":"supported","publish":true}
                ]}),
                1
            )
            .is_err()
        );
        assert!(
            VerificationPlan::parse(
                json!({"decisions":[
                    {"candidate":1,"accepted":false,"reason":"unsupported"},
                    {"candidate":0,"accepted":true,"reason":"supported"}
                ]}),
                2
            )
            .is_ok()
        );
    }
}
