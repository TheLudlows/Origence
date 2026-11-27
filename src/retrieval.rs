use crate::{
    error::{AppError, Result},
    parsing,
    service::{Service, decode, scope},
    storage::{
        DomainTx, FullTextHit, FullTextQuery, Permission, SourceVersion, VectorHit, VectorQuery,
        anyseek::{GraphEdgeCandidate, GraphEntityCandidate},
        sqlite::SqliteTx,
    },
    types::*,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use uuid::Uuid;

#[cfg(test)]
#[path = "retrieval/publication_tests.rs"]
mod publication_tests;

pub(crate) mod rank;
use crate::storage::sqlite::retrieval::{
    NativePublicationProof, NativeRankedCandidate, batch_native_candidate,
};
use rank::{fuse, prepare_branch};

const GRAPH_LIMIT: usize = 20;

struct GraphCandidates {
    seeds: BTreeMap<Uuid, GraphEntityCandidate>,
    edges: Vec<GraphEdgeCandidate>,
}

impl Service {
    async fn find_graph_candidates(&self, a: &AuthContext, terms: &str) -> Result<GraphCandidates> {
        let scope = scope(a);
        let remote = self.engine.vector();
        let candidates = remote.graph_seed_candidates(scope, terms).await?;
        let mut seeds = BTreeMap::new();
        if !candidates.is_empty() {
            // Apply the disclosure gate before the seed budget. A pending or
            // withdrawn projection can neither fan out nor crowd out live seeds.
            let mut tx = self.read(a, Permission::Read).await?;
            for candidate in candidates {
                if !seeds.contains_key(&candidate.id)
                    && !tx
                        .graph_evidence_for_publication(
                            candidate.id,
                            candidate.source,
                            candidate.generation,
                        )
                        .await?
                        .is_empty()
                {
                    seeds.insert(candidate.id, candidate);
                    if seeds.len() == GRAPH_LIMIT {
                        break;
                    }
                }
            }
            tx.commit().await?;
        }
        let seed_ids: Vec<_> = seeds.keys().copied().collect();
        let edges = if seed_ids.is_empty() {
            Vec::new()
        } else {
            remote.graph_neighbors(scope, &seed_ids).await?
        };
        Ok(GraphCandidates { seeds, edges })
    }

    pub async fn search(&self, a: &AuthContext, input: SearchInput) -> Result<Value> {
        // Native query futures are large. Keep that state on the heap rather
        // than embedding it in every host/router/helper future awaiting us.
        let (result, observations) =
            crate::retrieval_diagnostics::capture(Box::pin(self.search_inner(a, input))).await;
        result.map(|mut response| {
            response["retrieval"] = json!(observations);
            response
        })
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
        let query = input.query.clone();
        let terms = tokio::task::spawn_blocking(move || parsing::lexical(&query))
            .await
            .map_err(|error| AppError::Internal(error.into()))?;
        let authorization_started = std::time::Instant::now();
        // Resolve the scoped identity before branch candidate limits and native top-k.
        // The final authorized read below still verifies visibility and identity.
        let mut authorized = self.read(a, Permission::Read).await?;
        let asset_filter = if let Some(identity) = input.memory_identity.as_ref() {
            authorized.identity_candidate_asset(identity).await?
        } else {
            None
        };
        if input.memory_identity.is_some() && asset_filter.is_none() {
            authorized.rollback().await?;
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
        // Keyword eligibility uses this same authorized snapshot. Model IO in
        // vector/hybrid still runs after releasing the initial transaction.
        let mut keyword_authorization = if input.mode == "keyword" {
            Some(authorized)
        } else {
            authorized.rollback().await?;
            None
        };
        let authorization_ms = authorization_started.elapsed().as_secs_f64() * 1000.0;
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
        let mut full_text_hits = Vec::new();
        if effective != "vector" && !terms.is_empty() {
            let mut eligibility = match keyword_authorization.take() {
                Some(tx) => tx,
                None => self.read(a, Permission::Read).await?,
            };
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
        if let Some(tx) = keyword_authorization {
            tx.rollback().await?;
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
            mut warnings,
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
            Some(Box::pin(self.find_graph_candidates(a, &terms)).await?)
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
        if summaries_enabled {
            branches.push(tx.summary_hits(&terms, asset_filter).await?);
        }
        let (entities, relations) = if let Some(graph) = graph {
            let (entities, relations, evidence_hits) =
                Box::pin(project_graph_evidence(&mut tx, graph)).await?;
            branches.push(evidence_hits);
            (entities, relations)
        } else {
            (Vec::new(), Vec::new())
        };
        let hits = fuse(branches, effective == "hybrid");
        tx.commit().await?;
        let branches_ms = branches_started.elapsed().as_secs_f64() * 1000.0;
        let verification_started = std::time::Instant::now();
        let mut verify = self.read(a, Permission::Read).await?;
        // Validate every native participant, including those below the final
        // response limit: its disappearance can change another hit's RRF rank.
        verify.verify_native_candidates(&ranked_native).await?;
        let mut hits = verify
            .verify_search_hits(&hits, input.limit, input.memory_identity.as_ref())
            .await?;
        if !hits.is_empty() {
            verify.commit().await?;
            self.hydrate_hits(scope(a), &mut hits).await?;
            verify = self.read(a, Permission::Read).await?;
            verify.verify_native_candidates(&ranked_native).await?;
            verify.verify_content_hits(&hits).await?;
        }
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
        if input.memory_identity.is_none() {
            let shared = verify.global_keyword_hits(&terms, input.limit).await?;
            if !shared.is_empty() {
                if effective != "keyword" {
                    warnings.push("Global knowledge uses keyword matching.");
                }
                hits = merge_global_hits(hits, shared, input.limit);
            }
        }
        verify.commit().await?;
        let verification_ms = verification_started.elapsed().as_secs_f64() * 1000.0;
        let hits: Vec<Value> = hits
            .into_iter()
            .map(|hit| {
                let mut value = json!(hit);
                value["normalization_status"] = json!(hit.normalization_status());
                value["scope"] = json!(hit_scope(&hit));
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

/// Interleave local and shared rankings without comparing different score
/// scales. A local hit wins when there is only one available response slot.
pub(crate) fn merge_global_hits<T>(local: Vec<T>, global: Vec<T>, limit: usize) -> Vec<T> {
    let mut local = local.into_iter();
    let mut global = global.into_iter();
    (0..limit)
        .map_while(|rank| {
            if rank % 2 == 0 {
                local.next().or_else(|| global.next())
            } else {
                global.next().or_else(|| local.next())
            }
        })
        .collect()
}

fn hit_scope(hit: &SearchHit) -> &'static str {
    if hit.locator["parser"] == "global-knowledge-snapshot:v1" {
        "global"
    } else {
        "workspace"
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

async fn project_graph_evidence(
    tx: &mut SqliteTx,
    graph: GraphCandidates,
) -> Result<(Vec<Value>, Vec<Value>, Vec<SearchHit>)> {
    let mut names = BTreeMap::new();
    for (id, seed) in graph.seeds {
        if !tx
            .graph_evidence_for_publication(id, seed.source, seed.generation)
            .await?
            .is_empty()
        {
            names.insert(id, seed.name);
        }
    }
    // Snapshot only the seeds. Neighbor endpoints must not seed a second hop.
    let seeds: HashSet<_> = names.keys().copied().collect();
    let mut edges = BTreeMap::new();
    for edge in graph.edges {
        if !(seeds.contains(&edge.from) || seeds.contains(&edge.to))
            || edges.contains_key(&edge.id)
            || tx
                .graph_evidence_for_publication(edge.id, edge.source, edge.generation)
                .await?
                .is_empty()
        {
            continue;
        }
        names
            .entry(edge.from)
            .or_insert_with(|| edge.from_name.clone());
        names.entry(edge.to).or_insert_with(|| edge.to_name.clone());
        edges.insert(edge.id, edge);
    }
    let mut entities = Vec::new();
    let mut evidence_hits = Vec::new();
    let mut visible = HashSet::new();
    for (id, name) in names {
        let evidence = tx.graph_evidence(id).await?;
        if evidence.is_empty() {
            continue;
        }
        for e in &evidence {
            if let Some(hit) = tx.visible_hit(decode(e["chunk_id"].clone())?).await? {
                evidence_hits.push(hit);
            }
        }
        entities.push(json!({"id":id,"name":name,"evidence":evidence}));
        visible.insert(id);
        if entities.len() == GRAPH_LIMIT {
            break;
        }
    }
    let mut relations = Vec::new();
    for (id, edge) in edges {
        if !visible.contains(&edge.from) || !visible.contains(&edge.to) {
            continue;
        }
        let evidence = tx.graph_evidence(id).await?;
        if !evidence.is_empty() {
            let fact = format!("{} {} {}", edge.from_name, edge.predicate, edge.to_name);
            relations.push(json!({"id":id,"source_id":edge.from,"target_id":edge.to,
                "fact_text":fact,"evidence":evidence}));
        }
        if relations.len() == GRAPH_LIMIT {
            break;
        }
    }
    evidence_hits.sort_by_key(|hit| hit.chunk_id);
    evidence_hits.dedup_by_key(|hit| hit.chunk_id);
    Ok((entities, relations, evidence_hits))
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
    json!({"hits":hits,"memory_identity":input.memory_identity,"requested_mode":input.mode,"effective_mode":effective,"warnings":warnings,"embedding_profile":profile,"retrieval_policy":"sqlite-anyseek-filtered-rrf60-v3","active_components":{"summaries":summaries_enabled,"graph":graph_enabled},"graph":{"entities":entities,"relations":relations}})
}

/// Once a native candidate has participated in top-k, dropping it silently can
/// conceal an incomplete result set or retain RRF ranks from withdrawn evidence.
pub(crate) async fn live_native_candidate(
    tx: &mut crate::storage::sqlite::SqliteTx,
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

pub fn render(hits: &[SearchHit], budget: usize) -> (String, Vec<Value>) {
    // Explicit conservative byte budget, not a claim to model-specific tokenization.
    // Byte count includes all titles, separators and citation markers. No partial citation records.
    let mut rendered = String::new();
    let mut sources = Vec::new();
    for hit in hits {
        let marker = format!("[{}:{}:{}]", hit.asset_id, hit.version, hit.chunk_id);
        let mut identity = json!({"kind":hit.kind,"identity":hit.identity,"normalization_status":hit.normalization_status()});
        if hit_scope(hit) == "global" {
            identity["scope"] = json!("global");
        }
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
        sources.push(json!({"citation":marker,"asset_id":hit.asset_id,"version":hit.version,"chunk_id":hit.chunk_id,"source_event_id":hit.source_event_id,"locator":hit.locator,"kind":hit.kind,"scope":hit_scope(hit),"identity":hit.identity,"normalization_status":hit.normalization_status()}));
    }
    (rendered, sources)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn budget_includes_citations_and_utf8() {
        let hit = SearchHit {
            content_hash: String::new(),
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
