//! Frozen count-based routing, verified by the T0 0.40.0 probe. No result-count
//! heuristic, adaptive probe expansion, quantization, or all-partition ANN.
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
    let stats = table.index_stats(INDEX_NAME).await.map_err(backend)?;
    if !present
        || !stats.is_some_and(|stats| {
            stats.index_type == IndexType::IvfFlat && stats.distance_type == Some(DistanceType::L2)
        })
    {
        return Err(StorageError::Conflict("incompatible vector index".into()));
    }
    Ok(())
}

/// The table must already be pinned: routing counts and execution observe the
/// same physical snapshot. Missing or duplicate physical candidates force exact
/// search, and the relational authority still checks every returned candidate.
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
    use super::route_ann;

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
        let hits = store.search(query).await.unwrap();
        assert_eq!(hits.len(), 100);
        assert_eq!(hits[0].id, fresh.id);
        assert_eq!(hits[0].score, 1.0);
        assert!(hits.windows(2).all(|pair| pair[0].score >= pair[1].score));
    }
}
