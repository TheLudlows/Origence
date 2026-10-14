use crate::{
    error::{AppError, Result},
    parsing,
    service::{Service, decode, scope},
    storage::{
        DomainTx, FullTextHit, FullTextQuery, Permission, SourceVersion, VectorHit, VectorQuery,
    },
    types::*,
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

#[cfg(test)]
#[path = "retrieval/publication_tests.rs"]
mod publication_tests;

impl Service {
    pub async fn search(&self, a: &AuthContext, input: SearchInput) -> Result<Value> {
        // Native query futures are large. Keep that state on the heap rather
        // than embedding it in every host/router/helper future awaiting us.
        Box::pin(self.search_inner(a, input)).await
    }

    async fn search_inner(&self, a: &AuthContext, input: SearchInput) -> Result<Value> {
        // Separate poll bodies, not merely future storage: native SQL parsing
        // must not share a stack with every ranking/response temporary.
        match Box::pin(self.prepare_search(a, input)).await? {
            SearchPreparation::Empty(value) => Ok(value),
            SearchPreparation::Ready(prepared) => Box::pin(self.finish_search(a, *prepared)).await,
        }
    }

    async fn prepare_search(
        &self,
        a: &AuthContext,
        input: SearchInput,
    ) -> Result<SearchPreparation> {
        if let Some(identity) = &input.memory_identity {
            identity
                .validate()
                .map_err(|_| AppError::Invalid("invalid memory identity filter".into()))?;
        }
        if input.query.trim().is_empty()
            || input.query.len() > 4000
            || input.query.contains('\0')
            || !(1..=100).contains(&input.limit)
            || !["keyword", "vector", "hybrid"].contains(&input.mode.as_str())
        {
            return Err(AppError::Invalid("query, mode or limit invalid".into()));
        }
        let started = std::time::Instant::now();
        // Resolve the scoped identity before branch candidate limits and native top-k.
        // The final authorized read below still verifies visibility and identity.
        let mut authorized = self.read(a, Permission::Read).await?;
        let asset_filter = if let Some(identity) = input.memory_identity.as_ref() {
            authorized.identity_candidate_asset(identity).await?
        } else {
            None
        };
        authorized.rollback().await?;
        if input.memory_identity.is_some() && asset_filter.is_none() {
            return Ok(SearchPreparation::Empty(search_response(
                &input,
                &self.models.profile,
                &input.mode,
                &[],
                Vec::new(),
                (false, false),
                (Vec::new(), Vec::new()),
            )));
        }
        let authorization_ms = started.elapsed().as_secs_f64() * 1000.0;
        let embedding_started = std::time::Instant::now();
        let mut warnings = Vec::new();
        let mut effective = input.mode.clone();
        let embedding = if effective != "keyword" {
            match self.models.embed(&input.query).await {
                Ok(v) => Some(v),
                Err(_) if input.allow_partial => {
                    warnings.push("embedding unavailable; used keyword retrieval");
                    effective = "keyword".into();
                    None
                }
                Err(e) => return Err(e),
            }
        } else {
            None
        };
        let embedding_ms = embedding_started.elapsed().as_secs_f64() * 1000.0;
        let vector_dimension = embedding.as_ref().map_or(0, Vec::len);
        let mut eligibility_ms = 0.0;
        let mut vector_ms = 0.0;
        let mut vector_batches = 0_usize;
        let profile = self.models.profile.as_deref().unwrap_or("");
        let mut native_hits = Vec::new();
        if let Some(vector) = embedding {
            let eligibility_started = std::time::Instant::now();
            let mut tx = self.read(a, Permission::Read).await?;
            let generations = tx.vector_generations(profile, vector.len()).await?;
            let mut eligible = Vec::new();
            for generation in generations {
                let ids = tx
                    .vector_candidates(profile, vector.len(), generation, asset_filter)
                    .await?;
                let sources = tx.native_candidate_sources(&ids).await?;
                eligible.push((generation, ids, sources));
            }
            tx.commit().await?;
            eligibility_ms = eligibility_started.elapsed().as_secs_f64() * 1000.0;
            let vector_started = std::time::Instant::now();
            for (generation, ids, sources) in eligible {
                if !ids.is_empty() {
                    vector_batches += 1;
                    for hit in self
                        .engine
                        .vector()
                        .search_authorized(
                            VectorQuery {
                                scope: scope(a),
                                artifact_ids: Some(ids),
                                profile: profile.into(),
                                dimension: vector.len(),
                                generation,
                                embedding: vector.clone(),
                                limit: 100,
                            },
                            &sources,
                        )
                        .await?
                    {
                        native_hits.push((generation, hit));
                    }
                }
            }
            vector_ms = vector_started.elapsed().as_secs_f64() * 1000.0;
        }
        let terms = parsing::lexical(&input.query);
        let mut full_text_hits = Vec::new();
        if effective != "vector" && !terms.is_empty() {
            let mut eligibility = self.read(a, Permission::Read).await?;
            let mut eligible = Vec::new();
            for generation in eligibility.full_text_generations().await? {
                let ids = eligibility
                    .full_text_candidates(generation, asset_filter)
                    .await?;
                let sources = eligibility.native_candidate_sources(&ids).await?;
                eligible.push((generation, ids, sources));
            }
            eligibility.commit().await?;
            for (generation, ids, sources) in eligible {
                if ids.is_empty() {
                    continue;
                }
                for hit in self
                    .engine
                    .vector()
                    .search_full_text_authorized(
                        FullTextQuery {
                            scope: scope(a),
                            artifact_ids: Some(ids),
                            generation,
                            search_terms: terms.clone(),
                            limit: 100,
                        },
                        &sources,
                    )
                    .await?
                {
                    full_text_hits.push((generation, hit));
                }
            }
        }
        Ok(SearchPreparation::Ready(Box::new(PreparedSearch {
            input,
            started,
            asset_filter,
            authorization_ms,
            embedding_ms,
            eligibility_ms,
            vector_ms,
            vector_batches,
            vector_dimension,
            warnings,
            effective,
            terms,
            native_hits,
            full_text_hits,
        })))
    }

    async fn finish_search(&self, a: &AuthContext, prepared: PreparedSearch) -> Result<Value> {
        let PreparedSearch {
            input,
            started,
            asset_filter,
            authorization_ms,
            embedding_ms,
            eligibility_ms,
            vector_ms,
            vector_batches,
            vector_dimension,
            warnings,
            effective,
            terms,
            native_hits,
            full_text_hits,
        } = prepared;
        let profile = self.models.profile.as_deref().unwrap_or("");
        let native_count = native_hits.len() + full_text_hits.len();
        let branches_started = std::time::Instant::now();
        let summaries_enabled =
            effective == "hybrid" && input.memory_identity.is_none() && input.components.summaries;
        let graph_enabled =
            effective == "hybrid" && input.memory_identity.is_none() && input.components.graph;
        let graph = if graph_enabled {
            Some(self.engine.graph().snapshot(scope(a)).await?)
        } else {
            None
        };
        // Native candidates become evidence only inside a fresh scoped snapshot.
        let mut tx = self.read(a, Permission::Read).await?;
        let mut branches = Vec::new();
        let mut ranked_native = Vec::new();
        if effective != "vector" {
            let mut keyword = Vec::new();
            let mut seen = HashMap::new();
            let keys: Vec<_> = full_text_hits
                .iter()
                .map(|(generation, native)| (native.id, native.source, *generation))
                .collect();
            let candidates = tx.full_text_hits(&keys).await?;
            let missing: Vec<_> = keys
                .iter()
                .filter(|key| !candidates.contains_key(key))
                .map(|key| key.0)
                .collect();
            let live_missing = tx.visible_hits(&missing).await?;
            for (generation, native) in full_text_hits {
                let candidate = candidates
                    .get(&(native.id, native.source, generation))
                    .cloned();
                if let Some(mut hit) = batch_native_candidate(native.id, candidate, &live_missing)?
                {
                    if asset_filter.is_some_and(|asset| hit.asset_id != asset) {
                        return Err(AppError::Conflict(
                            "native retrieval identity changed".into(),
                        ));
                    }
                    if !unique_native(
                        &mut seen,
                        native.id,
                        generation,
                        native.source,
                        native.score,
                    )? {
                        continue;
                    }
                    hit.score = f64::from(native.score);
                    ranked_native.push(NativeRankedCandidate {
                        hit: hit.clone(),
                        proof: NativePublicationProof::FullText {
                            source: native.source,
                            generation,
                        },
                    });
                    keyword.push(hit);
                }
            }
            prepare_branch(&mut keyword, 100);
            branches.push(keyword);
        }
        let mut vectors = Vec::new();
        let mut seen_vectors = HashMap::new();
        let keys: Vec<_> = native_hits
            .iter()
            .map(|(generation, native)| (native.id, native.source, *generation))
            .collect();
        let candidates = tx
            .vector_hits_exact(&keys, profile, vector_dimension)
            .await?;
        let missing: Vec<_> = keys
            .iter()
            .filter(|key| !candidates.contains_key(key))
            .map(|key| key.0)
            .collect();
        let live_missing = tx.visible_hits(&missing).await?;
        for (generation, native) in native_hits {
            let candidate = candidates
                .get(&(native.id, native.source, generation))
                .cloned();
            if let Some(mut hit) = batch_native_candidate(native.id, candidate, &live_missing)? {
                if asset_filter.is_some_and(|asset| hit.asset_id != asset) {
                    return Err(AppError::Conflict(
                        "native retrieval identity changed".into(),
                    ));
                }
                if !unique_native(
                    &mut seen_vectors,
                    native.id,
                    generation,
                    native.source,
                    native.score,
                )? {
                    continue;
                }
                hit.score = native.score as f64;
                ranked_native.push(NativeRankedCandidate {
                    hit: hit.clone(),
                    proof: NativePublicationProof::Vector {
                        source: native.source,
                        generation,
                        profile: profile.into(),
                        dimension: vector_dimension,
                    },
                });
                vectors.push(hit);
            }
        }
        prepare_branch(&mut vectors, 100);
        if effective != "keyword" {
            branches.push(vectors);
        }
        let (mut entities, mut relations) = (Vec::new(), Vec::new());
        if summaries_enabled {
            branches.push(tx.keyword_hits(&terms, true, asset_filter).await?);
        }
        if let Some(graph) = graph {
            let all_entities: Vec<Value> = decode(graph["entities"].clone())?;
            let all_relations: Vec<Value> = decode(graph["relations"].clone())?;
            let mut ids = HashSet::new();
            for entity in &all_entities {
                if !terms
                    .split_whitespace()
                    .any(|term| entity["name"].as_str().unwrap_or("").contains(term))
                {
                    continue;
                }
                let id: Uuid = decode(entity["id"].clone())?;
                // Withdrawn dependencies must not consume the seed budget.
                if !tx.graph_evidence(id).await?.is_empty() {
                    ids.insert(id);
                    if ids.len() == 20 {
                        break;
                    }
                }
            }
            let seeds = ids.clone();
            let mut edges = Vec::new();
            for edge in all_relations {
                let from: Uuid = decode(edge["source_id"].clone())?;
                let to: Uuid = decode(edge["target_id"].clone())?;
                if seeds.contains(&from) || seeds.contains(&to) {
                    ids.insert(from);
                    ids.insert(to);
                    edges.push(edge);
                }
            }
            let mut evidence_hits = Vec::new();
            for mut entity in all_entities {
                let id: Uuid = decode(entity["id"].clone())?;
                if !ids.contains(&id) {
                    continue;
                }
                let evidence = tx.graph_evidence(id).await?;
                if !evidence.is_empty() {
                    for e in &evidence {
                        if let Some(h) = tx.visible_hit(decode(e["chunk_id"].clone())?).await? {
                            evidence_hits.push(h);
                        }
                    }
                    entity["evidence"] = json!(evidence);
                    entities.push(entity);
                }
                if entities.len() >= 20 {
                    break;
                }
            }
            let visible: HashSet<Uuid> = entities
                .iter()
                .map(|e| decode(e["id"].clone()))
                .collect::<Result<_>>()?;
            for mut edge in edges {
                if !visible.contains(&decode(edge["source_id"].clone())?)
                    || !visible.contains(&decode(edge["target_id"].clone())?)
                {
                    continue;
                }
                let evidence = tx.graph_evidence(decode(edge["id"].clone())?).await?;
                if !evidence.is_empty() {
                    edge["evidence"] = json!(evidence);
                    relations.push(edge);
                }
                if relations.len() >= 20 {
                    break;
                }
            }
            evidence_hits.sort_by_key(|h| h.chunk_id);
            evidence_hits.dedup_by_key(|h| h.chunk_id);
            branches.push(evidence_hits);
        }
        let hits = fuse(branches, effective == "hybrid");
        tx.commit().await?;
        let branches_ms = branches_started.elapsed().as_secs_f64() * 1000.0;
        let verification_started = std::time::Instant::now();
        let mut verify = self.read(a, Permission::Read).await?;
        // Validate every native participant, including those below the final
        // response limit: its disappearance can change another hit's RRF rank.
        verify_native_candidates(&mut verify, &ranked_native).await?;
        let hits = verify_search_hits(
            &mut verify,
            &hits,
            input.limit,
            input.memory_identity.as_ref(),
        )
        .await?;
        let mut live_entities = Vec::new();
        for mut e in entities {
            let evidence = verify.graph_evidence(decode(e["id"].clone())?).await?;
            if !evidence.is_empty() {
                e["evidence"] = json!(evidence);
                live_entities.push(e);
            }
        }
        let entities = live_entities;
        let ids: HashSet<Uuid> = entities
            .iter()
            .map(|e| decode(e["id"].clone()))
            .collect::<Result<_>>()?;
        let mut live_relations = Vec::new();
        for mut r in relations {
            if !ids.contains(&decode(r["source_id"].clone())?)
                || !ids.contains(&decode(r["target_id"].clone())?)
            {
                continue;
            }
            let evidence = verify.graph_evidence(decode(r["id"].clone())?).await?;
            if !evidence.is_empty() {
                r["evidence"] = json!(evidence);
                live_relations.push(r);
            }
        }
        let relations = live_relations;
        verify.commit().await?;
        let verification_ms = verification_started.elapsed().as_secs_f64() * 1000.0;
        let hits: Vec<Value> = hits
            .into_iter()
            .map(|hit| {
                let mut value = json!(hit);
                value["normalization_status"] = json!(hit.normalization_status());
                value
            })
            .collect();
        // Opt-in aggregate diagnostics only: never record queries, IDs, paths or evidence.
        tracing::debug!(
            target: "origence::search_timing",
            authorization_ms, embedding_ms, eligibility_ms, vector_ms, branches_ms,
            verification_ms, total_ms = started.elapsed().as_secs_f64() * 1000.0,
            vector_batches, native_count, returned_count = hits.len(),
            "search completed"
        );
        Ok(search_response(
            &input,
            &self.models.profile,
            &effective,
            &warnings,
            hits,
            (summaries_enabled, graph_enabled),
            (entities, relations),
        ))
    }
    pub async fn resolve(&self, a: &AuthContext, input: ResolveInput) -> Result<Value> {
        Box::pin(self.resolve_inner(a, input)).await
    }

    async fn resolve_inner(&self, a: &AuthContext, input: ResolveInput) -> Result<Value> {
        if input.budget_tokens > 32000 {
            return Err(AppError::Invalid("budget_tokens must be <=32000".into()));
        }
        let search = self
            .search(
                a,
                SearchInput {
                    components: input.components,
                    memory_identity: input.memory_identity,
                    query: input.query,
                    limit: 100,
                    mode: input.mode,
                    allow_partial: input.allow_partial,
                },
            )
            .await?;
        let hits: Vec<SearchHit> =
            serde_json::from_value(search["hits"].clone()).map_err(anyhow::Error::from)?;
        let (rendered, sources) = render(&hits, input.budget_tokens);
        // Graph fragments share the same conservative byte budget; cited as a side band.
        let graph_entities: Vec<Value> =
            serde_json::from_value(search["graph"]["entities"].clone()).unwrap_or_default();
        let graph_relations: Vec<Value> =
            serde_json::from_value(search["graph"]["relations"].clone()).unwrap_or_default();
        // Remaining budget after chunk rendering; whole graph blocks are kept or dropped, never partial.
        let remaining = input.budget_tokens.saturating_sub(rendered.len());
        let (graph_text, graph_sources) =
            crate::graph::render_graph(&graph_entities, &graph_relations, remaining);
        let rendered = format!("{rendered}{graph_text}");
        let mut all_sources = sources;
        all_sources.extend(graph_sources);
        Ok(
            json!({"rendered_context":rendered,"sources":all_sources,"budget_tokens":input.budget_tokens,"count":rendered.len(),"tokenizer":"utf8-bytes-upper-bound-v1","context_policy":"identity-provenance-v1","memory_identity":search["memory_identity"],"count_is_estimate":true,"effective_mode":search["effective_mode"],"warnings":search["warnings"],"active_components":search["active_components"],"graph":search["graph"]}),
        )
    }
}

enum SearchPreparation {
    Empty(Value),
    Ready(Box<PreparedSearch>),
}

// Phase handoff contains results only; no transaction crosses this boundary.
struct PreparedSearch {
    input: SearchInput,
    started: std::time::Instant,
    asset_filter: Option<Uuid>,
    authorization_ms: f64,
    embedding_ms: f64,
    eligibility_ms: f64,
    vector_ms: f64,
    vector_batches: usize,
    vector_dimension: usize,
    warnings: Vec<&'static str>,
    effective: String,
    terms: String,
    native_hits: Vec<(i64, VectorHit)>,
    full_text_hits: Vec<(i64, FullTextHit)>,
}

// Serialize only after the authority transactions finish. Keeping these JSON
// temporaries out of search_inner's poll frame leaves stack for native SQL
// parsing on the default Windows main thread; boxing the future alone does not.
fn search_response(
    input: &SearchInput,
    profile: &Option<String>,
    effective: &str,
    warnings: &[&str],
    hits: Vec<Value>,
    components: (bool, bool),
    graph: (Vec<Value>, Vec<Value>),
) -> Value {
    let (summaries_enabled, graph_enabled) = components;
    let (entities, relations) = graph;
    json!({"hits":hits,"memory_identity":input.memory_identity,"requested_mode":input.mode,"effective_mode":effective,"warnings":warnings,"embedding_profile":profile,"retrieval_policy":"turso-lance-filtered-rrf60-v2","active_components":{"summaries":summaries_enabled,"graph":graph_enabled},"graph":{"entities":entities,"relations":relations}})
}

type NativeCandidateKey = (Uuid, SourceVersion, i64);

/// Exact authority that justified a native score. Kept private and retained
/// until the response's final relational snapshot, never serialized to clients.
#[derive(Clone, Debug)]
pub(crate) enum NativePublicationProof {
    FullText {
        source: SourceVersion,
        generation: i64,
    },
    Vector {
        source: SourceVersion,
        generation: i64,
        profile: String,
        dimension: usize,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct NativeRankedCandidate {
    pub hit: SearchHit,
    pub proof: NativePublicationProof,
}

fn same_evidence_identity(first: &SearchHit, second: &SearchHit) -> bool {
    first.asset_id == second.asset_id
        && first.version == second.version
        && first.chunk_id == second.chunk_id
        && first.source_event_id == second.source_event_id
        && first.kind == second.kind
        && first.content == second.content
        && first.locator == second.locator
}

/// Recheck all native ranking participants before returning any previously
/// computed score. A disappeared participant makes that ranking unavailable;
/// a live object with changed publication identity is a storage conflict.
/// No native IO, new ranking, retry loop or cross-store snapshot is implied.
pub(crate) async fn verify_native_candidates(
    tx: &mut crate::storage::sqlite::TursoTx,
    candidates: &[NativeRankedCandidate],
) -> Result<()> {
    let mut full_text = Vec::new();
    let mut vectors: HashMap<(String, usize), Vec<NativeCandidateKey>> = HashMap::new();
    for candidate in candidates {
        match &candidate.proof {
            NativePublicationProof::FullText { source, generation } => {
                full_text.push((candidate.hit.chunk_id, *source, *generation));
            }
            NativePublicationProof::Vector {
                source,
                generation,
                profile,
                dimension,
            } => {
                vectors
                    .entry((profile.clone(), *dimension))
                    .or_default()
                    .push((candidate.hit.chunk_id, *source, *generation));
            }
        }
    }
    let full_text_hits = tx.full_text_hits(&full_text).await?;
    let mut vector_hits = HashMap::new();
    for ((profile, dimension), keys) in vectors {
        vector_hits.insert(
            (profile.clone(), dimension),
            tx.vector_hits_exact(&keys, &profile, dimension).await?,
        );
    }
    let mut checked = Vec::new();
    let mut missing = Vec::new();
    for candidate in candidates {
        let id = candidate.hit.chunk_id;
        let current = match &candidate.proof {
            NativePublicationProof::FullText { source, generation } => {
                full_text_hits.get(&(id, *source, *generation)).cloned()
            }
            NativePublicationProof::Vector {
                source,
                generation,
                profile,
                dimension,
            } => vector_hits
                .get(&(profile.clone(), *dimension))
                .and_then(|hits| hits.get(&(id, *source, *generation)))
                .cloned(),
        };
        if current.is_none() {
            missing.push(id);
        }
        checked.push(current);
    }
    let live_missing = tx.visible_hits(&missing).await?;
    for (candidate, current) in candidates.iter().zip(checked) {
        let current = batch_native_candidate(candidate.hit.chunk_id, current, &live_missing)?
            .ok_or_else(|| AppError::Unavailable("native retrieval became unavailable".into()))?;
        if !same_evidence_identity(&candidate.hit, &current) {
            return Err(AppError::Conflict(
                "native retrieval evidence changed".into(),
            ));
        }
    }
    Ok(())
}

/// Keep final response verification bounded by its original rank stop point.
pub(crate) async fn verify_search_hits(
    tx: &mut crate::storage::sqlite::TursoTx,
    hits: &[SearchHit],
    limit: usize,
    identity: Option<&crate::memory_identity::MemoryIdentity>,
) -> Result<Vec<SearchHit>> {
    let mut current = Vec::new();
    let mut cursor = 0;
    // At most the remaining response capacity is inspected per batch. This
    // preserves the old stop point for non-native Summary/Graph candidates:
    // a malformed lower-ranked row must not invalidate an already-full page.
    while cursor < hits.len() {
        let capacity = limit.saturating_sub(current.len()).max(1);
        let end = (cursor + capacity).min(hits.len());
        let ids: Vec<_> = hits[cursor..end].iter().map(|hit| hit.chunk_id).collect();
        let live_hits = tx.visible_hits(&ids).await?;
        let mut selected = Vec::new();
        for h in &hits[cursor..end] {
            if let Some(mut live) = live_hits.get(&h.chunk_id).cloned() {
                live.score = h.score;
                selected.push(live);
            }
        }
        let assets: Vec<_> = selected.iter().map(|hit| hit.asset_id).collect();
        let identities = tx.retrieval_identities(&assets).await?;
        for mut live in selected {
            live.identity = identities.get(&live.asset_id).cloned().flatten();
            if !live.matches_identity(identity) {
                return Err(AppError::Conflict(
                    "native retrieval identity changed".into(),
                ));
            }
            current.push(live);
        }
        if current.len() == limit {
            break;
        }
        cursor = end;
    }
    Ok(current)
}

fn batch_native_candidate(
    id: Uuid,
    candidate: Option<SearchHit>,
    live_missing: &HashMap<Uuid, SearchHit>,
) -> Result<Option<SearchHit>> {
    if candidate.is_none() {
        if live_missing.contains_key(&id) {
            return Err(AppError::Conflict(
                "native retrieval does not match live publication".into(),
            ));
        }
        return Err(AppError::Unavailable(
            "native retrieval became unavailable".into(),
        ));
    }
    Ok(candidate)
}

/// Once a native candidate has participated in top-k, dropping it silently can
/// conceal an incomplete result set or retain RRF ranks from withdrawn evidence.
pub(crate) async fn live_native_candidate(
    tx: &mut crate::storage::sqlite::TursoTx,
    id: Uuid,
    candidate: Option<SearchHit>,
) -> Result<Option<SearchHit>> {
    if candidate.is_none() {
        if tx.visible_hit(id).await?.is_some() {
            return Err(AppError::Conflict(
                "native retrieval does not match live publication".into(),
            ));
        }
        return Err(AppError::Unavailable(
            "native retrieval became unavailable".into(),
        ));
    }
    Ok(candidate)
}

/// Native duplicates are collapsible only when the complete authoritative
/// identity and score agree. Contradictory committed generations are not a
/// license to choose a convenient score; a rebuild needs an active pointer.
pub(crate) fn unique_native(
    seen: &mut HashMap<Uuid, (i64, SourceVersion, u32)>,
    id: Uuid,
    generation: i64,
    source: SourceVersion,
    score: f32,
) -> Result<bool> {
    if !score.is_finite() {
        return Err(AppError::Conflict("invalid native retrieval score".into()));
    }
    let identity = (generation, source, score.to_bits());
    if let Some(previous) = seen.get(&id) {
        if *previous != identity {
            return Err(AppError::Conflict(
                "conflicting native retrieval identity".into(),
            ));
        }
        return Ok(false);
    }
    seen.insert(id, identity);
    Ok(true)
}

/// Normalize each scoped branch before assigning RRF ranks. A retried native
/// generation cannot cast extra votes for the same evidence identity.
pub(crate) fn prepare_branch(hits: &mut Vec<SearchHit>, limit: usize) {
    hits.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(a.chunk_id.cmp(&b.chunk_id))
    });
    let mut seen = HashSet::new();
    hits.retain(|hit| seen.insert((hit.asset_id, hit.version, hit.chunk_id, hit.source_event_id)));
    hits.truncate(limit);
}

fn fuse(branches: Vec<Vec<SearchHit>>, hybrid: bool) -> Vec<SearchHit> {
    let mut merged: HashMap<Uuid, SearchHit> = HashMap::new();
    for mut branch in branches {
        prepare_branch(&mut branch, 100);
        for (rank, mut hit) in branch.into_iter().enumerate() {
            if hybrid {
                hit.score = 1.0 / (60.0 + rank as f64 + 1.0);
            }
            merged
                .entry(hit.chunk_id)
                .and_modify(|old| old.score += hit.score)
                .or_insert(hit);
        }
    }
    let mut hits: Vec<_> = merged.into_values().collect();
    hits.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(a.chunk_id.cmp(&b.chunk_id))
    });
    hits
}
pub fn render(hits: &[SearchHit], budget: usize) -> (String, Vec<Value>) {
    // Explicit conservative byte budget, not a claim to model-specific tokenization.
    // Byte count includes all titles, separators and citation markers. No partial citation records.
    let mut rendered = String::new();
    let mut sources = Vec::new();
    for hit in hits {
        let marker = format!("[{}:{}:{}]", hit.asset_id, hit.version, hit.chunk_id);
        let mut identity = json!({"kind":hit.kind,"identity":hit.identity,"normalization_status":hit.normalization_status()});
        if hit.locator["parser"] == crate::memory_semantics::PARSER {
            identity["semantic_status"] = hit.locator["semantic_status"].clone();
            identity["suggested_identity"] = hit.locator["suggested_identity"].clone();
            identity["semantic_value"] = hit.locator["semantic_value"].clone();
        }
        if let Some(learning) = hit.locator.get("learning") {
            identity["learning"] = learning.clone();
        }
        let block = format!("{marker} {}\n{identity}\n{}\n\n", hit.title, hit.content);
        if rendered.len() + block.len() > budget {
            continue;
        }
        rendered.push_str(&block);
        sources.push(json!({"citation":marker,"asset_id":hit.asset_id,"version":hit.version,"chunk_id":hit.chunk_id,"source_event_id":hit.source_event_id,"locator":hit.locator,"kind":hit.kind,"identity":hit.identity,"normalization_status":hit.normalization_status()}));
    }
    (rendered, sources)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn budget_includes_citations_and_utf8() {
        let hit = SearchHit {
            asset_id: Uuid::new_v4(),
            version: 1,
            chunk_id: Uuid::new_v4(),
            kind: "memory".into(),
            title: "标题".into(),
            content: "中文内容".repeat(30),
            locator: json!({}),
            source_event_id: Uuid::new_v4(),
            score: 1.0,
            identity: None,
        };
        for budget in [0, 10, 80, 500] {
            let (text, sources) = render(std::slice::from_ref(&hit), budget);
            assert!(text.len() <= budget);
            assert_eq!(text.is_empty(), sources.is_empty());
        }
        let mut hit = hit;
        hit.content = "审批".into();
        hit.identity = Some(
            serde_json::from_value(json!({
            "subject":{"kind":"service","stable_id":"billing"},
            "predicate":"release.approval","context":{"environment":"production"}
            }))
            .unwrap(),
        );
        let (text, sources) = render(std::slice::from_ref(&hit), 2000);
        assert!(text.contains("explicit_identity"));
        assert!(text.contains("production"));
        assert_eq!(sources[0]["identity"], json!(hit.identity));
        assert_eq!(sources[0]["kind"], "memory");
        let (smaller, sources) = render(std::slice::from_ref(&hit), text.len() - 1);
        assert!(smaller.is_empty());
        assert!(sources.is_empty());
        assert_eq!(render(&[hit], text.len()).0, text);
    }
}
