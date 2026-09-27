use crate::{
    error::{AppError, Result},
    graph, parsing,
    service::{Service, decode},
    storage::{sqlite::SqliteTx, *},
    types::{AuthContext, Chunk, GraphExtraction},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{sync::atomic::Ordering, time::Duration};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
struct PublishedChunk {
    id: Uuid,
    chunk: Chunk,
    embedding: Option<Vec<f32>>,
    summary: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Publication {
    generation: i64,
    version: i32,
    profile: Option<String>,
    #[serde(default)]
    summary_model: Option<String>,
    chunks: Vec<PublishedChunk>,
    graph: Option<GraphExtraction>,
}
fn auth(claim: &ClaimedJob) -> AuthContext {
    AuthContext {
        id: claim.created_by,
        tenant_id: claim.scope.tenant_id,
        workspace_id: claim.scope.workspace_id,
        role: String::new(),
    }
}
fn permission(claim: &ClaimedJob) -> Permission {
    if matches!(claim.kind.as_str(), "publish" | "restore") {
        Permission::Publish
    } else {
        Permission::Write
    }
}
async fn guard(tx: &mut SqliteTx, claim: &ClaimedJob) -> Result<()> {
    if !tx
        .active_job(claim.job_id, claim.generation, claim.run_token)
        .await?
    {
        return Err(AppError::Conflict("stale or cancelled job".into()));
    }
    tx.valid_targets(claim.asset, claim.source).await?;
    if claim.kind != "extract" {
        let current = tx
            .asset_current_version(claim.asset.ok_or(AppError::NotFound)?)
            .await?;
        if json!(current) != claim.payload["expected_version"] {
            return Err(AppError::Conflict(
                "publication superseded by another version".into(),
            ));
        }
    }
    if claim.kind == "publish" {
        let candidate = tx
            .candidate_for_review(decode(claim.payload["candidate_id"].clone())?)
            .await?;
        if candidate["state"] != "approved"
            || candidate["expected_version"] != claim.payload["expected_version"]
        {
            return Err(AppError::Conflict("candidate is no longer approved".into()));
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
    while !*stop.borrow() {
        if !process_next(&service).await? {
            tokio::select! {_ = tokio::time::sleep(Duration::from_millis(100))=>{}, _ = stop.changed()=>{}}
        }
    }
    Ok(())
}
pub async fn process_next(service: &Service) -> Result<bool> {
    let Some(claim) = service.engine.queue().claim_next().await? else {
        return Ok(false);
    };
    let result = process_claim(service, &claim).await;
    if let Err(error) = result {
        let retry = matches!(error, AppError::Database(_) | AppError::Internal(_));
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
async fn process_claim(service: &Service, claim: &ClaimedJob) -> Result<()> {
    if claim.payload["schema_version"] != 1 {
        return Err(AppError::Invalid("unsupported job payload schema".into()));
    }
    if claim.kind == "cleanup" {
        cleanup(service).await?;
        service
            .engine
            .queue()
            .settle(claim.clone(), JobFinish::Completed)
            .await?;
        return Ok(());
    }
    let a = auth(claim);
    let mut tx = service.read(&a, permission(claim)).await?;
    guard(&mut tx, claim).await?;
    if claim.kind == "extract" {
        let text = tx
            .event_content(claim.source.ok_or(AppError::NotFound)?, true)
            .await?
            .ok_or(AppError::NotFound)?;
        tx.commit().await?;
        let candidates = service.models.extract(&text).await?;
        let mut tx = service.write(&a, Permission::Write).await?;
        guard(&mut tx, claim).await?;
        let mut ids = Vec::new();
        for c in candidates {
            let (asset, version) = tx.slot(&c.fact_key).await?;
            ids.push(
                tx.insert_candidate(
                    asset,
                    &c.fact_key,
                    &c.content,
                    claim.source.unwrap(),
                    false,
                    version,
                )
                .await?,
            );
        }
        tx.settle_completed(
            claim.job_id,
            "candidates_created",
            &json!({"candidate_ids":ids}),
        )
        .await?;
        tx.commit().await?;
        return Ok(());
    }
    let profile: Option<String> = decode(claim.payload["embedding_profile"].clone())?;
    if profile.is_some() && profile != service.models.profile {
        return Err(AppError::Unavailable(
            "worker embedding profile differs from accepted job".into(),
        ));
    }
    let saved = claim
        .payload
        .get("publication")
        .filter(|p| p["generation"] == claim.generation);
    let publication = if let Some(saved) = saved {
        let p: Publication = decode(saved.clone())?;
        tx.commit().await?;
        p
    } else {
        let current = tx
            .asset_current_version(claim.asset.ok_or(AppError::NotFound)?)
            .await?;
        let chunks = match claim.kind.as_str() {
            "publish" => {
                let candidate = tx
                    .candidate_for_review(decode(claim.payload["candidate_id"].clone())?)
                    .await?;
                parsing::chunks(
                    candidate["content"].as_str().ok_or(AppError::NotFound)?,
                    "text",
                )?
            }
            "restore" => tx
                .chunks_for_version(
                    claim.asset.unwrap(),
                    decode(claim.payload["target_version"].clone())?,
                )
                .await?
                .into_iter()
                .map(|(content, locator)| Chunk { content, locator })
                .collect(),
            "ingest" => {
                let file = tx
                    .event_file(claim.source.ok_or(AppError::NotFound)?)
                    .await?;
                let format = claim.payload["format"]
                    .as_str()
                    .unwrap_or("text")
                    .to_string();
                tx.commit().await?;
                let chunks = if let Some(id) = file["file_id"].as_str() {
                    if file["media_type"] != format {
                        return Err(AppError::Invalid(
                            "file format does not match ingest request".into(),
                        ));
                    }
                    service
                        .file(&a, Uuid::parse_str(id).map_err(anyhow::Error::from)?)
                        .await?;
                    let key =
                        Service::blob_key(&a, file["hash"].as_str().ok_or(AppError::NotFound)?);
                    parsing::parse_file(&service.engine.blobs().path_for(&key)?, &format).await?
                } else {
                    parsing::chunks(file["content"].as_str().unwrap_or(""), &format)?
                };
                return publish_prepared(
                    service,
                    claim,
                    prepare(service, claim, current, profile, chunks).await?,
                )
                .await;
            }
            _ => return Err(AppError::Invalid("unknown job operation".into())),
        };
        tx.commit().await?;
        prepare(service, claim, current, profile, chunks).await?
    };
    publish_prepared(service, claim, publication).await
}
async fn prepare(
    service: &Service,
    claim: &ClaimedJob,
    current: Option<i32>,
    profile: Option<String>,
    chunks: Vec<Chunk>,
) -> Result<Publication> {
    if chunks.is_empty() {
        return Err(AppError::Conflict("no source chunks available".into()));
    }
    let text = chunks
        .iter()
        .map(|c| c.content.as_str())
        .collect::<Vec<_>>()
        .join("");
    let mut out = Vec::new();
    for (ordinal, chunk) in chunks.into_iter().enumerate() {
        let embedding = if profile.is_some() {
            Some(service.models.embed(&chunk.content).await?)
        } else {
            None
        };
        let summary = if claim.kind == "ingest" && service.models.extraction_enabled() {
            service.models.summarize(&chunk.content).await?
        } else {
            String::new()
        };
        let id = graph::entity_id(&format!(
            "chunk:{}:{}:{}",
            claim.job_id, claim.generation, ordinal
        ));
        out.push(PublishedChunk {
            id,
            chunk,
            embedding,
            summary,
        });
    }
    let graph = if claim.kind == "ingest" && service.models.extraction_enabled() {
        Some(service.models.extract_graph(&text).await?)
    } else {
        None
    };
    Ok(Publication {
        generation: claim.generation,
        version: current.unwrap_or(0) + 1,
        profile,
        summary_model: service.models.summary_model().map(str::to_owned),
        chunks: out,
        graph,
    })
}
fn ledger(claim: &ClaimedJob, publication: &Publication) -> Vec<LedgerEntry> {
    let mut items = Vec::new();
    let source = SourceVersion {
        source_id: claim.source.unwrap(),
        version: publication.version,
    };
    let mut push = |artifact_id, artifact_type: &str, surface| {
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
            idempotency_key: ledger_idempotency_key(&key),
        });
    };
    for chunk in &publication.chunks {
        if chunk.embedding.is_some() {
            push(chunk.id, "chunk", Surface::Vector);
        }
    }
    if let Some(g) = &publication.graph {
        for e in &g.entities {
            push(graph::entity_id(&e.name), "entity", Surface::Graph);
        }
        for r in &g.relations {
            push(
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
    items
}
async fn publish_prepared(
    service: &Service,
    claim: &ClaimedJob,
    publication: Publication,
) -> Result<()> {
    let a = auth(claim);
    let source = SourceVersion {
        source_id: claim.source.ok_or(AppError::NotFound)?,
        version: publication.version,
    };
    let entries = ledger(claim, &publication);
    let mut tx = service.write(&a, permission(claim)).await?;
    guard(&mut tx, claim).await?;
    tx.save_publication(claim.job_id, &json!(publication))
        .await?;
    for entry in &entries {
        tx.register_pending(entry.clone()).await?;
        tx.resume_pending(entry.key).await?;
    }
    tx.commit().await?;
    let vectors = publication
        .chunks
        .iter()
        .filter_map(|c| {
            c.embedding.as_ref().map(|embedding| VectorEntry {
                id: c.id,
                embedding: embedding.clone(),
                profile: publication.profile.clone().unwrap_or_default(),
                dimension: embedding.len(),
                generation: claim.generation,
                source,
            })
        })
        .collect();
    service.engine.vector().upsert(claim.scope, vectors).await?;
    if let Some(g) = &publication.graph {
        service
            .engine
            .graph()
            .upsert_entities(claim.scope, source, g.entities.clone())
            .await?;
        service
            .engine
            .graph()
            .upsert_relations(claim.scope, source, g.relations.clone())
            .await?;
    }
    let mut tx = service.write(&a, permission(claim)).await?;
    guard(&mut tx, claim).await?;
    let asset = claim.asset.ok_or(AppError::NotFound)?;
    let content = publication
        .chunks
        .iter()
        .map(|c| c.chunk.content.as_str())
        .collect::<Vec<_>>()
        .join("");
    tx.insert_version(
        asset,
        publication.version,
        &content,
        &hash(content.as_bytes()),
        source.source_id,
        decode(
            claim
                .payload
                .get("target_version")
                .cloned()
                .unwrap_or(Value::Null),
        )?,
        claim.payload["title"].as_str(),
    )
    .await?;
    tx.attach_review(
        asset,
        publication.version,
        decode(claim.payload["review_id"].clone())?,
    )
    .await?;
    for (ordinal, c) in publication.chunks.iter().enumerate() {
        tx.insert_chunk(
            c.id,
            asset,
            publication.version,
            ordinal as i32,
            &c.chunk.content,
            &c.chunk.locator,
            &parsing::lexical(&c.chunk.content),
        )
        .await?;
        tx.register_owner(source, "chunk", c.id, Some(c.id)).await?;
        if !c.summary.is_empty() {
            let id = graph::entity_id(&format!("summary:{}", c.id));
            tx.insert_summary(
                id,
                c.id,
                &c.summary,
                publication.summary_model.as_deref().unwrap_or(""),
                &parsing::lexical(&c.summary),
            )
            .await?;
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
    for entry in entries {
        if entry.key.surface == Surface::Graph {
            tx.register_owner(source, &entry.artifact_type, entry.key.artifact_id, None)
                .await?;
        }
        tx.confirm_committed(entry.key).await?;
    }
    tx.update_asset_version(asset, publication.version, claim.payload["title"].as_str())
        .await?;
    if let Some(id) = claim.payload["candidate_id"].as_str() {
        tx.mark_candidate_published(Uuid::parse_str(id).map_err(anyhow::Error::from)?)
            .await?;
    }
    tx.audit("asset.published",asset,json!({"version":publication.version,"source_event_id":source.source_id,"job_id":claim.job_id})).await?;
    let mut capabilities = vec!["keyword"];
    if publication.profile.is_some() {
        capabilities.push("vector");
    }
    if publication.chunks.iter().any(|c| !c.summary.is_empty()) {
        capabilities.push("summary");
    }
    if publication.graph.is_some() {
        capabilities.push("graph");
    }
    tx.settle_completed(claim.job_id,"published",&json!({"asset_id":asset,"version":publication.version,"readiness":"ready","index_capabilities":capabilities})).await?;
    tx.commit().await?;
    Ok(())
}
/// Runs only before serving requests or on the single worker, never concurrently
/// with native writes. Visibility is already blocked by the relational tombstone.
pub async fn cleanup(service: &Service) -> Result<()> {
    for scope in service.engine.relational().scopes().await? {
        let (vectors, entities, relations) =
            service.engine.relational().cleanup_plan(scope).await?;
        service
            .engine
            .vector()
            .delete_objects(scope, vectors)
            .await?;
        let ids = service.engine.graph().list_ids(scope).await?;
        let dead_entities: Vec<Uuid> = decode::<Vec<Uuid>>(ids["entities"].clone())?
            .into_iter()
            .filter(|id| !entities.contains(id))
            .collect();
        let dead_relations: Vec<Uuid> = decode::<Vec<Uuid>>(ids["relations"].clone())?
            .into_iter()
            .filter(|id| !relations.contains(id))
            .collect();
        service
            .engine
            .graph()
            .delete_objects(scope, dead_entities, dead_relations)
            .await?;
        service.engine.relational().finish_cleanup(scope).await?;
    }
    Ok(())
}
