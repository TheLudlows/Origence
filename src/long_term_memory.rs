//! Shared workspace admission and recall for factual and experiential memories.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    consolidation::{ConclusionState, MemoryView, RETAIN_SOURCE_KIND},
    context_retrieval::{CONTEXT_CANDIDATE_LIMIT, ContextRoute},
    error::{AppError, Result},
    evidence::{conditions_match, validate_applicability_context},
    learning::ImproveInput,
    memory_identity::MemoryIdentity,
    parsing,
    service::Service,
    session::{EvidenceKind, UsedEvidence},
    storage::{DomainTx, Permission, sqlite::now_ms},
    types::{AuthContext, CaptureInput},
};

const FACT_SCAN_LIMIT: usize = 512;
const MAX_FACT_QUERY_TERMS: usize = 32;
const MAX_RECALL_PER_KIND: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExperienceSource {
    #[schemars(with = "String")]
    pub session_id: Uuid,
    pub improve: ImproveInput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LongTermRetainInput {
    pub content: Option<String>,
    pub experience: Option<ExperienceSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LongTermRecallInput {
    pub query: String,
    pub identity: Option<MemoryIdentity>,
    pub as_of_ms: Option<i64>,
    pub limit: Option<usize>,
    #[serde(default)]
    pub applicability_context: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Applicability {
    CallerIdentityExactMatch,
    CallerContextExactMatch,
    Unverified,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FactCitation {
    pub source_event_id: Uuid,
    pub byte_start: usize,
    pub byte_end: usize,
    pub effective_at_ms: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecalledMemory {
    Fact {
        memory_id: Uuid,
        version: i32,
        identity: MemoryIdentity,
        state: ConclusionState,
        applicability: Applicability,
        citations: Vec<FactCitation>,
    },
    Experience {
        /// The stable ID of the published Learning.
        memory_id: Uuid,
        version: i32,
        statement: String,
        why: String,
        conditions: Vec<String>,
        applicability: Applicability,
        /// A reference to the published workspace chunk.
        citation: UsedEvidence,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LongTermRecall {
    pub as_of_ms: i64,
    pub memories: Vec<RecalledMemory>,
    /// The scoped fact scan ended at its limit and more evidence rows exist.
    pub fact_scan_exhausted: bool,
}

pub(crate) struct FactSearchRow {
    pub memory_id: Uuid,
    pub source_event_id: Uuid,
    pub quote: String,
}

#[derive(Default)]
struct FactMatch {
    score: usize,
    matched_sources: BTreeSet<Uuid>,
}

fn rank_facts(rows: Vec<FactSearchRow>, terms: &str) -> Vec<(Uuid, FactMatch)> {
    let query: BTreeSet<_> = terms
        .split_whitespace()
        .take(MAX_FACT_QUERY_TERMS)
        .collect();
    let mut matches = BTreeMap::<Uuid, FactMatch>::new();
    for row in rows {
        let lexical = parsing::lexical(&row.quote);
        let words: BTreeSet<_> = lexical.split_whitespace().collect();
        let score = query.intersection(&words).count();
        if score > 0 {
            let matched = matches.entry(row.memory_id).or_default();
            matched.score = matched.score.max(score);
            matched.matched_sources.insert(row.source_event_id);
        }
    }
    let mut ranked: Vec<_> = matches.into_iter().collect();
    ranked.sort_by(|(left_id, left), (right_id, right)| {
        right.score.cmp(&left.score).then(left_id.cmp(right_id))
    });
    ranked
}

fn recalled_fact(view: MemoryView, as_of_ms: i64, applicability: Applicability) -> RecalledMemory {
    let date = view
        .evidence
        .iter()
        .map(|evidence| evidence.effective_at_ms)
        .filter(|date| date.is_none_or(|date| date <= as_of_ms))
        .max();
    let citations = view
        .evidence
        .into_iter()
        .filter(|evidence| Some(evidence.effective_at_ms) == date)
        .map(|evidence| FactCitation {
            source_event_id: evidence.source_event_id,
            byte_start: evidence.statement.byte_start,
            byte_end: evidence.statement.byte_end,
            effective_at_ms: evidence.effective_at_ms,
        })
        .collect();
    RecalledMemory::Fact {
        memory_id: view.memory_id,
        version: view.version,
        identity: view.identity,
        state: view.state,
        applicability,
        citations,
    }
}

impl Service {
    /// Submit one source into the shared long-term memory workflow. The writer
    /// chooses the approved source; a private session still requires its owner.
    pub async fn retain_long_term_memory(
        &self,
        auth: &AuthContext,
        key: &str,
        input: LongTermRetainInput,
    ) -> Result<Value> {
        match (input.content, input.experience) {
            (Some(content), None) => {
                self.retain_memory(auth, key, CaptureInput { content })
                    .await
            }
            (None, Some(source)) => {
                self.admit_improve(
                    auth,
                    key,
                    RETAIN_SOURCE_KIND,
                    source.session_id,
                    source.improve,
                )
                .await
            }
            _ => Err(AppError::Invalid(
                "submit exactly one of content or experience".into(),
            )),
        }
    }

    /// Retrieve both memory kinds with one query and one final authorized
    /// snapshot. Text matches discover candidates; only caller conditions can
    /// establish exact applicability matches.
    pub async fn recall_long_term_memories(
        &self,
        auth: &AuthContext,
        input: LongTermRecallInput,
    ) -> Result<LongTermRecall> {
        let limit = input.limit.unwrap_or(10);
        if !(1..=MAX_RECALL_PER_KIND).contains(&limit) {
            return Err(AppError::Invalid(
                "memory recall limit must be 1 to 20".into(),
            ));
        }
        validate_applicability_context(&input.applicability_context)
            .map_err(|error| AppError::Invalid(error.into()))?;
        if let Some(identity) = &input.identity {
            identity
                .validate()
                .map_err(|_| AppError::Invalid("invalid memory identity".into()))?;
        }
        let as_of_ms = input.as_of_ms.unwrap_or_else(now_ms);
        let query = self
            .prepare_context_query(auth, &input.query, "keyword", false)
            .await?;
        let (ranked_facts, fact_scan_exhausted) =
            if input.identity.is_none() && !query.terms.is_empty() {
                let mut tx = self.read(auth, Permission::Read).await?;
                let mut rows = tx.long_term_fact_search_rows(FACT_SCAN_LIMIT + 1).await?;
                tx.commit().await?;
                let exhausted = rows.len() > FACT_SCAN_LIMIT;
                rows.truncate(FACT_SCAN_LIMIT);
                let terms = query.terms.clone();
                let ranked = tokio::task::spawn_blocking(move || rank_facts(rows, &terms))
                    .await
                    .map_err(|error| AppError::Internal(error.into()))?;
                (ranked, exhausted)
            } else {
                (Vec::new(), false)
            };
        let experiences = self
            .context_candidates(
                auth,
                &query,
                ContextRoute::Learning,
                None,
                CONTEXT_CANDIDATE_LIMIT,
            )
            .await?;
        let mut tx = self.read(auth, Permission::Read).await?;
        tx.verify_native_candidates(&experiences.ranked_native)
            .await?;
        let mut memories = Vec::new();
        if let Some(identity) = &input.identity {
            if let Some(view) = tx.long_term_memory(identity, None, as_of_ms).await? {
                memories.push(recalled_fact(
                    view,
                    as_of_ms,
                    Applicability::CallerIdentityExactMatch,
                ));
            }
        } else {
            for (id, matched) in ranked_facts {
                if let Some(view) = tx.long_term_memory_by_id(id, None, as_of_ms).await?
                    && matched.matched_sources.iter().all(|source| {
                        view.evidence
                            .iter()
                            .any(|evidence| evidence.source_event_id == *source)
                    })
                {
                    memories.push(recalled_fact(view, as_of_ms, Applicability::Unverified));
                    if memories.len() == limit {
                        break;
                    }
                }
            }
        }
        let mut seen = BTreeSet::new();
        for candidate in experiences.candidates {
            let fresh = tx.refresh_context_candidate(candidate, None).await?;
            let learning = fresh
                .learning
                .ok_or_else(|| AppError::Conflict("experience metadata unavailable".into()))?;
            if seen.insert(learning.learning_id) {
                let applicability =
                    if conditions_match(&learning.conditions, &input.applicability_context) {
                        Applicability::CallerContextExactMatch
                    } else {
                        Applicability::Unverified
                    };
                memories.push(RecalledMemory::Experience {
                    memory_id: learning.learning_id,
                    version: fresh.hit.version,
                    statement: learning.statement,
                    why: learning.why,
                    conditions: learning.conditions,
                    applicability,
                    citation: UsedEvidence {
                        kind: EvidenceKind::Chunk,
                        id: fresh.hit.chunk_id,
                        asset_id: fresh.hit.asset_id,
                        asset_version: fresh.hit.version,
                        chunk_id: fresh.hit.chunk_id,
                    },
                });
                if seen.len() == limit {
                    break;
                }
            }
        }
        tx.commit().await?;
        Ok(LongTermRecall {
            as_of_ms,
            memories,
            fact_scan_exhausted,
        })
    }
}
