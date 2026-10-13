//! Frozen count-based routing, verified by the T0 0.40.0 probe. No result-count
//! heuristic, adaptive probe expansion, quantization, or all-partition ANN.
use std::collections::HashSet;

use lance::index::DatasetIndexExt;
use lancedb::{
    DistanceType, Table,
    index::{Index, IndexType, vector::IvfFlatIndexBuilder},
    query::{HasQuery, QueryBase, VectorQuery as NativeVectorQuery},
};

use super::{StorageError, StorageResult, VectorQuery, filter_indexes};

pub(super) const INDEX_NAME: &str = "vector_l2_ivf64_probe32_v1";
const PARTITIONS: u32 = 64;
const PROBES: usize = 32;
const MINIMUM_ELIGIBLE: usize = 4096;
const CANDIDATE_LIMIT: usize = 100;

fn route_ann(eligible: usize, snapshot_rows: usize) -> bool {
    eligible >= MINIMUM_ELIGIBLE
        && eligible <= snapshot_rows
        && (eligible as u128) * 5 >= (snapshot_rows as u128) * 2
}

/// Build only during the serialized publication write, never from a read or
/// a background maintenance loop. Appends remain searchable without refresh.
pub(super) async fn ensure_index(table: &Table) -> StorageResult<()> {
    filter_indexes::ensure(table, INDEX_NAME, IndexType::IvfFlat, "vector").await?;
    let indices = table.list_indices().await.map_err(backend)?;
    if indices.iter().any(|index| index.name == INDEX_NAME) {
        return check_index(table).await;
    }
    if table.count_rows(None).await.map_err(backend)? < MINIMUM_ELIGIBLE {
        return Ok(());
    }
    table
        .create_index(
            &["vector"],
            Index::IvfFlat(
                IvfFlatIndexBuilder::default()
                    .distance_type(DistanceType::L2)
                    .num_partitions(PARTITIONS),
            ),
        )
        .name(INDEX_NAME.into())
        .replace(false)
        .execute()
        .await
        .map_err(backend)?;
    check_index(table).await
}

async fn check_index(table: &Table) -> StorageResult<()> {
    let present = filter_indexes::check(table, INDEX_NAME, IndexType::IvfFlat, "vector").await?;
    if !present {
        return Err(StorageError::Conflict("incompatible vector index".into()));
    }
    // Guard legacy/corrupt coverage before either statistics API: LanceDB's
    // summary may itself fall back to Lance's raw statistics implementation.
    check_physical_partitions(table).await?;
    let stats = table.index_stats(INDEX_NAME).await.map_err(backend)?;
    if !stats.is_some_and(|stats| {
        stats.index_type == IndexType::IvfFlat && stats.distance_type == Some(DistanceType::L2)
    }) {
        return Err(StorageError::Conflict("incompatible vector index".into()));
    }
    Ok(())
}

/// LanceDB's summary stats and manifest details omit the physical IVF count.
/// Inspect the actual segment statistics through the public Lance API instead.
async fn check_physical_partitions(table: &Table) -> StorageResult<()> {
    let dataset = table.dataset()
        .ok_or_else(|| StorageError::Unavailable("native vector dataset is unavailable".into()))?
        .get().await.map_err(backend)?;
    let segments = dataset.load_indices_by_name(INDEX_NAME).await
        .map_err(|error| StorageError::Backend(error.to_string()))?;
    if segments.is_empty() {
        return Err(StorageError::Conflict("vector index has no physical segments".into()));
    }
    // index_statistics can auto-migrate legacy fragment metadata. Refuse both
    // triggers before calling it, using this exact immutable dataset snapshot.
    // This adapter must never turn a read into repair or a format migration.
    let mut covered = HashSet::new();
    for segment in &segments {
        let bitmap = segment.fragment_bitmap.as_ref()
            .ok_or_else(|| StorageError::Conflict("vector index coverage is missing".into()))?;
        for fragment in dataset.fragments().iter() {
            let id = u32::try_from(fragment.id)
                .map_err(|_| StorageError::Conflict("invalid vector fragment identity".into()))?;
            if bitmap.contains(id) {
                let rows = fragment.physical_rows.and_then(|physical| {
                    match fragment.deletion_file.as_ref() {
                        None => Some(physical),
                        Some(deletion) => deletion.num_deleted_rows
                            .and_then(|deleted| physical.checked_sub(deleted)),
                    }
                });
                if rows.is_none() || !covered.insert(fragment.id) {
                    return Err(StorageError::Conflict("incompatible vector fragment metadata".into()));
                }
            }
        }
    }
    let raw = dataset.index_statistics(INDEX_NAME).await
        .map_err(|error| StorageError::Backend(error.to_string()))?;
    validate_partition_statistics(&raw, segments.len())
}

