//! Independent, current-only recall for the evidence bundle.
//!
//! These routes deliberately use original chunks only. Query embedding is
//! prepared once, then shared by every enabled document route. No summary,
//! graph, publication or background work is introduced here.

use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    error::{AppError, Result},
    learning::SupportField,
    memory_identity::MemoryIdentity,
    parsing,
    retrieval::{NativePublicationProof, NativeRankedCandidate},
    service::{Service, scope},
    session::ArchivedEvidence,
    storage::{DomainTx, FullTextQuery, Permission, VectorQuery},
    types::{AuthContext, SearchHit},
};

/// Separate candidate pools, never a label applied after a shared top-k.
#[derive(
    Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ContextRoute {
    Memory,
    Knowledge,
    Learning,
}

pub(crate) const CONTEXT_CANDIDATE_LIMIT: usize = 100;
pub(crate) const SESSION_SCAN_LIMIT: usize = 512;

/// One query embedding and one explicit fallback decision for all routes.
pub(crate) struct ContextQuery {
    pub terms: String,
    pub requested_mode: String,
    pub effective_mode: String,
    pub warnings: Vec<String>,
    embedding: Option<Vec<f32>>,
}

/// Reader-safe support metadata. Archived question/answer and verbatim quotes
/// are intentionally absent; raw session material requires Write permission.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ContextLearningSupport {
    pub turn_id: Uuid,
    pub field: SupportField,
    pub byte_start: usize,
    pub byte_end: usize,
    pub feedback_id: Uuid,
    pub principal_id: Uuid,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ContextLearning {
    pub learning_id: Uuid,
    pub improve_id: Uuid,
    pub session_id: Uuid,
    pub statement: String,
    pub why: String,
    pub conditions: Vec<String>,
    pub supports: Vec<ContextLearningSupport>,
}

#[derive(Clone, Debug)]
pub(crate) struct ContextCandidate {
    pub route: ContextRoute,
    pub hit: SearchHit,
    pub version_root_source_event_id: Uuid,
    pub source_version: i32,
    pub learning: Option<ContextLearning>,
    pub retrieval_routes: Vec<String>,
    /// Only deterministic S2 single-valued assertions/conflicts qualify.
    pub single_valued: bool,
    pub semantic_identity: Option<MemoryIdentity>,
    pub semantic_value: Option<Value>,
}

pub(crate) struct ContextCandidateResult {
    pub candidates: Vec<ContextCandidate>,
    pub ranked_native: Vec<NativeRankedCandidate>,
}

#[derive(Clone, Debug)]
pub(crate) struct ContextSessionTurn {
    pub session_id: Uuid,
    pub turn_id: Uuid,
    pub source_event_id: Uuid,
    pub question: String,
    pub answer: String,
    pub evidence: Vec<ArchivedEvidence>,
    pub created_at: i64,
    pub score: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct ContextSessionResult {
    pub turns: Vec<ContextSessionTurn>,
    pub scanned: usize,
    /// More scoped rows exist beyond the fixed recent-turn scan bound.
    pub scan_exhausted: bool,
}

impl Service {
    pub(crate) async fn prepare_context_query(
        &self,
        auth: &AuthContext,
        query: &str,
        mode: &str,
        allow_partial: bool,
    ) -> Result<ContextQuery> {
        if query.trim().is_empty()
            || query.len() > 4000
            || query.contains('\0')
            || !["keyword", "vector", "hybrid"].contains(&mode)
        {
            return Err(AppError::Invalid("query or mode invalid".into()));
        }
        // Reject revoked/unauthorized callers before external model IO.
        self.read(auth, Permission::Read).await?.rollback().await?;
        let mut prepared = ContextQuery {
            terms: parsing::lexical(query),
            requested_mode: mode.into(),
            effective_mode: mode.into(),
            warnings: Vec::new(),
            embedding: None,
        };
        if mode != "keyword" {
            match self.models.embed(query).await {
                Ok(embedding) => prepared.embedding = Some(embedding),
                Err(_) if allow_partial => {
                    prepared.effective_mode = "keyword".into();
                    prepared
                        .warnings
                        .push("embedding unavailable; used keyword retrieval".into());
                }
                Err(error) => return Err(error),
            }
        }
        Ok(prepared)
    }

