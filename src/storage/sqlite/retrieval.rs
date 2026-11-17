//! Final publication and response checks inside the caller's scoped snapshot.
use super::SqliteTx;
use crate::{
    error::{AppError, Result},
    storage::SourceVersion,
    types::SearchHit,
};
use std::collections::HashMap;
use uuid::Uuid;

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

pub(crate) fn batch_native_candidate(
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

impl SqliteTx {
    /// Recheck all native ranking participants before returning any previously
    /// computed score. A disappeared participant makes that ranking unavailable;
    /// a live object with changed publication identity is a storage conflict.
    /// No native IO, new ranking, retry loop or cross-store snapshot is implied.
    pub(crate) async fn verify_native_candidates(
        &mut self,
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
        let full_text_hits = self.full_text_hits(&full_text).await?;
        let mut vector_hits = HashMap::new();
        for ((profile, dimension), keys) in vectors {
            vector_hits.insert(
                (profile.clone(), dimension),
                self.vector_hits_exact(&keys, &profile, dimension).await?,
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
        let live_missing = self.visible_hits(&missing).await?;
        for (candidate, current) in candidates.iter().zip(checked) {
            let current = batch_native_candidate(candidate.hit.chunk_id, current, &live_missing)?
                .ok_or_else(|| {
                AppError::Unavailable("native retrieval became unavailable".into())
            })?;
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
        &mut self,
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
            let live_hits = self.visible_hits(&ids).await?;
            let mut selected = Vec::new();
            for h in &hits[cursor..end] {
                if let Some(mut live) = live_hits.get(&h.chunk_id).cloned() {
                    live.score = h.score;
                    selected.push(live);
                }
            }
            let assets: Vec<_> = selected.iter().map(|hit| hit.asset_id).collect();
            let identities = self.retrieval_identities(&assets).await?;
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
}
