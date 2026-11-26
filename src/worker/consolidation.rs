//! Extracts from the immutable source and settles its separate long-term projection.
use super::{
    AppError, ClaimedJob, DomainTx, JobPayload, Result, Service, SqliteTx, auth, decode, guard,
};
use crate::{consolidation::GroundedFact, storage::Permission};
use serde_json::json;

pub(super) async fn process(
    service: &Service,
    claim: &ClaimedJob,
    payload: &JobPayload,
    mut read: SqliteTx,
) -> Result<()> {
    let JobPayload::ConsolidateMemory {
        extraction_profile,
        extraction_model,
    } = payload
    else {
        return Err(AppError::Invalid(
            "invalid memory consolidation operation".into(),
        ));
    };
    let source = claim.source.ok_or(AppError::NotFound)?;
    if claim.asset.is_some() {
        return Err(AppError::Invalid(
            "invalid memory consolidation target".into(),
        ));
    }
    let auth = auth(claim);
    let facts: Vec<GroundedFact> = if let Some(saved) = claim.payload.get("consolidation_plan") {
        read.commit().await?;
        decode(saved.clone())?
    } else {
        if service.models.consolidation_extraction_profile().as_deref()
            != Some(extraction_profile.as_str())
            || service.models.summary_model() != Some(extraction_model.as_str())
        {
            return Err(AppError::Unavailable(
                "worker extraction profile differs from accepted job".into(),
            ));
        }
        let content = read
            .event_content(source, true)
            .await?
            .ok_or(AppError::NotFound)?;
        let properties = read.consolidation_properties().await?;
        read.commit().await?;
        let mut facts = if properties.is_empty() {
            Vec::new()
        } else {
            service
                .models
                .extract_consolidation_facts(&content, &properties)
                .await?
        };
        // Keep only facts from the catalog snapshot presented to the model.
        // The write path also checks each accepted fact against the live catalog.
        facts.retain(|fact| {
            properties
                .iter()
                .any(|property| property.check_identity(&fact.identity).is_ok())
        });
        let mut write = service.write(&auth, Permission::Write).await?;
        guard(&mut write, claim, payload).await?;
        write
            .save_consolidation_plan(claim.job_id, &json!(facts))
            .await?;
        write.commit().await?;
        facts
    };

    let mut write = service.write(&auth, Permission::Write).await?;
    guard(&mut write, claim, payload).await?;
    let memories = write.consolidate_memory_facts(source, &facts).await?;
    write
        .settle_completed(
            claim.job_id,
            "consolidated",
            &json!({"source_event_id":source,"fact_count":facts.len(),"memories":memories}),
        )
        .await?;
    write.commit().await?;
    Ok(())
}