    pub(crate) async fn context_candidates(
        &self,
        auth: &AuthContext,
        query: &ContextQuery,
        route: ContextRoute,
        identity: Option<&MemoryIdentity>,
        limit: usize,
    ) -> Result<ContextCandidateResult> {
        if !(1..=CONTEXT_CANDIDATE_LIMIT).contains(&limit) {
            return Err(AppError::Invalid("context candidate limit invalid".into()));
        }
        // A requested exact identity narrows only the Memory route.
        let identity = identity.filter(|_| route == ContextRoute::Memory);
        if let Some(identity) = identity {
            identity
                .validate()
                .map_err(|_| AppError::Invalid("invalid memory identity filter".into()))?;
        }
        let mut tx = self.read(auth, Permission::Read).await?;
        let profile = self.models.profile.as_deref().unwrap_or("");
        let vector_dimension = query.embedding.as_ref().map_or(0, Vec::len);
        let mut eligible = Vec::new();
        if let Some(vector) = &query.embedding {
            for generation in tx.vector_generations(profile, vector.len()).await? {
                let ids = tx
                    .context_vector_candidates(profile, vector.len(), generation, route, identity)
                    .await?;
                let sources = tx.native_candidate_sources(&ids).await?;
                eligible.push((generation, ids, sources));
            }
        }
        let mut full_text_eligible = Vec::new();
        if query.effective_mode != "vector" && !query.terms.is_empty() {
            for generation in tx.full_text_generations().await? {
                let ids = tx
                    .context_full_text_candidates(generation, route, identity)
                    .await?;
                let sources = tx.native_candidate_sources(&ids).await?;
                full_text_eligible.push((generation, ids, sources));
            }
        }
        tx.commit().await?;
        let mut full_text_hits = Vec::new();
        for (generation, ids, sources) in full_text_eligible {
            if ids.is_empty() {
                continue;
            }
            for hit in self
                .engine
                .vector()
                .search_full_text_authorized(
                    FullTextQuery {
                        scope: scope(auth),
                        artifact_ids: Some(ids),
                        generation,
                        search_terms: query.terms.clone(),
                        limit: CONTEXT_CANDIDATE_LIMIT,
                    },
                    &sources,
                )
                .await?
            {
                full_text_hits.push((generation, hit));
            }
        }

        let mut native_hits = Vec::new();
        if let Some(vector) = &query.embedding {
            for (generation, ids, sources) in eligible {
                if !ids.is_empty() {
                    for hit in self
                        .engine
                        .vector()
                        .search_authorized(
                            VectorQuery {
                                scope: scope(auth),
                                artifact_ids: Some(ids),
                                profile: profile.into(),
                                dimension: vector.len(),
                                generation,
                                embedding: vector.clone(),
                                limit: CONTEXT_CANDIDATE_LIMIT,
                            },
                            &sources,
                        )
                        .await?
                    {
                        native_hits.push((generation, hit));
                    }
                }
            }
        }

        // Native IO is never authority. Re-read in a fresh authorized snapshot
        // and reject stale, cross-route or newly withdrawn candidates.
        let mut tx = self.read(auth, Permission::Read).await?;
        let mut branches = Vec::new();
        let mut ranked_native = Vec::new();
        if query.effective_mode != "vector" {
            let mut keyword = Vec::new();
            let mut seen = HashMap::new();
            for (generation, native) in full_text_hits {
                let candidate = tx
                    .full_text_hit(native.id, native.source, generation)
                    .await?;
                if let Some(mut hit) =
                    crate::retrieval::live_native_candidate(&mut tx, native.id, candidate).await?
                {
                    if !tx.context_route_matches(hit.asset_id, route).await?
                        || !tx.context_identity_matches(&hit, identity).await?
                    {
                        return Err(AppError::Conflict(
                            "native context candidate route changed".into(),
                        ));
                    }
                    if !crate::retrieval::unique_native(
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
            crate::retrieval::prepare_branch(&mut keyword, CONTEXT_CANDIDATE_LIMIT);
            branches.push(("keyword", keyword));
        }
        if query.effective_mode != "keyword" {
            let mut vectors = Vec::new();
            let mut seen = HashMap::new();
            for (generation, native) in native_hits {
                let candidate = tx
                    .vector_hit_exact(
                        native.id,
                        native.source,
                        profile,
                        vector_dimension,
                        generation,
                    )
                    .await?;
                if let Some(mut hit) =
                    crate::retrieval::live_native_candidate(&mut tx, native.id, candidate).await?
                {
                    if !tx.context_route_matches(hit.asset_id, route).await?
                        || !tx.context_identity_matches(&hit, identity).await?
                    {
                        return Err(AppError::Conflict(
                            "native context candidate route changed".into(),
                        ));
                    }
                    if !crate::retrieval::unique_native(
                        &mut seen,
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
            crate::retrieval::prepare_branch(&mut vectors, CONTEXT_CANDIDATE_LIMIT);
            branches.push(("vector", vectors));
        }
        let fused = fuse_context_branches(branches, query.effective_mode == "hybrid");
        let mut candidates = Vec::new();
        for (hit, origins) in fused {
            if let Some(mut candidate) = tx.context_candidate(hit.chunk_id, route, identity).await?
            {
                candidate.hit.score = hit.score;
                candidate.retrieval_routes = origins;
                candidates.push(candidate);
                if candidates.len() == limit {
                    break;
                }
            } else {
                return Err(AppError::Conflict(
                    "native context candidate route changed".into(),
                ));
            }
        }
        tx.commit().await?;
        Ok(ContextCandidateResult {
            candidates,
            ranked_native,
        })
    }

    pub(crate) async fn context_session_candidates(
        &self,
        auth: &AuthContext,
        query: &str,
        session_id: Option<Uuid>,
        exclude_session_id: Option<Uuid>,
        limit: usize,
    ) -> Result<ContextSessionResult> {
        if limit == 0
            || limit > CONTEXT_CANDIDATE_LIMIT
            || query.len() > 4000
            || query.contains('\0')
            || session_id.is_some_and(|id| id.is_nil())
            || exclude_session_id.is_some_and(|id| id.is_nil())
        {
            return Err(AppError::Invalid("session recall input invalid".into()));
        }
        let mut tx = self.read(auth, Permission::Write).await?;
        let result = tx
            .context_recent_turns(
                &parsing::lexical(query),
                session_id,
                exclude_session_id,
                limit,
            )
            .await?;
        tx.commit().await?;
        Ok(result)
    }
}

fn hit_order(a: &SearchHit, b: &SearchHit) -> std::cmp::Ordering {
    b.score
        .total_cmp(&a.score)
        .then(a.chunk_id.cmp(&b.chunk_id))
}

fn fuse_context_branches(
    branches: Vec<(&str, Vec<SearchHit>)>,
    hybrid: bool,
) -> Vec<(SearchHit, Vec<String>)> {
    let mut merged: HashMap<Uuid, (SearchHit, Vec<String>)> = HashMap::new();
    for (origin, mut branch) in branches {
        crate::retrieval::prepare_branch(&mut branch, CONTEXT_CANDIDATE_LIMIT);
        for (rank, mut hit) in branch.into_iter().enumerate() {
            if hybrid {
                hit.score = 1.0 / (60.0 + rank as f64 + 1.0);
            }
            merged
                .entry(hit.chunk_id)
                .and_modify(|(old, origins)| {
                    old.score += hit.score;
                    origins.push(origin.into());
                })
                .or_insert((hit, vec![origin.into()]));
        }
    }
    let mut hits: Vec<_> = merged.into_values().collect();
    hits.sort_by(|a, b| hit_order(&a.0, &b.0));
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hit(id: u128, content: &str) -> SearchHit {
        SearchHit {
            asset_id: Uuid::from_u128(id + 100),
            version: 1,
            chunk_id: Uuid::from_u128(id),
            kind: "memory".into(),
            title: String::new(),
            content: content.into(),
            locator: json!({}),
            source_event_id: Uuid::from_u128(id + 200),
            score: 1.0,
            identity: None,
        }
    }

    #[test]
    fn fusion_keeps_distinct_evidence_and_records_both_origins() {
        let first = hit(1, "enabled");
        let opposite = hit(2, "disabled");
        let fused = fuse_context_branches(
            vec![
                ("keyword", vec![first.clone(), opposite]),
                ("vector", vec![first]),
            ],
            true,
        );
        assert_eq!(fused.len(), 2);
        assert_eq!(fused[0].0.chunk_id, Uuid::from_u128(1));
        assert_eq!(fused[0].1, ["keyword", "vector"]);
        assert_eq!(fused[1].0.content, "disabled");
    }
}
