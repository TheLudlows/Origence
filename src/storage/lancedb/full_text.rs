//! Original-chunk BM25 over application-tokenized terms. Derived rows remain
//! hidden until the relational ledger commits their full publication identity.
use std::{collections::HashMap, sync::Arc};

use arrow_array::{
    Array, Float32Array, Int32Array, Int64Array, RecordBatch, RecordBatchIterator,
    StringArray,
};
use arrow_schema::{DataType, Field, Schema};
use futures::TryStreamExt;
use lancedb::{
    Table,
    database::CreateTableMode,
    index::{
        Index, IndexType,
        scalar::{FtsIndexBuilder, FullTextSearchQuery, MatchQuery, Operator},
    },
    query::{ExecutableQuery, HasQuery, QueryBase},
};
use uuid::Uuid;

use super::{LanceDbStore, filter_indexes, scoped_filter};
use crate::storage::{
    capabilities::{FullTextEntry, FullTextHit, FullTextQuery},
    error::{StorageError, StorageResult},
    scope::{Scope, SourceVersion},
};

pub(super) const TABLE_NAME: &str = "chunk_terms_v1";
const INDEX_NAME: &str = "chunk_terms_whitespace_v1";

// The marker records the application creation contract. Public index_stats
// exposes type/columns, not a tokenizer attestation. It cannot prove that an
// external writer has not replaced an index under the same name and type.
pub(super) fn schema_for() -> Arc<Schema> {
    Arc::new(
        Schema::new(vec![
            Field::new("tenant", DataType::Utf8, false),
            Field::new("workspace", DataType::Utf8, false),
            Field::new("id", DataType::Utf8, false),
            Field::new("source_id", DataType::Utf8, false),
            Field::new("version", DataType::Int32, false),
            Field::new("generation", DataType::Int64, false),
            Field::new("search_terms", DataType::Utf8, false),
        ])
        .with_metadata(HashMap::from([(
            "origence.full_text".into(),
            "chunk-lexical-whitespace-and-v1;lower_case=false;stem=false;remove_stop_words=false;ascii_folding=false;max_token_length=none".into(),
        )])),
    )
}

pub(super) async fn check_schema(table: &Table) -> StorageResult<()> {
    if table.schema().await.map_err(backend)?.as_ref() != schema_for().as_ref() {
        return Err(StorageError::Conflict("incompatible full-text schema".into()));
    }
    Ok(())
}

async fn ensure_index(table: &Table) -> StorageResult<()> {
    filter_indexes::ensure(table, INDEX_NAME, IndexType::FTS, "search_terms").await?;
    let indices = table.list_indices().await.map_err(backend)?;
    if !indices.iter().any(|index| index.name == INDEX_NAME) {
        table
            .create_index(
                &["search_terms"],
                Index::FTS(
                    FtsIndexBuilder::default()
                        .base_tokenizer("whitespace".into())
                        .lower_case(false)
                        .stem(false)
                        .remove_stop_words(false)
                        .ascii_folding(false)
                        .max_token_length(None),
                ),
            )
            .name(INDEX_NAME.into())
            .replace(false)
            .execute()
            .await
            .map_err(backend)?;
    }
    check_index(table).await
}

async fn check_index(table: &Table) -> StorageResult<()> {
    if !filter_indexes::check(table, INDEX_NAME, IndexType::FTS, "search_terms").await? {
        return Err(StorageError::Unavailable("native full-text index is not ready".into()));
    }
    Ok(())
}

pub(super) async fn upsert(
    store: &LanceDbStore,
    scope: Scope,
    entries: Vec<FullTextEntry>,
) -> StorageResult<()> {
    if entries.is_empty() {
        return Ok(());
    }
    // merge_insert does not define duplicate input-key resolution. Normalize
    // equivalent retry entries and reject contradictory input before any write.
    let mut unique = HashMap::new();
    for entry in &entries {
        if entry.search_terms.contains('\0') {
            return Err(StorageError::InvalidArgument("full-text terms contain NUL"));
        }
        let key = (entry.id, entry.source, entry.generation);
        if let Some(previous) = unique.insert(key, entry)
            && previous.search_terms != entry.search_terms
        {
            return Err(StorageError::Conflict("conflicting full-text batch identity".into()));
        }
    }
    let mut entries: Vec<_> = unique.into_values().collect();
    entries.sort_by_key(|entry| {
        (entry.id, entry.source.source_id, entry.source.version, entry.generation)
    });
    let schema = schema_for();
    let table = store
        .db
        .create_empty_table(TABLE_NAME, schema.clone())
        .mode(CreateTableMode::exist_ok(|request| request))
        .execute()
        .await
        .map_err(|error| match error {
            lancedb::Error::Schema { message } => StorageError::Conflict(message),
            other => backend(other),
        })?;
    check_schema(&table).await?;
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(StringArray::from(vec![scope.tenant_id.to_string(); entries.len()])),
            Arc::new(StringArray::from(vec![scope.workspace_id.to_string(); entries.len()])),
            Arc::new(StringArray::from(entries.iter().map(|entry| entry.id.to_string()).collect::<Vec<_>>())),
            Arc::new(StringArray::from(entries.iter().map(|entry| entry.source.source_id.to_string()).collect::<Vec<_>>())),
            Arc::new(Int32Array::from(entries.iter().map(|entry| entry.source.version).collect::<Vec<_>>())),
            Arc::new(Int64Array::from(entries.iter().map(|entry| entry.generation).collect::<Vec<_>>())),
            Arc::new(StringArray::from(entries.iter().map(|entry| entry.search_terms.as_str()).collect::<Vec<_>>())),
        ],
    )
    .map_err(|error| StorageError::Backend(error.to_string()))?;
    let mut merge = table.merge_insert(&[
        "tenant", "workspace", "source_id", "version", "id", "generation",
    ]);
    merge.when_matched_update_all(None);
    merge.when_not_matched_insert_all();
    merge
        .execute(Box::new(RecordBatchIterator::new(vec![Ok(batch)], schema)))
        .await
        .map_err(backend)?;
    // If index creation fails after the data commit, publication is still
    // pending. Retrying merges the same identity and retries index readiness.
    ensure_index(&table).await
}

