//! Semantic capture runs within the original serialized durable worker.
use super::*;
use crate::{
    semantic_capture::{EXTRACTOR, Extraction, ExtractionProvenance, SemanticCaptureInput},
    semantics::{SemanticPlan, plan_semantic},
};

#[derive(Clone, Serialize, Deserialize)]
struct CapturePlan {
    extraction: Extraction,
    routes: Vec<SemanticPlan>,
}
/// Recheck every original member, including duplicates, on replay and before commit.
pub(super) async fn guard_capture(tx: &mut SqliteTx, claim: &ClaimedJob) -> Result<()> {
    if claim.payload["extractor"] != EXTRACTOR {
        return Err(AppError::Invalid("unsupported semantic extractor".into()));
    }
    if let Some(value) = claim.payload.get("semantic_capture_plan") {
        let plan: CapturePlan = decode(value.clone())?;
        for route in plan.routes {
            tx.valid_targets(Some(route.asset), None).await?;
            if tx.asset_current_version(route.asset).await? != route.expected_version {
                return Err(AppError::Conflict(
                    "semantic capture superseded by another version".into(),
                ));
            }
            for member in route.members {
                let text = tx
                    .event_content(member.source_event_id, true)
                    .await?
                    .ok_or_else(|| AppError::Conflict("semantic source unavailable".into()))?;
                if text.get(member.statement.byte_start..member.statement.byte_end)
                    != Some(member.statement.quote.as_str())
                {
                    return Err(AppError::Invalid("invalid semantic provenance".into()));
                }
            }
            // A duplicate must not report a fact as current after its publication
            // root was withdrawn, even if all its original members remain active.
            if route.duplicate {
                tx.asset_view(route.asset, route.expected_version).await?;
            }
        }
    }
    Ok(())
}
pub(super) fn results(claim: &ClaimedJob) -> Result<Vec<Value>> {
    let plan: CapturePlan = decode(claim.payload["semantic_capture_plan"].clone())?;
    Ok(plan
        .routes
        .iter()
        .zip(&plan.extraction.candidates)
        .map(|(route, candidate)| {
            let mut result = route.result();
            result["target"] = json!(candidate.target);
            result["requested_operation"] = json!(candidate.operation);
            result
        })
        .collect())
}
pub(super) async fn process(service: &Service, claim: &ClaimedJob) -> Result<()> {
    let a = auth(claim);
    let profile: Option<String> = decode(claim.payload["embedding_profile"].clone())?;
    let mut active = claim.clone();
    let mut tx = service.read(&a, Permission::Write).await?;
    guard(&mut tx, claim).await?;
    if let Some(saved) = claim
        .payload
        .get("publication")
        .filter(|p| p["generation"] == claim.generation)
    {
        let publication: Publication = decode(saved.clone())?;
        tx.commit().await?;
        return publish_prepared(service, claim, publication).await;
    }
    let plan = if let Some(saved) = claim.payload.get("semantic_capture_plan") {
        let plan: CapturePlan = decode(saved.clone())?;
        tx.commit().await?;
        plan
    } else {
        if json!(service.models.semantic_extraction_profile())
            != claim.payload["extraction_profile"]
        {
            return Err(AppError::Unavailable(
                "worker extraction profile differs from accepted job".into(),
            ));
        }
        let source = claim.source.ok_or(AppError::NotFound)?;
        let content = tx
            .event_content(source, true)
            .await?
            .ok_or(AppError::NotFound)?;
        let request = SemanticCaptureInput {
            content,
            targets: decode(claim.payload["targets"].clone())?,
        };
        let properties: Vec<crate::memory_semantics::MemoryProperty> =
            decode(claim.payload["properties"].clone())?;
        tx.commit().await?;
        let extraction = service
            .models
            .extract_semantic(&request, &properties)
            .await?;
        let provenance = ExtractionProvenance {
            parser: EXTRACTOR.into(),
            model: decode(claim.payload["extraction_model"].clone())?,
        };
        let mut tx = service.write(&a, Permission::Write).await?;
        guard(&mut tx, claim).await?;
        let mut routes = Vec::new();
        for candidate in &extraction.candidates {
            let input = candidate.input(&request)?;
            routes.push(plan_semantic(&mut tx, &input, source, Some(provenance.clone())).await?);
        }
        let plan = CapturePlan { extraction, routes };
        tx.save_semantic_capture_plan(claim.job_id, &json!(plan))
            .await?;
        tx.audit(
            "memory.semantic_capture_planned",
            claim.job_id,
            json!({"candidate_count":plan.routes.len(),"extractor":EXTRACTOR}),
        )
        .await?;
        tx.commit().await?;
        active.payload["semantic_capture_plan"] = json!(plan);
        plan
    };
    let drafts = plan
        .routes
        .into_iter()
        .filter(|r| !r.duplicate)
        .map(|route| DraftMemory {
            asset: route.asset,
            fact_key: None,
            expected_version: route.expected_version,
            chunks: route.chunks(),
        })
        .collect();
    let publication = prepare(service, &active, profile, drafts).await?;
    publish_prepared(service, &active, publication).await
}
