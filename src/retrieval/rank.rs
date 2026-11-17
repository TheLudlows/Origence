//! Deterministic ranking of already-authorized branch hits; no storage or IO.
use crate::types::SearchHit;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

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

pub(super) fn fuse(branches: Vec<Vec<SearchHit>>, hybrid: bool) -> Vec<SearchHit> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hit(id: u128, score: f64) -> SearchHit {
        SearchHit {
            asset_id: Uuid::from_u128(1000 + id),
            version: 1,
            chunk_id: Uuid::from_u128(id),
            kind: "knowledge".into(),
            title: format!("hit {id}"),
            content: "verified evidence".into(),
            locator: json!({}),
            source_event_id: Uuid::from_u128(2000 + id),
            score,
            identity: None,
        }
    }

    #[test]
    fn branch_deduplicates_before_limit_and_breaks_score_ties_by_chunk() {
        let mut hits = vec![hit(3, 1.0), hit(2, 2.0), hit(1, 2.0), hit(1, 3.0)];
        prepare_branch(&mut hits, 2);
        assert_eq!(
            hits.iter()
                .map(|h| (h.chunk_id.as_u128(), h.score))
                .collect::<Vec<_>>(),
            vec![(1, 3.0), (2, 2.0)]
        );
        prepare_branch(&mut hits, 0);
        assert!(hits.is_empty());
    }

    #[test]
    fn fusion_preserves_rrf_votes_raw_scores_and_stable_ties() {
        let branches = vec![
            vec![hit(2, 4.0), hit(1, 8.0), hit(1, 8.0)],
            vec![hit(3, 4.0), hit(2, 8.0)],
        ];
        let hybrid = fuse(branches.clone(), true);
        assert_eq!(
            hybrid
                .iter()
                .map(|h| h.chunk_id.as_u128())
                .collect::<Vec<_>>(),
            vec![2, 1, 3]
        );
        assert_eq!(hybrid[0].score, 1.0 / 62.0 + 1.0 / 61.0);
        assert_eq!(hybrid[1].score, 1.0 / 61.0);
        assert_eq!(hybrid[2].score, 1.0 / 62.0);
        let raw = fuse(branches, false);
        assert_eq!(
            raw.iter().map(|h| h.score).collect::<Vec<_>>(),
            vec![12.0, 8.0, 4.0]
        );
        let ties = fuse(vec![vec![hit(2, 9.0)], vec![hit(1, 9.0)]], true);
        assert_eq!(ties[0].chunk_id.as_u128(), 1);
        let capped = fuse(vec![(1..=101).rev().map(|id| hit(id, 1.0)).collect()], true);
        assert_eq!(capped.len(), 100);
        assert_eq!(capped.last().unwrap().chunk_id.as_u128(), 100);
        assert!(fuse(Vec::new(), true).is_empty());
    }
}
