//! Non-AML public-library contracts for native original-chunk full-text search.
#![cfg(feature = "local-vector")]

use origence::{
    parsing,
    storage::{
        FullTextEntry, FullTextQuery, LanceDbStore, Lifecycle, Scope, SourceVersion,
        StorageError, VectorEntry, VectorQuery, VectorStore,
    },
};
use uuid::Uuid;

fn scope() -> Scope {
    Scope { tenant_id: Uuid::new_v4(), workspace_id: Uuid::new_v4() }
}

fn entry(terms: &str) -> FullTextEntry {
    FullTextEntry {
        id: Uuid::new_v4(),
        source: SourceVersion { source_id: Uuid::new_v4(), version: 1 },
        generation: 1,
        search_terms: terms.into(),
    }
}

fn query(scope: Scope, terms: &str) -> FullTextQuery {
    FullTextQuery {
        scope,
        artifact_ids: None,
        generation: 1,
        search_terms: terms.into(),
        limit: 100,
    }
}

#[tokio::test]
async fn full_text_uses_literal_and_and_application_chinese_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let store = LanceDbStore::open(dir.path()).await.unwrap();
    let scope = scope();
    let both = entry("alpha beta");
    let chinese = entry(&parsing::lexical("江南城市"));
    let literal = entry("the Running café alpha-beta AND OR");
    let decoy = entry("run running cafe alpha beta");
    store.upsert_full_text(scope, vec![
        both.clone(), entry("alpha"), entry("beta"), chinese.clone(),
        literal.clone(), decoy.clone(),
    ]).await.unwrap();
    let db = lancedb::connect(dir.path().to_str().unwrap()).execute().await.unwrap();
    let table = db.open_table("chunk_terms_v1").execute().await.unwrap();
    let indices = table.list_indices().await.unwrap();
    assert_eq!(indices.len(), 5);
    for column in ["tenant", "workspace", "generation", "id"] {
        assert!(indices.iter().any(|index| index.columns == [column]
            && index.index_type == lancedb::index::IndexType::BTree));
    }
    let hits = store.search_full_text(query(scope, "alpha beta")).await.unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().any(|hit| hit.id == both.id));
    assert!(hits.iter().any(|hit| hit.id == decoy.id));
    for terms in ["the", "Running", "café", "AND OR alpha-beta"] {
        let hits = store.search_full_text(query(scope, terms)).await.unwrap();
        assert_eq!(hits.len(), 1, "{terms}");
        assert_eq!(hits[0].id, literal.id, "{terms}");
    }
    let hits = store.search_full_text(query(scope, "run")).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, decoy.id);
    let hits = store.search_full_text(query(scope, &parsing::lexical("江南城市"))).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, chinese.id);
    assert!(store.search_full_text(query(scope, "missingterm")).await.unwrap().is_empty());
    assert!(store.search_full_text(query(scope, " \t\n")).await.unwrap().is_empty());
}

#[tokio::test]
async fn full_text_prefilters_scope_generation_and_authorized_ids_before_topk() {
    let dir = tempfile::tempdir().unwrap();
    let store = LanceDbStore::open(dir.path()).await.unwrap();
    let scope = scope();
    let allowed = entry(&format!("quartz {}", "ballast ".repeat(40)));
    let forbidden = entry("quartz quartz quartz");
    let next = FullTextEntry { generation: 2, ..forbidden.clone() };
    store.upsert_full_text(scope, vec![allowed.clone(), forbidden.clone(), next]).await.unwrap();
    let foreign_tenant = Scope { tenant_id: Uuid::new_v4(), ..scope };
    let foreign_workspace = Scope { workspace_id: Uuid::new_v4(), ..scope };
    store.upsert_full_text(foreign_tenant, vec![forbidden.clone()]).await.unwrap();
    store.upsert_full_text(foreign_workspace, vec![forbidden.clone()]).await.unwrap();
    let mut request = query(scope, "quartz");
    request.artifact_ids = Some(vec![allowed.id, allowed.id]);
    request.limit = 1;
    let hits = store.search_full_text(request.clone()).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, allowed.id);
    request.artifact_ids = Some(vec![]);
    assert!(store.search_full_text(request.clone()).await.unwrap().is_empty());
    request.artifact_ids = None;
    request.generation = 2;
    let hits = store.search_full_text(request).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, forbidden.id);
    assert_eq!(store.search_full_text(query(foreign_tenant, "quartz")).await.unwrap().len(), 1);
}