fn validate_partition_statistics(raw: &str, expected_segments: usize) -> StorageResult<()> {
    #[derive(serde::Deserialize)]
    struct SegmentStatistics {
        num_partitions: u64,
    }
    #[derive(serde::Deserialize)]
    struct Statistics {
        indices: Vec<SegmentStatistics>,
    }
    let statistics: Statistics = serde_json::from_str(raw)
        .map_err(|_| StorageError::Conflict("vector partition statistics are unavailable".into()))?;
    if expected_segments == 0
        || statistics.indices.len() != expected_segments
        || statistics.indices.iter().any(|segment| segment.num_partitions != u64::from(PARTITIONS))
    {
        return Err(StorageError::Conflict("vector index must have 64 partitions per segment".into()));
    }
    Ok(())
}

/// The table must already be pinned: routing counts and execution observe the
/// same physical snapshot. The preceding metadata preflight rejects missing or
/// duplicate candidates; authority checks still gate every returned candidate.
pub(super) async fn prepare(
    table: &Table,
    query: &VectorQuery,
    filter: &str,
) -> StorageResult<(NativeVectorQuery, bool)> {
    let eligible = query.artifact_ids.as_ref().map(|ids| ids.len());
    let mut ann = false;
    if query.limit == CANDIDATE_LIMIT
        && eligible.is_some_and(|count| count >= MINIMUM_ELIGIBLE)
        && table
            .list_indices()
            .await
            .map_err(backend)?
            .iter()
            .any(|index| index.name == INDEX_NAME)
    {
        check_index(table).await?;
        let snapshot_rows = table.count_rows(None).await.map_err(backend)?;
        let eligible = eligible.unwrap_or(0);
        if route_ann(eligible, snapshot_rows) {
            ann = table
                .count_rows(Some(filter.into()))
                .await
                .map_err(backend)?
                == eligible;
        }
    }
    let mut native = table
        .query()
        .nearest_to(query.embedding.as_slice())
        .map_err(backend)?
        .distance_type(DistanceType::L2)
        .only_if(filter)
        .limit(query.limit);
    // These are public request fields in 0.40.0; defaults are not the contract.
    native.mut_query().prefilter = true;
    native.mut_query().fast_search = false;
    native = if ann {
        native.nprobes(PROBES)
    } else {
        native.bypass_vector_index()
    };
    Ok((native, ann))
}

