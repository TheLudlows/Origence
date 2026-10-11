use crate::{
    error::{AppError, Result},
    parsing,
    service::{Service, decode, scope},
    storage::{DomainTx, Permission, VectorQuery, VectorStore},
    types::*,
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;
impl Service {
    pub async fn search(&self, a: &AuthContext, input: SearchInput) -> Result<Value> {
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
            return Ok(json!({
                "hits": [],
                "memory_identity": input.memory_identity,
                "requested_mode": input.mode,
                "effective_mode": input.mode,
                "warnings": [],
                "embedding_profile": self.models.profile,
                "active_components": {"summaries": false, "graph": false},
                "retrieval_policy": "local-scoped-exact-rrf60-v1",
                "graph": {"entities": [], "relations": []}
            }));
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
                eligible.push((
                    generation,
                    tx.vector_candidates(profile, vector.len(), generation, asset_filter)
                        .await?,
                ));
            }
            tx.commit().await?;
            eligibility_ms = eligibility_started.elapsed().as_secs_f64() * 1000.0;
            let vector_started = std::time::Instant::now();
            for (generation, ids) in eligible {
                for batch in ids.chunks(512) {
                    vector_batches += 1;
                    for hit in self
                        .engine
                        .vector()
                        .search(VectorQuery {
                            scope: scope(a),
                            artifact_ids: Some(batch.to_vec()),
                            profile: profile.into(),
                            dimension: vector.len(),
                            generation,
                            embedding: vector.clone(),
                            limit: 100,
                        })
                        .await?
                    {
                        native_hits.push((generation, hit));
                    }
                }
            }
            vector_ms = vector_started.elapsed().as_secs_f64() * 1000.0;
        }
        let native_count = native_hits.len();
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
        let terms = parsing::lexical(&input.query);
        // Native candidates become evidence only inside a fresh scoped snapshot.
        let mut tx = self.read(a, Permission::Read).await?;
        let mut branches = Vec::new();
        if effective != "vector" {
            branches.push(tx.keyword_hits(&terms, false, asset_filter).await?);
        }
        let mut vectors = Vec::new();
        for (generation, native) in native_hits {
            if let Some(mut hit) = tx
                .vector_hit(native.id, native.source, profile, generation)
                .await?
            {
                hit.score = native.score as f64;
                vectors.push(hit);
            }
        }
        vectors.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then(a.chunk_id.cmp(&b.chunk_id))
        });
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
            let mut ids: HashSet<Uuid> = all_entities
                .iter()
                .filter(|e| {
                    terms
                        .split_whitespace()
                        .any(|term| e["name"].as_str().unwrap_or("").contains(term))
                })
                .take(20)
                .map(|e| decode(e["id"].clone()))
                .collect::<Result<_>>()?;
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
        let mut current = Vec::new();
        let mut identities = HashMap::new();
        for h in hits {
            if let Some(mut live) = verify.visible_hit(h.chunk_id).await? {
                live.score = h.score;
                if let std::collections::hash_map::Entry::Vacant(entry) =
                    identities.entry(live.asset_id)
                {
                    let asset = verify.asset_view(live.asset_id, None).await?;
                    let identity: Option<crate::memory_identity::MemoryIdentity> =
                        decode(asset["identity"].clone())?;
                    entry.insert(identity);
                }
                live.identity = identities[&live.asset_id].clone();
                if live.matches_identity(input.memory_identity.as_ref()) {
                    current.push(live);
                    if current.len() == input.limit {
                        break;
                    }
                }
            }
        }
        let hits = current;
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
        Ok(
            json!({"hits":hits,"memory_identity":input.memory_identity,"requested_mode":input.mode,"effective_mode":effective,"warnings":warnings,"embedding_profile":self.models.profile,"retrieval_policy":"local-scoped-exact-rrf60-v1","active_components":{"summaries":summaries_enabled,"graph":graph_enabled},"graph":{"entities":entities,"relations":relations}}),
        )
    }
    pub async fn resolve(&self, a: &AuthContext, input: ResolveInput) -> Result<Value> {
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

fn fuse(branches: Vec<Vec<SearchHit>>, hybrid: bool) -> Vec<SearchHit> {
    let mut merged: HashMap<Uuid, SearchHit> = HashMap::new();
    for branch in branches {
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
        let identity = json!({"kind":hit.kind,"identity":hit.identity,"normalization_status":hit.normalization_status()});
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