#[tokio::test]
async fn full_text_replay_update_delete_and_reopen_preserve_full_identity() {
    let dir = tempfile::tempdir().unwrap();
    let store = LanceDbStore::open(dir.path()).await.unwrap();
    let scope = scope();
    let original = entry("before lifecyclestay");
    let updated = FullTextEntry { search_terms: "after lifecyclestay".into(), ..original.clone() };
    let version = FullTextEntry {
        source: SourceVersion { version: 2, ..original.source },
        search_terms: "versiontoken".into(),
        ..original.clone()
    };
    let generation = FullTextEntry {
        generation: 2,
        search_terms: "generationtoken".into(),
        ..original.clone()
    };
    store.upsert_full_text(scope, vec![original.clone(), original.clone()]).await.unwrap();
    store.upsert_full_text(scope, vec![original.clone()]).await.unwrap();
    assert_eq!(store.search_full_text(query(scope, "before")).await.unwrap().len(), 1);
    store.upsert_full_text(scope, vec![updated.clone(), version.clone(), generation.clone()]).await.unwrap();
    assert!(store.search_full_text(query(scope, "before")).await.unwrap().is_empty());
    assert_eq!(store.search_full_text(query(scope, "after")).await.unwrap().len(), 1);
    let fresh = entry("freshlexeme");
    store.upsert_full_text(scope, vec![fresh.clone()]).await.unwrap();
    assert_eq!(store.search_full_text(query(scope, "freshlexeme")).await.unwrap().len(), 1);
    let other_scope = Scope { workspace_id: Uuid::new_v4(), ..scope };
    store.upsert_full_text(other_scope, vec![updated.clone()]).await.unwrap();
    // Source removal also removes all retry generations, retaining other versions.
    store.delete_source(scope, original.source).await.unwrap();
    store.delete_source(scope, original.source).await.unwrap();
    assert!(store.search_full_text(query(scope, "after")).await.unwrap().is_empty());
    let mut newer = query(scope, "generationtoken");
    newer.generation = 2;
    assert!(store.search_full_text(newer).await.unwrap().is_empty());
    assert_eq!(store.search_full_text(query(scope, "versiontoken")).await.unwrap().len(), 1);
    assert_eq!(store.search_full_text(query(other_scope, "after")).await.unwrap().len(), 1);
    store.delete_objects(scope, vec![fresh.id, fresh.id]).await.unwrap();
    store.delete_objects(scope, vec![fresh.id]).await.unwrap();
    store.check().await.unwrap();
    drop(store);
    let reopened = LanceDbStore::open(dir.path()).await.unwrap();
    reopened.check().await.unwrap();
    assert!(reopened.search_full_text(query(scope, "freshlexeme")).await.unwrap().is_empty());
    assert_eq!(reopened.search_full_text(query(scope, "versiontoken")).await.unwrap().len(), 1);
}

#[tokio::test]
async fn full_text_retry_after_real_first_write_failure() {
    let dir = tempfile::tempdir().unwrap();
    let store = LanceDbStore::open(dir.path()).await.unwrap();
    let scope = scope();
    let record = entry("recoverytoken");
    let blocker = dir.path().join("chunk_terms_v1.lance");
    tokio::fs::write(&blocker, b"block dataset creation").await.unwrap();
    assert!(store.upsert_full_text(scope, vec![record.clone()]).await.is_err());
    tokio::fs::remove_file(blocker).await.unwrap();
    store.upsert_full_text(scope, vec![record.clone()]).await.unwrap();
    store.upsert_full_text(scope, vec![record]).await.unwrap();
    assert_eq!(store.search_full_text(query(scope, "recoverytoken")).await.unwrap().len(), 1);
}

