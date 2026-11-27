use crate::{
    error::{AppError, Result},
    graph,
    jobs::{JobKind, JobPayload},
    parsing,
    service::{Service, decode},
    storage::{sqlite::SqliteTx, *},
    types::{AuthContext, Chunk, GraphExtraction},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{sync::atomic::Ordering, time::Duration};
use uuid::Uuid;
mod consolidation;
mod improve;
mod semantic_capture;

#[derive(Clone, Serialize, Deserialize)]
struct PublishedChunk {
    id: Uuid,
    chunk: Chunk,
    embedding: Option<Vec<f32>>,
    summary: String,
    search_terms: String,
    summary_terms: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct PublishedMemory {
    asset: Uuid,
    #[serde(default)]
    fact_key: Option<String>,
    #[serde(default)]
    expected_version: Option<i32>,
    version: i32,
    chunks: Vec<PublishedChunk>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Publication {
    generation: i64,
    profile: Option<String>,
    #[serde(default)]
    summary_model: Option<String>,
    graph: Option<GraphExtraction>,
    memories: Vec<PublishedMemory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    receipt_metadata: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    full_text_operation_epoch: Option<u64>,
}
/// One asset's pending publication built before model IO, version still to be
/// verified against the live asset at commit time.
struct DraftMemory {
    asset: Uuid,
    fact_key: Option<String>,
    expected_version: Option<i32>,
    chunks: Vec<Chunk>,
}
fn auth(claim: &ClaimedJob) -> AuthContext {
    AuthContext {
        id: claim.created_by,
        tenant_id: claim.scope.tenant_id,
        workspace_id: claim.scope.workspace_id,
        role: String::new(),
    }
}
fn permission(payload: &JobPayload) -> Permission {
    if payload.kind() == JobKind::Restore {
        Permission::Publish
    } else {
        Permission::Write
    }
}
fn first_extracted_fact<'a>(
    seen: &mut std::collections::HashSet<&'a str>,
    content: &'a str,
    job_id: Uuid,
) -> bool {
    if seen.insert(content) {
        return true;
    }
    // Model-provided keys and statement text may contain sensitive data.
    tracing::warn!(%job_id, "duplicate extracted fact dropped");
    false
}
async fn guard(tx: &mut SqliteTx, claim: &ClaimedJob, payload: &JobPayload) -> Result<()> {
    if !tx
        .active_job(claim.job_id, claim.generation, claim.run_token)
        .await?
    {
        return Err(AppError::Conflict("stale or cancelled job".into()));
    }
    tx.valid_targets(claim.asset, claim.source).await?;
    tx.guard_learning_publication(claim.asset, claim.source)
        .await?;
    if let JobPayload::SemanticPublish {
        parser,
        semantic_members,
        ..
    } = payload
    {
        if parser != crate::memory_semantics::PARSER {
            return Err(AppError::Invalid("unsupported semantic parser".into()));
        }
        for member in semantic_members {
            tx.require_learning_source_visible(member.source_event_id)
                .await?;
            let source = tx
                .event_content(member.source_event_id, true)
                .await?
                .ok_or_else(|| AppError::Conflict("semantic source unavailable".into()))?;
            if source.get(member.statement.byte_start..member.statement.byte_end)
                != Some(member.statement.quote.as_str())
            {
                return Err(AppError::Invalid("invalid semantic provenance".into()));
            }
        }
    }
    if payload.kind() == JobKind::SemanticExtract {
        semantic_capture::guard_capture(tx, claim, payload).await?;
    }
    if payload.kind() == JobKind::Improve {
        improve::guard_improve(tx, claim, payload).await?;
    }
    if !matches!(
        payload.kind(),
        JobKind::Extract | JobKind::SemanticExtract | JobKind::Improve | JobKind::ConsolidateMemory
    ) {
        let current = tx
            .asset_current_version(claim.asset.ok_or(AppError::NotFound)?)
            .await?;
        if current != payload.expected_version() {
            return Err(AppError::Conflict(
                "publication superseded by another version".into(),
            ));
        }
    }
    Ok(())
}

/// One serialized worker owns all native writes and cleanup. Graceful shutdown
/// finishes its current job; a hard exit is recovered from the saved plan.
pub async fn run(
    service: Service,
    mut stop: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<()> {
    if service.worker_running.swap(true, Ordering::AcqRel) {
        anyhow::bail!("worker already running");
    }
    struct Reset(std::sync::Arc<std::sync::atomic::AtomicBool>);
    impl Drop for Reset {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Release);
        }
    }
    let _reset = Reset(service.worker_running.clone());
    let mut next_archive_cleanup = tokio::time::Instant::now();
    while !*stop.borrow() {
        if tokio::time::Instant::now() >= next_archive_cleanup {
            if let Err(error) = cleanup_private_archives(&service).await {
                tracing::warn!(code = error.code(), "private archive cleanup will retry");
            }
            next_archive_cleanup = tokio::time::Instant::now() + Duration::from_secs(1);
        }
        if !process_next(&service).await? {
            tokio::select! {_ = tokio::time::sleep(Duration::from_millis(100))=>{}, _ = stop.changed()=>{}}
        }
    }
    Ok(())
}
pub async fn process_next(service: &Service) -> Result<bool> {
    // Preserve the single serialized worker while keeping publication futures
    // out of every host's surrounding async state and debug stack frame.
    Box::pin(process_next_inner(service)).await
}

#[cfg(test)]
pub(crate) fn unboxed_process_next_future_size(service: &Service) -> usize {
    fn size<F: std::future::Future>(_: impl FnOnce() -> F) -> usize {
        std::mem::size_of::<F>()
    }
    size(|| process_next_inner(service))
}

async fn process_next_inner(service: &Service) -> Result<bool> {
    let Some(mut claim) = service.engine.queue().claim_next().await? else {
        return Ok(false);
    };
    let result = process_claim(service, &mut claim).await;
    if let Err(error) = result {
        let retry = matches!(
            error,
            AppError::Database(_) | AppError::Internal(_) | AppError::RetryableUnavailable(_)
        );
        let state = if matches!(error, AppError::Conflict(_)) {
            "superseded"
        } else {
            "failed"
        };
        service
            .engine
            .relational()
            .fail_job(&claim, state, error.code(), retry)
            .await?;
        tracing::warn!(job_id=%claim.job_id,code=error.code(),"job did not publish");
        cleanup(service).await?;
    }
    Ok(true)
}
async fn process_claim(service: &Service, claim: &mut ClaimedJob) -> Result<()> {
    // Decode within the claim settlement path so invalid jobs fail normally and
    // cannot strand the worker or bypass operation-specific validation.
    let payload = JobPayload::decode(&claim.kind, &claim.payload)?;
    let payload = &payload;
    if payload.kind() == JobKind::Cleanup {
        cleanup(service).await?;
        service
            .engine
            .queue()
            .settle(claim.clone(), JobFinish::Completed)
            .await?;
        return Ok(());
    }
    let a = auth(claim);
    let mut tx = service.read(&a, permission(payload)).await?;
    guard(&mut tx, claim, payload).await?;
    let profile = payload.embedding_profile().map(str::to_owned);
    if profile.is_some() && profile != service.models.profile {
        return Err(AppError::Unavailable(
            "worker embedding profile differs from accepted job".into(),
        ));
    }
    if let JobPayload::MessageBatchIngest { .. } = payload {
        let source = tx
            .event_content(claim.source.ok_or(AppError::NotFound)?, true)
            .await?
            .ok_or(AppError::NotFound)?;
        tx.commit().await?;
        // Move only the generic batch's saved plan into the blocking parser.
        // Guards/settlement use claim identity, generation, asset and source;
        // none of them reread this in-memory plan. The durable envelope stays
        // untouched until the ordinary save_publication transaction succeeds.
        let saved = claim
            .payload
            .as_object_mut()
            .and_then(|fields| fields.remove("publication"));
        return process_message_batch(service, claim, payload, source, saved).await;
    }
    if payload.kind() == JobKind::SemanticExtract {
        tx.commit().await?;
        return semantic_capture::process(service, claim, payload).await;
    }
    if payload.kind() == JobKind::Improve {
        tx.commit().await?;
        return improve::process(service, claim, payload).await;
    }
    if matches!(payload, JobPayload::ConsolidateMemory { .. }) {
        return Box::pin(consolidation::process(service, claim, payload, tx)).await;
    }
    if payload.kind() == JobKind::Extract {
        // A saved plan replays without re-calling the extraction model.
        if let Some(saved) = claim
            .payload
            .get("publication")
            .filter(|p| p["generation"] == claim.generation)
        {
            let p: Publication = decode(saved.clone())?;
            tx.commit().await?;
            return publish_prepared(service, claim, payload, p).await;
        }
        let text = tx
            .event_content(claim.source.ok_or(AppError::NotFound)?, true)
            .await?
            .ok_or(AppError::NotFound)?;
        tx.commit().await?;
        let drafts = if let JobPayload::IdentifiedExtract { identity, .. } = payload {
            let mut chunks = service.models.extract_identified(&text, identity).await?;
            let mut tx = service.read(&a, Permission::Write).await?;
            guard(&mut tx, claim, payload).await?;
            let asset = claim.asset.ok_or(AppError::NotFound)?;
            let binding = tx.identity_for_asset(asset).await?;
            tx.commit().await?;
            if binding.as_ref().is_some_and(|binding| binding != identity) {
                return Err(AppError::Conflict(
                    "identified capture asset binding changed".into(),
                ));
            }
            if binding.is_none() {
                for chunk in &mut chunks {
                    chunk.locator["suggested_identity"] = json!(identity);
                }
            }
            if chunks.is_empty() {
                Vec::new()
            } else {
                vec![DraftMemory {
                    asset,
                    fact_key: None,
                    expected_version: None,
                    chunks,
                }]
            }
        } else {
            let memories = service.models.extract(&text).await?;
            let mut tx = service.write(&a, Permission::Write).await?;
            guard(&mut tx, claim, payload).await?;
            let mut drafts = Vec::new();
            let mut seen =
                std::collections::HashMap::<&str, std::collections::HashSet<&str>>::new();
            for m in &memories {
                let facts = seen.entry(&m.fact_key).or_default();
                let first = facts.is_empty();
                if !first_extracted_fact(facts, &m.content, claim.job_id) {
                    continue;
                }
                let mut chunks = parsing::chunks(&m.content, "text")?;
                let slot = if first {
                    Some(tx.slot(&m.fact_key).await?)
                } else {
                    None
                };
                let (asset, fact_key) = match slot {
                    Some((asset, None)) => (asset, Some(m.fact_key.clone())),
                    _ => {
                        for chunk in &mut chunks {
                            chunk.locator["suggested_fact_key"] = json!(m.fact_key);
                        }
                        let id = graph::entity_id(&format!(
                            "capture-evidence:{}:{}",
                            claim.job_id,
                            drafts.len()
                        ));
                        (tx.reserve_capture_evidence_slot(id).await?, None)
                    }
                };
                drafts.push(DraftMemory {
                    asset,
                    fact_key,
                    expected_version: None,
                    chunks,
                });
            }
            tx.commit().await?;
            drafts
        };
        let publication = prepare(service, claim, payload, profile, drafts).await?;
        return publish_prepared(service, claim, payload, publication).await;
    }
    let learning_metadata = tx
        .learning_publication_metadata(claim.asset.ok_or(AppError::NotFound)?)
        .await?;
    let saved = claim
        .payload
        .get("publication")
        .filter(|p| p["generation"] == claim.generation || learning_metadata.is_some());
    let publication = if let Some(saved) = saved {
        let mut p: Publication = decode(saved.clone())?;
        if let Some(metadata) = learning_metadata {
            // Learning publication is immutable and retries keep the accepted
            // model plan. Explicit retry advances the queue generation, so
            // rebind only native object IDs/ledger ownership to that generation.
            // Other operations deliberately retain their existing replay rules.
            if p.profile != profile
                || p.graph.is_some()
                || p.memories.len() != 1
                || p.memories[0].asset != claim.asset.ok_or(AppError::NotFound)?
                || p.memories[0].expected_version != payload.expected_version()
                || p.memories[0].chunks.is_empty()
                || p.memories[0]
                    .chunks
                    .iter()
                    .any(|c| c.chunk.locator["learning"] != metadata || !c.summary.is_empty())
            {
                return Err(AppError::Invalid(
                    "invalid learning publication plan".into(),
                ));
            }
            if p.generation != claim.generation {
                p.generation = claim.generation;
                for (memory_index, memory) in p.memories.iter_mut().enumerate() {
                    for (ordinal, chunk) in memory.chunks.iter_mut().enumerate() {
                        chunk.id = graph::entity_id(&format!(
                            "chunk:{}:{}:{}:{}",
                            claim.job_id, claim.generation, memory_index, ordinal
                        ));
                    }
                }
            }
        }
        tx.commit().await?;
        p
    } else {
        let expected = payload.expected_version();
        let asset = claim.asset.ok_or(AppError::NotFound)?;
        let chunks = match payload {
            JobPayload::SemanticPublish {
                semantic_members: members,
                suggested_identity: identity,
                semantic_status: status,
                ..
            } => {
                let chunks = crate::memory_semantics::chunks(
                    members,
                    status,
                    if status == "confirmed" {
                        None
                    } else {
                        identity.as_ref()
                    },
                );
                tx.commit().await?;
                chunks
            }
            JobPayload::Publish { .. } => {
                let text = tx
                    .event_content(claim.source.ok_or(AppError::NotFound)?, true)
                    .await?
                    .ok_or(AppError::NotFound)?;
                let chunks = parsing::chunks(&text, "text")?;
                tx.commit().await?;
                chunks
            }
            JobPayload::Restore { target_version, .. } => {
                let references = tx.chunk_refs_for_version(asset, *target_version).await?;
                tx.commit().await?;
                service.read_chunk_content(claim.scope, &references).await?
            }
            JobPayload::Ingest { format, .. } => {
                let file = tx
                    .event_file(claim.source.ok_or(AppError::NotFound)?)
                    .await?;
                tx.commit().await?;
                if let Some(id) = file["file_id"].as_str() {
                    if file["media_type"] != format.as_str() {
                        return Err(AppError::Invalid(
                            "file format does not match ingest request".into(),
                        ));
                    }
                    service
                        .file(&a, Uuid::parse_str(id).map_err(anyhow::Error::from)?)
                        .await?;
                    let key =
                        Service::blob_key(&a, file["hash"].as_str().ok_or(AppError::NotFound)?);
                    parsing::parse_file(&service.engine.blobs().path_for(&key)?, format).await?
                } else {
                    parsing::chunks(file["content"].as_str().unwrap_or(""), format)?
                }
            }
            JobPayload::LegacyExtract { .. }
            | JobPayload::MessageBatchIngest { .. }
            | JobPayload::IdentifiedExtract { .. }
            | JobPayload::SemanticExtract { .. }
            | JobPayload::Improve { .. }
            | JobPayload::ConsolidateMemory { .. }
            | JobPayload::Cleanup { .. } => {
                return Err(AppError::Invalid("invalid publication operation".into()));
            }
        };
        prepare(
            service,
            claim,
            payload,
            profile,
            vec![DraftMemory {
                asset,
                fact_key: None,
                expected_version: expected,
                chunks,
            }],
        )
        .await?
    };
    publish_prepared(service, claim, payload, publication).await
}
/// Generic batches use the same prepare/publish and native ledger as every
/// other durable publication. Reparse even on recovery to validate the accepted
/// resource limits and exact provenance outside the async executor and any tx.
async fn process_message_batch(
    service: &Service,
    claim: &ClaimedJob,
    payload: &JobPayload,
    source: String,
    saved: Option<Value>,
) -> Result<()> {
    let JobPayload::MessageBatchIngest {
        accepted_spec,
        expected_version,
    } = payload
    else {
        return Err(AppError::Invalid("invalid message batch operation".into()));
    };
    let spec = accepted_spec.clone();
    let source_event = claim.source.ok_or(AppError::NotFound)?;
    let asset = claim.asset.ok_or(AppError::NotFound)?;
    let expected = *expected_version;
    let generation = claim.generation;
    let job_id = claim.job_id;
    let (chunks, saved) = tokio::task::spawn_blocking(move || -> Result<_> {
        let chunks = crate::messages::chunks(&source, &spec, source_event)?;
        let saved = if let Some(saved) = saved.filter(|plan| plan["generation"] == generation) {
            let publication: Publication = decode(saved)?;
            if publication.generation != generation
                || publication.profile.as_deref() != Some(spec.embedding_profile.as_str())
                || publication.graph.is_some()
                || publication.memories.len() != 1
            {
                return Err(AppError::Invalid("invalid message publication plan".into()));
            }
            let memory = &publication.memories[0];
            if (!memory.chunks.is_empty() && publication.receipt_metadata.is_none())
                || memory.asset != asset
                || memory.expected_version != expected
                || memory.version != expected.unwrap_or(0) + 1
                || memory.fact_key.is_some()
                || memory.chunks.len() != chunks.len()
                || memory
                    .chunks
                    .iter()
                    .zip(&chunks)
                    .enumerate()
                    .any(|(index, (saved, source))| {
                        saved.id
                            != graph::entity_id(&format!("chunk:{job_id}:{generation}:0:{index}"))
                            || saved.chunk.content != source.content
                            || saved.chunk.locator != source.locator
                            || !saved.summary.is_empty()
                            || saved.embedding.as_ref().is_none_or(|vector| {
                                vector.is_empty()
                                    || vector.len() > 4096
                                    || vector.iter().any(|x| !x.is_finite())
                            })
                    })
            {
                return Err(AppError::Invalid("invalid message publication plan".into()));
            }
            Some(publication)
        } else {
            None
        };
        Ok((chunks, saved))
    })
    .await
    .map_err(|error| AppError::Internal(error.into()))??;
    let publication = match saved {
        Some(publication) => publication,
        None => {
            prepare(
                service,
                claim,
                payload,
                Some(accepted_spec.embedding_profile.clone()),
                vec![DraftMemory {
                    asset,
                    fact_key: None,
                    expected_version: expected,
                    chunks,
                }],
            )
            .await?
        }
    };
    publish_prepared(service, claim, payload, publication).await
}

async fn prepare(
    service: &Service,
    claim: &ClaimedJob,
    payload: &JobPayload,
    profile: Option<String>,
    drafts: Vec<DraftMemory>,
) -> Result<Publication> {
    if drafts.iter().any(|d| d.chunks.is_empty()) {
        return Err(AppError::Conflict("no source chunks available".into()));
    }
    // Applicability must survive normal knowledge chunking. Every Learning
    // chunk keeps the same immutable conditions/provenance, including chunks
    // that do not contain the source's leading conditions text.
    let mut tx = service.read(&auth(claim), permission(payload)).await?;
    guard(&mut tx, claim, payload).await?;
    let mut learning_metadata = Vec::with_capacity(drafts.len());
    for draft in &drafts {
        learning_metadata.push(tx.learning_publication_metadata(draft.asset).await?);
    }
    tx.commit().await?;
    let has_learning = learning_metadata.iter().any(Option::is_some);
    let mut memories = Vec::new();
    for (memory_index, draft) in drafts.into_iter().enumerate() {
        let mut out = Vec::new();
        for (ordinal, mut chunk) in draft.chunks.into_iter().enumerate() {
            if let Some(metadata) = &learning_metadata[memory_index] {
                chunk
                    .locator
                    .as_object_mut()
                    .ok_or_else(|| AppError::Invalid("invalid learning locator".into()))?
                    .insert("learning".into(), metadata.clone());
            }
            let embedding = if profile.is_some() {
                Some(service.models.embed(&chunk.content).await?)
            } else {
                None
            };
            let summary = if payload.kind() == JobKind::Ingest
                && !has_learning
                && service.models.extraction_enabled()
            {
                service.models.summarize(&chunk.content).await?
            } else {
                String::new()
            };
            let id = graph::entity_id(&format!(
                "chunk:{}:{}:{}:{}",
                claim.job_id, claim.generation, memory_index, ordinal
            ));
            out.push(PublishedChunk {
                id,
                chunk,
                embedding,
                summary,
                search_terms: String::new(),
                summary_terms: String::new(),
            });
        }
        memories.push(PublishedMemory {
            asset: draft.asset,
            fact_key: draft.fact_key,
            expected_version: draft.expected_version,
            version: draft.expected_version.unwrap_or(0) + 1,
            chunks: out,
        });
    }
    let text = memories
        .iter()
        .flat_map(|m| m.chunks.iter())
        .map(|c| c.chunk.content.as_str())
        .collect::<Vec<_>>()
        .join("");
    let graph = if payload.kind() == JobKind::Ingest
        && !has_learning
        && service.models.extraction_enabled()
    {
        Some(service.models.extract_graph(&text).await?)
    } else {
        None
    };
    // Freeze both indexes once, outside the executor and every SQL transaction.
    let memories = tokio::task::spawn_blocking(move || {
        for memory in &mut memories {
            for chunk in &mut memory.chunks {
                chunk.search_terms = parsing::lexical(&chunk.chunk.content);
                chunk.summary_terms = parsing::lexical(&chunk.summary);
            }
        }
        memories
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(Publication {
        generation: claim.generation,
        profile,
        summary_model: service.models.summary_model().map(str::to_owned),
        graph,
        memories,
        receipt_metadata: None,
        full_text_operation_epoch: None,
    })
}
fn source_for(claim: &ClaimedJob, version: i32) -> Result<SourceVersion> {
    Ok(SourceVersion {
        source_id: claim.source.ok_or(AppError::NotFound)?,
        version,
    })
}
fn ledger(claim: &ClaimedJob, publication: &Publication) -> Result<Vec<LedgerEntry>> {
    fn push(
        items: &mut Vec<LedgerEntry>,
        claim: &ClaimedJob,
        source: SourceVersion,
        artifact_id: Uuid,
        artifact_type: &str,
        surface: Surface,
    ) {
        let key = LedgerKey {
            scope: claim.scope,
            source,
            artifact_id,
            surface,
            generation: claim.generation,
        };
        items.push(LedgerEntry {
            key,
            artifact_type: artifact_type.into(),
        });
    }
    let mut items = Vec::new();
    for memory in &publication.memories {
        let source = source_for(claim, memory.version)?;
        for chunk in &memory.chunks {
            push(
                &mut items,
                claim,
                source,
                chunk.id,
                "chunk",
                Surface::FullText,
            );
            if chunk.embedding.is_some() {
                push(
                    &mut items,
                    claim,
                    source,
                    chunk.id,
                    "chunk",
                    Surface::Vector,
                );
            }
        }
    }
    // Only knowledge ingest produces a graph, and ingest is single-memory.
    if let (Some(g), Some(first)) = (&publication.graph, publication.memories.first()) {
        let source = source_for(claim, first.version)?;
        for e in &g.entities {
            push(
                &mut items,
                claim,
                source,
                graph::entity_id(&e.name),
                "entity",
                Surface::Graph,
            );
        }
        for r in &g.relations {
            push(
                &mut items,
                claim,
                source,
                graph::relation_id(
                    graph::entity_id(&r.source),
                    &r.predicate,
                    graph::entity_id(&r.target),
                ),
                "relation",
                Surface::Graph,
            );
        }
    }
    Ok(items)
}
/// Graph support belongs to the same frozen source/version/generation as the
/// chunk projection. Never manufacture a new source or graph on a retry.
fn graph_publication(
    claim: &ClaimedJob,
    kind: JobKind,
    publication: &Publication,
) -> Result<Option<crate::storage::anyseek::GraphPublication>> {
    let Some(graph) = &publication.graph else {
        return Ok(None);
    };
    if kind != JobKind::Ingest
        || publication.generation != claim.generation
        || publication.memories.len() != 1
        || publication.memories[0].chunks.is_empty()
        || publication.memories[0].version <= 0
    {
        return Err(AppError::Invalid(
            "invalid graph publication ownership".into(),
        ));
    }
    // The graph is stored on an existing chunk, not as additional group members.
    if publication.memories[0].chunks.len() > crate::storage::anyseek::MAX_GROUP {
        return Err(AppError::Invalid(
            "AnySeek graph publication exceeds 64 chunks".into(),
        ));
    }
    Ok(Some(crate::storage::anyseek::GraphPublication {
        source: source_for(claim, publication.memories[0].version)?,
        generation: claim.generation,
        graph: graph.clone(),
    }))
}

async fn publish_prepared(
    service: &Service,
    claim: &ClaimedJob,
    payload: &JobPayload,
    mut publication: Publication,
) -> Result<()> {
    if payload.kind() == JobKind::Extract
        && (payload.expected_version().is_some()
            || publication
                .memories
                .iter()
                .any(|memory| memory.expected_version.is_some() || memory.version != 1))
    {
        return Err(AppError::Conflict(
            "extraction cannot replace a published memory".into(),
        ));
    }
    if publication.full_text_operation_epoch.is_none() {
        publication.full_text_operation_epoch = Some(crate::storage::anyseek::operation_epoch()?);
    }
    let graph_projection = graph_publication(claim, payload.kind(), &publication)?;
    let a = auth(claim);
    let entries = ledger(claim, &publication)?;
    let mut tx = service.write(&a, permission(payload)).await?;
    guard(&mut tx, claim, payload).await?;
    tx.save_publication(claim.job_id, &json!(publication))
        .await?;
    for entry in &entries {
        tx.register_pending(entry.clone()).await?;
        tx.resume_pending(entry.key).await?;
    }
    tx.commit().await?;
    let mut vectors = Vec::new();
    let mut full_text = Vec::new();
    for memory in &publication.memories {
        let source = source_for(claim, memory.version)?;
        for c in &memory.chunks {
            full_text.push(FullTextEntry {
                id: c.id,
                source,
                generation: claim.generation,
                payload: ChunkPayload {
                    content: c.chunk.content.clone(),
                    locator: c.chunk.locator.clone(),
                    summary: (!c.summary.is_empty()).then(|| ChunkSummary {
                        text: c.summary.clone(),
                        model_revision: publication.summary_model.clone().unwrap_or_default(),
                    }),
                },
                search_terms: c.search_terms.clone(),
            });
            if let Some(embedding) = &c.embedding {
                vectors.push(VectorEntry {
                    id: c.id,
                    embedding: embedding.clone(),
                    profile: publication.profile.clone().unwrap_or_default(),
                    dimension: embedding.len(),
                    generation: claim.generation,
                    source,
                });
            }
        }
    }
    service
        .engine
        .vector()
        .upsert_publication(
            claim.scope,
            full_text,
            vectors,
            graph_projection,
            publication.full_text_operation_epoch,
        )
        .await?;
    // The graph projection was written in the same AnySeek group as chunks.
    // Only the final authority transaction may make those supports visible.
    let mut tx = service.write(&a, permission(payload)).await?;
    guard(&mut tx, claim, payload).await?;
    for memory in &publication.memories {
        let current = tx.asset_current_version(memory.asset).await?;
        if current != memory.expected_version {
            return Err(AppError::Conflict(
                "publication superseded by another version".into(),
            ));
        }
        let source = source_for(claim, memory.version)?;
        let content = memory
            .chunks
            .iter()
            .map(|c| c.chunk.content.as_str())
            .collect::<Vec<_>>()
            .join("");
        tx.insert_version(
            memory.asset,
            memory.version,
            &content,
            source.source_id,
            payload.target_version(),
            payload.title(),
        )
        .await?;
        for (ordinal, c) in memory.chunks.iter().enumerate() {
            tx.insert_chunk(
                c.id,
                memory.asset,
                memory.version,
                ordinal as i32,
                &c.chunk.content,
                &c.chunk.locator,
            )
            .await?;
            tx.register_owner(source, "chunk", c.id, Some(c.id)).await?;
            if !c.summary.is_empty() {
                let id = graph::entity_id(&format!("summary:{}", c.id));
                tx.insert_summary(id, c.id, &c.summary_terms).await?;
                tx.register_owner(source, "summary", id, Some(c.id)).await?;
            }
            if let Some(e) = &c.embedding {
                tx.index_ready(
                    c.id,
                    publication.profile.as_deref().unwrap_or(""),
                    e.len(),
                    claim.generation,
                )
                .await?;
            }
        }
        tx.update_asset_version(memory.asset, memory.version, payload.title())
            .await?;
    }
    for entry in entries {
        if entry.key.surface == Surface::Graph {
            tx.register_owner(
                entry.key.source,
                &entry.artifact_type,
                entry.key.artifact_id,
                None,
            )
            .await?;
        }
        tx.confirm_committed(entry.key).await?;
    }
    for memory in &publication.memories {
        tx.audit(
            "asset.published",
            memory.asset,
            json!({"version":memory.version,"source_event_id":claim.source,"job_id":claim.job_id}),
        )
        .await?;
    }
    let mut capabilities = vec!["keyword"];
    if publication.profile.is_some() {
        capabilities.push("vector");
    }
    if publication
        .memories
        .iter()
        .any(|m| m.chunks.iter().any(|c| !c.summary.is_empty()))
    {
        capabilities.push("summary");
    }
    if publication.graph.is_some() {
        capabilities.push("graph");
    }
    let mut result = json!({"readiness":"ready","index_capabilities":capabilities});
    if payload.kind() == JobKind::SemanticExtract {
        result["memories"] = json!(semantic_capture::results(claim)?);
        if let JobPayload::SemanticExtract { extractor, .. } = payload {
            result["extractor"] = json!(extractor);
        }
    } else if payload.kind() == JobKind::Extract {
        let identity = if matches!(payload, JobPayload::IdentifiedExtract { .. }) {
            tx.identity_for_asset(claim.asset.ok_or(AppError::NotFound)?)
                .await?
        } else {
            None
        };
        result["memories"] = json!(
            publication
                .memories
                .iter()
                .map(|m| json!({
                    "asset_id": m.asset,
                    "fact_key": m.fact_key,
                    "identity": identity,
                    "version": m.version,
                }))
                .collect::<Vec<_>>()
        );
    } else {
        let first = publication
            .memories
            .first()
            .ok_or(AppError::Conflict("publication without memories".into()))?;
        result["asset_id"] = json!(first.asset);
        result["version"] = json!(first.version);
    }
    tx.settle_completed(claim.job_id, "published", &result)
        .await?;
    for memory in &publication.memories {
        tx.mark_learning_published(memory.asset).await?;
    }
    tx.commit().await?;
    Ok(())
}
async fn cleanup_private_archives(service: &Service) -> Result<()> {
    let store = service.engine.relational();
    let backend = service.engine.vector().private_archive();
    let mut result = Ok(());
    for entry in store.withdrawn_archives(16).await? {
        match backend.archive_delete(&entry.locator).await {
            Ok(()) => store.finish_archive_cleanup(&entry.locator).await?,
            Err(error) => {
                store.defer_archive_cleanup(&entry.locator).await?;
                result = Err(error.into());
            }
        }
    }
    for (domain, session, turn) in store.withdrawn_native_archive_turns(16).await? {
        let deleted = backend.native_archive_delete(domain, session, turn).await;
        store
            .settle_native_turn_cleanup(domain.scope, session, turn, deleted.is_ok())
            .await?;
        if let Err(error) = deleted {
            result = Err(error.into());
        }
    }
    for (domain, session) in store.withdrawn_native_archive_sessions(2).await? {
        let mut removed = false;
        match backend.native_archive_page(domain, session, 0, 8).await {
            Ok((_, turns)) => {
                for turn in turns {
                    match backend
                        .native_archive_delete(domain, session, turn.turn)
                        .await
                    {
                        Ok(()) => removed = true,
                        Err(error) => result = Err(error.into()),
                    }
                }
            }
            Err(error) => result = Err(error.into()),
        }
        // Keep sweeping withdrawn sessions so delayed writes that were
        // allocated before the withdrawal cannot escape physical cleanup.
        store
            .reschedule_native_session_cleanup(domain.scope, session, removed)
            .await?;
    }
    result
}

/// Runs only before serving requests or on the single worker, never concurrently
/// with native writes. Visibility is already blocked by the relational tombstone.
pub async fn cleanup(service: &Service) -> Result<()> {
    for scope in service.engine.relational().scopes().await? {
        let chunks = service.engine.relational().cleanup_plan(scope).await?;
        // Graph metadata follows its source publication group. Do not delete
        // entity/edge IDs globally: another source may still support them.
        service
            .engine
            .vector()
            .delete_objects(scope, chunks)
            .await?;
        service.engine.relational().finish_cleanup(scope).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct LogBuffer(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for LogBuffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn publication_roundtrip_keeps_receipt_metadata() {
        let metadata = json!({"format":"message-receipt:v1","chunks":[
            {"id_json":"\"chunk\"","dimension":4096},
            {"id_json":"null","dimension":null}
        ]});
        let value = json!({"generation":3,"profile":"fixture","summary_model":null,
            "graph":null,"memories":[],"receipt_metadata":metadata});
        let publication: Publication = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(publication).unwrap(), value);
        let mut unprepared = value;
        unprepared
            .as_object_mut()
            .unwrap()
            .remove("receipt_metadata");
        let publication: Publication = serde_json::from_value(unprepared.clone()).unwrap();
        assert!(publication.receipt_metadata.is_none());
        assert_eq!(serde_json::to_value(publication).unwrap(), unprepared);
    }

    fn graph_plan_fixture(chunks: usize) -> (ClaimedJob, Publication) {
        let claim = ClaimedJob {
            scope: Scope {
                tenant_id: Uuid::new_v4(),
                workspace_id: Uuid::new_v4(),
            },
            job_id: Uuid::new_v4(),
            kind: "ingest".into(),
            payload: json!({}),
            generation: 3,
            run_token: 1,
            created_by: Uuid::new_v4(),
            asset: Some(Uuid::new_v4()),
            source: Some(Uuid::new_v4()),
        };
        let graph = graph::parse_graph_extraction(&json!({
            "entities":[{"name":"alpha","entity_type":"component","description":""},
                        {"name":"beta","entity_type":"component","description":""}],
            "relations":[{"source":"alpha","predicate":"uses","target":"beta"}]
        }))
        .unwrap();
        let publication = Publication {
            generation: 3,
            profile: None,
            summary_model: None,
            graph: Some(graph),
            memories: vec![PublishedMemory {
                asset: claim.asset.unwrap(),
                fact_key: None,
                expected_version: None,
                version: 1,
                chunks: (0..chunks)
                    .map(|n| PublishedChunk {
                        id: Uuid::from_u128(n as u128 + 1),
                        chunk: Chunk {
                            content: "alpha uses beta".into(),
                            locator: json!({}),
                        },
                        embedding: None,
                        summary: String::new(),
                        search_terms: "alpha uses beta".into(),
                        summary_terms: String::new(),
                    })
                    .collect(),
            }],
            receipt_metadata: None,
            full_text_operation_epoch: Some(7),
        };
        (claim, publication)
    }

    #[test]
    fn graph_publication_preserves_saved_source_and_retry_plan() {
        let (claim, publication) = graph_plan_fixture(1);
        let saved = serde_json::to_value(&publication).unwrap();
        let replay: Publication = serde_json::from_value(saved.clone()).unwrap();
        let projection = graph_publication(&claim, JobKind::Ingest, &replay)
            .unwrap()
            .unwrap();
        assert_eq!(projection.source, source_for(&claim, 1).unwrap());
        assert_eq!(projection.generation, 3);
        assert_eq!(Some(projection.graph), replay.graph);
        assert_eq!(serde_json::to_value(&replay).unwrap(), saved);
        assert_eq!(replay.full_text_operation_epoch, Some(7));
        let entries = ledger(&claim, &replay).unwrap();
        assert_eq!(entries.len(), 4);
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.key.surface == Surface::Graph)
                .count(),
            3
        );
    }

    #[test]
    fn graph_publication_rejects_ambiguous_owner_and_chunk_overflow() {
        let (claim, mut publication) = graph_plan_fixture(64);
        assert!(matches!(
            graph_publication(&claim, JobKind::Publish, &publication),
            Err(AppError::Invalid(_))
        ));
        assert!(graph_publication(&claim, JobKind::Ingest, &publication).is_ok());
        let extra = publication.memories[0].chunks[0].clone();
        publication.memories[0].chunks.push(extra);
        assert!(matches!(
            graph_publication(&claim, JobKind::Ingest, &publication),
            Err(AppError::Invalid(_))
        ));
        publication.memories[0].chunks.truncate(1);
        publication.generation += 1;
        assert!(matches!(
            graph_publication(&claim, JobKind::Ingest, &publication),
            Err(AppError::Invalid(_))
        ));
        publication.generation = claim.generation;
        publication.memories.push(publication.memories[0].clone());
        assert!(matches!(
            graph_publication(&claim, JobKind::Ingest, &publication),
            Err(AppError::Invalid(_))
        ));
        publication.memories.truncate(1);
        publication.memories[0].chunks.clear();
        assert!(matches!(
            graph_publication(&claim, JobKind::Ingest, &publication),
            Err(AppError::Invalid(_))
        ));
        publication.graph = None;
        assert!(
            graph_publication(&claim, JobKind::Ingest, &publication)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn duplicate_extracted_fact_log_omits_sensitive_key() {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let writer = LogBuffer(bytes.clone());
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let job_id = Uuid::new_v4();
        let sensitive_key = "private-sentinel-user@example.test";
        tracing::subscriber::with_default(subscriber, || {
            let mut seen = std::collections::HashSet::new();
            assert!(first_extracted_fact(&mut seen, sensitive_key, job_id));
            assert!(bytes.lock().unwrap().is_empty());
            assert!(!first_extracted_fact(&mut seen, sensitive_key, job_id));
            assert_eq!(seen.len(), 1);
        });
        let logs = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
        assert!(logs.contains("duplicate extracted fact dropped"));
        assert!(logs.contains(&job_id.to_string()));
        assert!(!logs.contains(sensitive_key));
    }
}