pub(super) async fn search(
    store: &LanceDbStore,
    mut query: FullTextQuery,
) -> StorageResult<Vec<FullTextHit>> {
    if query.limit == 0
        || query.search_terms.split_whitespace().next().is_none()
        || query.artifact_ids.as_ref().is_some_and(Vec::is_empty)
    {
        return Ok(Vec::new());
    }
    if query.search_terms.contains('\0') {
        return Err(StorageError::InvalidArgument("full-text terms contain NUL"));
    }
    let Some(table) = store.open_existing(TABLE_NAME).await? else {
        return Ok(Vec::new());
    };
    let version = table.version().await.map_err(backend)?;
    table.checkout(version).await.map_err(backend)?;
    check_schema(&table).await?;
    check_index(&table).await?;
    if let Some(ids) = query.artifact_ids.as_mut() {
        ids.sort_unstable();
        ids.dedup();
    }
    let filter = scoped_filter(query.scope, query.generation, query.artifact_ids.as_deref());
    // Structured MatchQuery treats AND/OR, quotes and punctuation as literal
    // token input. No raw query-string parser can change conjunction or scope.
    let fts = FullTextSearchQuery::new_query(
        MatchQuery::new(query.search_terms)
            .with_column(Some("search_terms".into()))
            .with_operator(Operator::And)
            .with_fuzziness(Some(0))
            .into(),
    )
    .wand_factor(Some(1.0));
    let mut native = table.query().full_text_search(fts).only_if(filter).limit(query.limit);
    native.mut_query().prefilter = true;
    native.mut_query().fast_search = false;
    let batches = native
        .execute()
        .await
        .map_err(backend)?
        .try_collect::<Vec<RecordBatch>>()
        .await
        .map_err(|error| StorageError::Backend(error.to_string()))?;
    let mut hits = Vec::new();
    for batch in batches {
        let strings = |name| -> StorageResult<&StringArray> {
            batch.column_by_name(name)
                .and_then(|column| column.as_any().downcast_ref::<StringArray>())
                .ok_or_else(|| StorageError::Backend("invalid full-text string column".into()))
        };
        let ids = strings("id")?;
        let tenants = strings("tenant")?;
        let workspaces = strings("workspace")?;
        let sources = strings("source_id")?;
        let versions = batch.column_by_name("version")
            .and_then(|column| column.as_any().downcast_ref::<Int32Array>())
            .ok_or_else(|| StorageError::Backend("invalid full-text version column".into()))?;
        let generations = batch.column_by_name("generation")
            .and_then(|column| column.as_any().downcast_ref::<Int64Array>())
            .ok_or_else(|| StorageError::Backend("invalid full-text generation column".into()))?;
        let scores = batch.column_by_name("_score")
            .and_then(|column| column.as_any().downcast_ref::<Float32Array>())
            .ok_or_else(|| StorageError::Backend("invalid full-text score column".into()))?;
        for index in 0..batch.num_rows() {
            if [ids, tenants, workspaces, sources].iter().any(|column| column.is_null(index))
                || versions.is_null(index) || generations.is_null(index) || scores.is_null(index)
                || !scores.value(index).is_finite() || scores.value(index) <= 0.0
                || tenants.value(index) != query.scope.tenant_id.to_string()
                || workspaces.value(index) != query.scope.workspace_id.to_string()
                || generations.value(index) != query.generation
            {
                return Err(StorageError::Backend("invalid scoped full-text candidate".into()));
            }
            let id = Uuid::parse_str(ids.value(index))
                .map_err(|_| StorageError::Backend("invalid full-text identity".into()))?;
            if query.artifact_ids.as_ref().is_some_and(|allowed| allowed.binary_search(&id).is_err()) {
                return Err(StorageError::Backend("ineligible full-text candidate".into()));
            }
            hits.push(FullTextHit {
                id,
                source: SourceVersion {
                    source_id: Uuid::parse_str(sources.value(index))
                        .map_err(|_| StorageError::Backend("invalid full-text source".into()))?,
                    version: versions.value(index),
                },
                score: scores.value(index),
            });
        }
    }
    // Order within the returned BM25-legal set. This does not promise which
    // equally-scored members native top-k selects, including after a rebuild.
    hits.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
    Ok(hits)
}

fn backend(error: lancedb::Error) -> StorageError {
    StorageError::Backend(error.to_string())
}