#[tokio::test]
async fn full_text_tied_topk_is_a_sorted_legal_set_without_global_tie_promise() {
    let dir = tempfile::tempdir().unwrap();
    let store = LanceDbStore::open(dir.path()).await.unwrap();
    let scope = scope();
    let entries: Vec<_> = (0..96).rev().map(|i| FullTextEntry {
        id: Uuid::from_u128(i + 1),
        ..entry("tieanchor parity")
    }).collect();
    store.upsert_full_text(scope, entries.clone()).await.unwrap();
    let mut request = query(scope, "tieanchor");
    request.limit = 10;
    let hits = store.search_full_text(request).await.unwrap();
    assert_eq!(hits.len(), 10);
    assert!(hits.windows(2).all(|pair| pair[0].score >= pair[1].score));
    assert!(hits.windows(2).all(|pair| pair[0].score != pair[1].score || pair[0].id < pair[1].id));
    assert!(hits.iter().all(|hit| entries.iter().any(|entry| entry.id == hit.id)));
}

#[tokio::test]
async fn duplicate_input_conflict_is_rejected_and_delete_covers_both_indexes() {
    let dir = tempfile::tempdir().unwrap();
    let store = LanceDbStore::open(dir.path()).await.unwrap();
    let scope = scope();
    let text = entry("jointtoken");
    let conflict = FullTextEntry { search_terms: "conflict".into(), ..text.clone() };
    assert!(matches!(store.upsert_full_text(scope, vec![text.clone(), conflict]).await,
        Err(StorageError::Conflict(_))));
    assert!(store.search_full_text(query(scope, "jointtoken")).await.unwrap().is_empty());
    let vector = VectorEntry {
        id: text.id, source: text.source, generation: text.generation,
        dimension: 2, profile: "paired:v1".into(), embedding: vec![1.0, 0.0],
    };
    store.upsert_full_text(scope, vec![text.clone()]).await.unwrap();
    store.upsert(scope, vec![vector.clone(), vector.clone()]).await.unwrap();
    store.delete_objects(scope, vec![text.id]).await.unwrap();
    assert!(store.search_full_text(query(scope, "jointtoken")).await.unwrap().is_empty());
    assert!(store.search(VectorQuery {
        scope, artifact_ids: None, profile: vector.profile,
        dimension: vector.dimension, generation: vector.generation,
        embedding: vector.embedding, limit: 100,
    }).await.unwrap().is_empty());
}

#[tokio::test]
async fn full_text_empty_lexical_rows_do_not_block_later_terms() {
    let dir = tempfile::tempdir().unwrap();
    let store = LanceDbStore::open(dir.path()).await.unwrap();
    let scope = scope();
    let empty = entry("");
    store.upsert_full_text(scope, vec![empty]).await.unwrap();
    assert!(store.search_full_text(query(scope, "anything")).await.unwrap().is_empty());
    store.upsert_full_text(scope, vec![entry("laterlexeme")]).await.unwrap();
    assert_eq!(store.search_full_text(query(scope, "laterlexeme")).await.unwrap().len(), 1);
}

#[tokio::test]
async fn authoritative_candidates_require_native_table_and_every_physical_id() {
    let dir = tempfile::tempdir().unwrap();
    let store = LanceDbStore::open(dir.path()).await.unwrap();
    let scope = scope();
    let text = entry("missingnative");
    let mut full_text = query(scope, "missingnative");
    full_text.artifact_ids = Some(vec![text.id]);
    assert!(matches!(store.search_full_text(full_text.clone()).await,
        Err(StorageError::Unavailable(_))));
    assert!(matches!(store.check_full_text_available().await,
        Err(StorageError::Unavailable(_))));
    let vector = VectorEntry {
        id: text.id, source: text.source, generation: 1, dimension: 2,
        profile: "required-native:v1".into(), embedding: vec![1.0, 0.0],
    };
    let vector_query = VectorQuery {
        scope, artifact_ids: Some(vec![text.id]), generation: 1,
        profile: vector.profile.clone(), dimension: 2,
        embedding: vector.embedding.clone(), limit: 100,
    };
    assert!(matches!(store.search(vector_query.clone()).await,
        Err(StorageError::Unavailable(_))));
    store.upsert_full_text(scope, vec![text.clone()]).await.unwrap();
    store.upsert(scope, vec![vector]).await.unwrap();
    store.check_full_text_available().await.unwrap();
    full_text.artifact_ids.as_mut().unwrap().push(Uuid::new_v4());
    assert!(matches!(store.search_full_text(full_text).await,
        Err(StorageError::Unavailable(_))));
    store.delete_objects(scope, vec![text.id]).await.unwrap();
    assert!(matches!(store.search(vector_query).await,
        Err(StorageError::Unavailable(_))));
}

