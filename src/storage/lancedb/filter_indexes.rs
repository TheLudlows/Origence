//! Scalar indexes for the scope/generation/authorized-ID prefilter. They are
//! created once during publication. Unindexed append fragments are still read.
use lancedb::{
    Table,
    index::{Index, IndexConfig, IndexType, scalar::BTreeIndexBuilder},
};

use super::{StorageError, StorageResult};

const FILTERS: [(&str, &str); 4] = [
    ("filter_tenant_v1", "tenant"),
    ("filter_workspace_v1", "workspace"),
    ("filter_generation_v1", "generation"),
    ("filter_id_v1", "id"),
];

fn validate(
    indices: &[IndexConfig],
    native_name: &str,
    native_type: &IndexType,
    native_column: &str,
) -> StorageResult<()> {
    for index in indices {
        let valid = if index.name == native_name {
            &index.index_type == native_type && index.columns == [native_column]
        } else {
            FILTERS.iter().any(|(name, column)| {
                index.name == *name
                    && index.index_type == IndexType::BTree
                    && index.columns == [*column]
            })
        };
        if !valid {
            return Err(StorageError::Conflict(
                "incompatible native filter index".into(),
            ));
        }
    }
    Ok(())
}

pub(super) async fn ensure(
    table: &Table,
    native_name: &str,
    native_type: IndexType,
    native_column: &str,
) -> StorageResult<()> {
    let indices = table.list_indices().await.map_err(backend)?;
    validate(&indices, native_name, &native_type, native_column)?;
    for (name, column) in FILTERS {
        if !indices.iter().any(|index| index.name == name) {
            table
                .create_index(&[column], Index::BTree(BTreeIndexBuilder::default()))
                .name(name.into())
                .replace(false)
                .execute()
                .await
                .map_err(backend)?;
        }
    }
    Ok(())
}

/// Only the declared four scalar indexes and one correctly typed native index
/// are permitted. Missing native vector index is legitimate for small tables;
/// missing scalar indexes is a recoverable interrupted-publication state.
pub(super) async fn check(
    table: &Table,
    native_name: &str,
    native_type: IndexType,
    native_column: &str,
) -> StorageResult<bool> {
    let indices = table.list_indices().await.map_err(backend)?;
    validate(&indices, native_name, &native_type, native_column)?;
    if !FILTERS
        .iter()
        .all(|(name, _)| indices.iter().any(|index| index.name == *name))
    {
        return Err(StorageError::Unavailable(
            "native filter indexes are not ready".into(),
        ));
    }
    Ok(indices.iter().any(|index| index.name == native_name))
}

fn backend(error: lancedb::Error) -> StorageError {
    StorageError::Backend(error.to_string())
}