fn backend(error: lancedb::Error) -> StorageError {
    StorageError::Backend(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{route_ann, validate_partition_statistics};

    #[test]
    fn physical_partition_statistics_require_every_segment_to_match() {
        assert!(validate_partition_statistics(r#"{"indices":[{"num_partitions":64}]}"#, 1).is_ok());
        assert!(validate_partition_statistics(r#"{"indices":[{"num_partitions":64},{"num_partitions":64}]}"#, 2).is_ok());
        for (raw, segments) in [
            (r#"{"indices":[{"num_partitions":32}]}"#, 1),
            (r#"{"indices":[{"num_partitions":128}]}"#, 1),
            (r#"{"indices":[{"num_partitions":64},{"num_partitions":32}]}"#, 2),
            (r#"{"indices":[{}]}"#, 1),
            (r#"{"indices":[]}"#, 0),
            (r#"{"num_indices":64}"#, 1),
            (r#"{"indices":[{"num_partitions":64}]}"#, 2),
        ] {
            assert!(validate_partition_statistics(raw, segments).is_err(), "{raw}");
        }
    }

    #[test]
    fn frozen_policy_boundaries_and_overflow() {
        assert!(!route_ann(0, 10000));
        assert!(!route_ann(4095, 10000));
        assert!(route_ann(4096, 10000));
        assert!(route_ann(4097, 10000));
        assert!(!route_ann(4096, 20000));
        assert!(route_ann(4096, 10240));
        assert!(!route_ann(4096, 10241));
        assert!(!route_ann(4096, 0));
        assert!(route_ann(usize::MAX, usize::MAX));
    }
}

#[cfg(test)]
mod native_tests {
    use super::*;
    use crate::storage::{
        LanceDbStore, Scope, SourceVersion, VectorEntry, VectorStore,
        lancedb::{scoped_filter, table_name},
    };
    use lancedb::query::ExecutableQuery;
    use uuid::Uuid;

    #[tokio::test]
    async fn frozen_native_ann_and_exact_requests_preserve_unindexed_rows() {
        let dir = tempfile::tempdir().unwrap();
        let store = LanceDbStore::open(dir.path()).await.unwrap();
        let scope = Scope {
            tenant_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
        };
        let source = SourceVersion {
            source_id: Uuid::new_v4(),
            version: 1,
        };
        let mut state = 52_u64;
        let entries: Vec<_> = (0..4096)
            .map(|id| VectorEntry {
                id: Uuid::from_u128(id + 1),
                source,
                generation: 1,
                profile: "frozen-policy:v1".into(),
                dimension: 128,
                embedding: (0..128)
                    .map(|_| {
                        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                        ((state >> 40) as f32) / 16777216.0
                    })
                    .collect(),
            })
            .collect();
        store.upsert(scope, entries.clone()).await.unwrap();
        let ids: Vec<_> = entries.iter().map(|entry| entry.id).collect();
        let mut query = VectorQuery {
            scope,
            artifact_ids: Some(ids),
            generation: 1,
            profile: entries[0].profile.clone(),
            dimension: 128,
            embedding: entries[0].embedding.clone(),
            limit: 100,
        };
        let table = store
            .open_existing(&table_name(&query.profile))
            .await
            .unwrap()
            .unwrap();
        table
            .checkout(table.version().await.unwrap())
            .await
            .unwrap();
        let filter = scoped_filter(scope, 1, query.artifact_ids.as_deref());
        let (ann, selected) = prepare(&table, &query, &filter).await.unwrap();
        assert!(selected);
        let request = ann.clone().into_request();
        assert!(request.base.prefilter && !request.base.fast_search && request.use_index);
        assert_eq!(request.minimum_nprobes, 32);
        assert_eq!(request.maximum_nprobes, Some(32));
        assert_eq!(request.distance_type, Some(DistanceType::L2));
        let plan = ann.explain_plan(true).await.unwrap();
        assert!(plan.contains("ANNIVF") || plan.contains("ANNIvf"), "{plan}");
        query.artifact_ids.as_mut().unwrap().pop();
        let filter = scoped_filter(scope, 1, query.artifact_ids.as_deref());
        let (exact, selected) = prepare(&table, &query, &filter).await.unwrap();
        assert!(!selected);
        assert!(!exact.clone().into_request().use_index);
        assert!(
            exact
                .explain_plan(true)
                .await
                .unwrap()
                .contains("KNNVectorDistance")
        );
        query.artifact_ids = None;
        assert!(
            !prepare(&table, &query, &scoped_filter(scope, 1, None))
                .await
                .unwrap()
                .1
        );
        drop(table);
        // Fresh appended data is deliberately not added to the existing index.
        let fresh = VectorEntry {
            id: Uuid::from_u128(100000),
            embedding: vec![2.0; 128],
            ..entries[0].clone()
        };
        store.upsert(scope, vec![fresh.clone()]).await.unwrap();
        query.artifact_ids = Some(
            entries
                .iter()
                .map(|entry| entry.id)
                .chain([fresh.id])
                .collect(),
        );
        query.embedding = fresh.embedding;
        let hits = store.search(query.clone()).await.unwrap();
        assert_eq!(hits.len(), 100);
        assert_eq!(hits[0].id, fresh.id);
        assert_eq!(hits[0].score, 1.0);
        assert!(hits.windows(2).all(|pair| pair[0].score >= pair[1].score));

        // A same-name, same-metric replacement must not silently change the
        // frozen 32-of-64 policy into a full-partition 32-of-32 search.
        let table = store.open_existing(&table_name(&query.profile)).await.unwrap().unwrap();
        table.create_index(&["vector"], Index::IvfFlat(
            IvfFlatIndexBuilder::default().distance_type(DistanceType::L2).num_partitions(32),
        )).name(INDEX_NAME.into()).replace(true).execute().await.unwrap();
        assert!(matches!(store.search(query).await, Err(StorageError::Conflict(_))));
    }
}