#[tokio::test]
async fn full_text_rejects_replaced_tokenizer_under_the_same_index_name() {
    use lancedb::index::{Index, scalar::FtsIndexBuilder};
    let dir = tempfile::tempdir().unwrap();
    let store = LanceDbStore::open(dir.path()).await.unwrap();
    let scope = scope();
    let text = entry("running contracttoken");
    store.upsert_full_text(scope, vec![text.clone()]).await.unwrap();
    let db = lancedb::connect(dir.path().to_str().unwrap()).execute().await.unwrap();
    let table = db.open_table("chunk_terms_v1").execute().await.unwrap();
    table.create_index(&["search_terms"], Index::FTS(FtsIndexBuilder::default()
        .lance_tokenizer("text".into())
        .base_tokenizer("whitespace".into()).lower_case(false)
        .stem(true).remove_stop_words(false).ascii_folding(false).max_token_length(None)))
        .name("chunk_terms_whitespace_v1".into()).replace(true).execute().await.unwrap();
    assert!(matches!(store.check_full_text_available().await, Err(StorageError::Conflict(_))));
    let mut request = query(scope, "contracttoken");
    request.artifact_ids = Some(vec![text.id]);
    assert!(matches!(store.search_full_text(request).await, Err(StorageError::Conflict(_))));
}

#[tokio::test]
async fn native_topk_rejects_identical_or_conflicting_physical_duplicate_ids() {
    use std::sync::Arc;
    use arrow_array::{RecordBatch, StringArray};
    use futures::TryStreamExt;
    use lancedb::query::{ExecutableQuery, QueryBase};
    for conflicting_source in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = LanceDbStore::open(dir.path()).await.unwrap();
        let scope = scope();
        let text = entry("duplicatetoken");
        let other = entry("duplicatetoken ballast");
        store.upsert_full_text(scope, vec![text.clone(), other.clone()]).await.unwrap();
        let vector = VectorEntry {
            id: text.id, source: text.source, generation: 1, dimension: 2,
            profile: "duplicate-native:v1".into(), embedding: vec![1.0, 0.0],
        };
        store.upsert(scope, vec![vector.clone(), VectorEntry {
            id: other.id, source: other.source, embedding: vec![0.0, 1.0], ..vector.clone()
        }]).await.unwrap();
        let db = lancedb::connect(dir.path().to_str().unwrap()).execute().await.unwrap();
        for name in db.table_names().execute().await.unwrap() {
            let table = db.open_table(&name).execute().await.unwrap();
            let mut batches: Vec<RecordBatch> = table.query()
                .only_if(format!("id='{}'", text.id)).limit(1)
                .execute().await.unwrap().try_collect().await.unwrap();
            let batch = batches.remove(0);
            let mut columns = batch.columns().to_vec();
            if conflicting_source {
                let column = batch.schema().index_of("source_id").unwrap();
                columns[column] = Arc::new(StringArray::from(vec![Uuid::new_v4().to_string()]));
            }
            let duplicate = RecordBatch::try_new(batch.schema(), columns).unwrap();
            table.add(duplicate).execute().await.unwrap();
        }
        let mut request = query(scope, "duplicatetoken");
        request.artifact_ids = Some(vec![text.id, other.id]);
        request.limit = 1;
        assert!(matches!(store.search_full_text(request).await, Err(StorageError::Conflict(_))));
        assert!(matches!(store.search(VectorQuery {
            scope, artifact_ids: Some(vec![text.id, other.id]), generation: 1,
            dimension: 2, profile: vector.profile, embedding: vector.embedding, limit: 1,
        }).await, Err(StorageError::Conflict(_))));
    }
}
