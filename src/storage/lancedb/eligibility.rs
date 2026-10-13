//! Metadata-only consistency check before native top-k. The relationally
//! authorized ID set must map one-to-one to physical rows in this snapshot.
use std::collections::{HashMap, HashSet};

use arrow_array::{Array, Int32Array, Int64Array, StringArray};
use futures::TryStreamExt;
use lancedb::{Table, query::{ExecutableQuery, QueryBase, Select}};
use uuid::Uuid;

use super::{SourceVersion, StorageError, StorageResult};

/// Reads at most eligible_count + 1 projected identity rows, never embeddings or
/// text. Even identical physical duplicates fail closed: folding this metadata
/// alone would still let duplicate native rows consume the later top-k budget.
pub(super) async fn check(
    table: &Table,
    filter: &str,
    generation: i64,
    allowed: Option<&[Uuid]>,
    expected_sources: Option<&HashMap<Uuid, SourceVersion>>,
) -> StorageResult<()> {
    let Some(allowed) = allowed else {
        return if expected_sources.is_some() {
            Err(StorageError::InvalidArgument("authorized native query requires candidate IDs"))
        } else {
            Ok(())
        };
    };
    if allowed.is_empty() {
        return Ok(());
    }
    let limit = allowed.len().checked_add(1)
        .ok_or(StorageError::InvalidArgument("too many native candidates"))?;
    let mut stream = table.query()
        .only_if(filter)
        .select(Select::columns(&["id", "source_id", "version", "generation"]))
        .limit(limit)
        .execute()
        .await
        .map_err(|error| StorageError::Backend(error.to_string()))?;
    let mut seen = HashSet::with_capacity(allowed.len());
    while let Some(batch) = stream.try_next().await
        .map_err(|error| StorageError::Backend(error.to_string()))?
    {
        let strings = |name| -> StorageResult<&StringArray> {
            batch.column_by_name(name)
                .and_then(|column| column.as_any().downcast_ref::<StringArray>())
                .ok_or_else(|| StorageError::Conflict("invalid native candidate identity".into()))
        };
        let ids = strings("id")?;
        let sources = strings("source_id")?;
        let versions = batch.column_by_name("version")
            .and_then(|column| column.as_any().downcast_ref::<Int32Array>())
            .ok_or_else(|| StorageError::Conflict("invalid native candidate version".into()))?;
        let generations = batch.column_by_name("generation")
            .and_then(|column| column.as_any().downcast_ref::<Int64Array>())
            .ok_or_else(|| StorageError::Conflict("invalid native candidate generation".into()))?;
        for row in 0..batch.num_rows() {
            if ids.is_null(row) || sources.is_null(row) || versions.is_null(row)
                || generations.is_null(row) || generations.value(row) != generation
                || Uuid::parse_str(sources.value(row)).is_err()
            {
                return Err(StorageError::Conflict("invalid native candidate identity".into()));
            }
            let id = Uuid::parse_str(ids.value(row))
                .map_err(|_| StorageError::Conflict("invalid native candidate identity".into()))?;
            if allowed.binary_search(&id).is_err() || !seen.insert(id) {
                return Err(StorageError::Conflict("ambiguous native candidate identity".into()));
            }
            if let Some(expected_sources) = expected_sources {
                let actual = SourceVersion {
                    source_id: Uuid::parse_str(sources.value(row))
                        .map_err(|_| StorageError::Conflict("invalid native candidate source".into()))?,
                    version: versions.value(row),
                };
                if expected_sources.get(&id) != Some(&actual) {
                    return Err(StorageError::Conflict("native candidate source differs from authority".into()));
                }
            }
        }
    }
    if seen.len() != allowed.len() {
        return Err(StorageError::Unavailable("native index candidates are missing".into()));
    }
    Ok(())
}
