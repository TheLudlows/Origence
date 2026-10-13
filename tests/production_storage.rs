//! Production Turso + LanceDB publication contracts, through non-AML callers.
//!
//! Existing message-batch tests cover vector failures and authority failures.
//! These tests cover the new full-text surface, its authority boundary, and the
//! retrieval behavior that must survive the backend replacement. All model IO
//! is a deterministic localhost fixture; no model quality is evaluated.
#![cfg(feature = "local-storage")]

use axum::{Json, Router, routing::post};
use origence::{
    error::AppError,
    messages::{AcceptedMessageSpec, IngestReceipt, MessageBatch, PublicationStatus, SourceMessage},
    models::Models,
    parsing,
    service::Service,
    storage::{
        AuthorizedScope, DomainTx, FullTextEntry, FullTextQuery, LedgerEntry, LedgerKey,
        Permission, RelationalStore, Scope, SourceVersion, Surface, VectorEntry, VectorStore,
        ledger_idempotency_key, native_turso,
    },
    types::{AuthContext, RetrievalComponents, SearchInput},
    worker,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::task::JoinHandle;
use uuid::Uuid;

struct FixtureModels {
    base: String,
    embeddings: Arc<AtomicUsize>,
    chats: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}

impl FixtureModels {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let embeddings = Arc::new(AtomicUsize::new(0));
        let chats = Arc::new(AtomicUsize::new(0));
        let embedding_calls = embeddings.clone();
        let chat_calls = chats.clone();
        let app = Router::new()
            .route(
                "/embeddings",
                post(move |Json(input): Json<Value>| {
                    let calls = embedding_calls.clone();
                    async move {
                        assert_eq!(input["dimensions"], 2);
                        calls.fetch_add(1, Ordering::SeqCst);
                        Json(json!({"data":[{"embedding":[1.0,0.5]}]}))
                    }
                }),
            )
            .route(
                "/chat/completions",
                post(move || {
                    let calls = chat_calls.clone();
                    async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        (
                            axum::http::StatusCode::SERVICE_UNAVAILABLE,
                            Json(json!({"error":"raw-only fixture must not extract"})),
                        )
                    }
                }),
            );
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            base,
            embeddings,
            chats,
            task,
        }
    }

    fn models(&self) -> Models {
        Models::configured(
            self.base.clone(),
            "local-fixture".into(),
            Some("storage-fixture-v1".into()),
            None,
            2,
        )
        .unwrap()
    }

    fn spec(&self) -> AcceptedMessageSpec {
        AcceptedMessageSpec::raw_only(self.models().profile.unwrap())
    }
}

impl Drop for FixtureModels {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn scope(auth: &AuthContext) -> Scope {
    Scope {
        tenant_id: auth.tenant_id,
        workspace_id: auth.workspace_id,
    }
}

fn authorized(auth: &AuthContext) -> AuthorizedScope {
    AuthorizedScope {
        scope: scope(auth),
        principal_id: auth.id,
        role: auth.role.clone(),
    }
}

async fn provision(service: &Service) -> (String, AuthContext) {
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    service
        .engine
        .relational()
        .create_workspace(scope.tenant_id, scope.workspace_id, "production storage regression")
        .await
        .unwrap();
    let key = service
        .engine
        .relational()
        .issue_key(scope, "admin")
        .await
        .unwrap();
    let auth = service.auth(&key.token).await.unwrap();
    (key.token, auth)
}

fn batch(id: &str, texts: &[&str]) -> MessageBatch {
    MessageBatch {
        batch_id: id.into(),
        session_id: "storage-contract".into(),
        messages: texts
            .iter()
            .map(|text| SourceMessage {
                role: "tool".into(),
                content: (*text).into(),
                timestamp: None,
            })
            .collect(),
    }
}

fn query(text: &str, mode: &str, summaries: bool) -> SearchInput {
    SearchInput {
        query: text.into(),
        mode: mode.into(),
        limit: 100,
        allow_partial: false,
        memory_identity: None,
        components: RetrievalComponents {
            summaries,
            graph: false,
        },
    }
}

async fn context(service: &Service, auth: &AuthContext, text: &str, mode: &str) -> Value {
    serde_json::to_value(
        service
            .evidence_bundle(
                auth,
                serde_json::from_value(json!({
                    "query":text,"mode":mode,"budget":32000,
                    "routes":[{"route":"knowledge","limit":100,"max_bytes":32000}]
                }))
                .unwrap(),
            )
            .await
            .unwrap(),
    )
    .unwrap()
}

async fn assert_invisible(service: &Service, auth: &AuthContext) {
    for mode in ["keyword", "vector", "hybrid"] {
        let result = service.search(auth, query("Atlas", mode, false)).await.unwrap();
        assert_eq!(result["hits"], json!([]), "{mode}: {result}");
        let result = context(service, auth, "Atlas", mode).await;
        assert_eq!(result["items"], json!([]), "{mode}: {result}");
    }
}

async fn payload(service: &Service, receipt: &IngestReceipt) -> Value {
    let text: String = native_turso::query_scalar("SELECT payload FROM oc_jobs WHERE id=?")
        .bind(receipt.job_id)
        .fetch_one(service.engine.relational().pool())
        .await
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

fn entries(receipt: &IngestReceipt, payload: &Value) -> Vec<FullTextEntry> {
    let generation = payload["publication"]["generation"].as_i64().unwrap();
    payload["publication"]["memories"][0]["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|chunk| FullTextEntry {
            id: serde_json::from_value(chunk["id"].clone()).unwrap(),
            source: SourceVersion {
                source_id: receipt.source_event_id,
                version: receipt.target_version,
            },
            generation,
            search_terms: chunk["search_terms"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| parsing::lexical(chunk["chunk"]["content"].as_str().unwrap())),
        })
        .collect()
}

async fn native_ids(service: &Service, auth: &AuthContext, generation: i64) -> BTreeSet<Uuid> {
    service
        .engine
        .vector()
        .search_full_text(FullTextQuery {
            scope: scope(auth),
            artifact_ids: None,
            generation,
            search_terms: parsing::lexical("Atlas"),
            limit: 100,
        })
        .await
        .unwrap()
        .into_iter()
        .map(|hit| hit.id)
        .collect()
}

async fn publish(
    service: &Service,
    auth: &AuthContext,
    models: &FixtureModels,
    id: &str,
    text: &str,
) -> IngestReceipt {
    let receipt = service
        .submit_message_batch(auth, id, batch(id, &[text]), models.spec())
        .await
        .unwrap();
    assert!(worker::process_next(service).await.unwrap());
    assert_eq!(
        service.message_batch_status(auth, &receipt).await.unwrap(),
        PublicationStatus::Retrievable
    );
    receipt
}

#[tokio::test]
async fn full_text_write_failure_restarts_the_original_plan_without_partial_publication() {
    let models = FixtureModels::start().await;
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(dir.path(), models.models()).await.unwrap();
    let (token, auth) = provision(&service).await;
    // A real native IO failure, after vector preparation, with no production
    // fault hook. This is the adapter's fixed full-text dataset name.
    let obstruction = dir.path().join("vectors/chunk_terms_v1.lance");
    assert!(!obstruction.exists());
    tokio::fs::write(&obstruction, b"test-owned full-text obstruction")
        .await
        .unwrap();
    let input = batch("full-text-restart", &["Atlas 中文🙂 first", "Atlas café second"]);
    let receipt = service
        .submit_message_batch(&auth, "full-text-restart", input.clone(), models.spec())
        .await
        .unwrap();
    // Cleanup may encounter the intentionally broken native table too; the
    // durable job state below must be settled in either case.
    assert!(matches!(
        worker::process_next(&service).await,
        Ok(true) | Err(AppError::Internal(_))
    ));
    let job = service.job(&auth, receipt.job_id).await.unwrap();
    assert_eq!(job["state"], "retry_wait");
    assert_eq!(job["generation"], 1);
    assert_eq!(job["error_code"], "INTERNAL_ERROR");
    assert_eq!(
        service.message_batch_status(&auth, &receipt).await.unwrap(),
        PublicationStatus::Pending
    );
    for table in ["oc_versions", "oc_chunks", "oc_index_entries"] {
        let count: i64 = native_turso::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(service.engine.relational().pool())
            .await
            .unwrap();
        assert_eq!(count, 0, "{table}");
    }
    let saved = payload(&service, &receipt).await;
    let planned = entries(&receipt, &saved);
    assert_eq!(planned.len(), 2);
    let committed: i64 = native_turso::query_scalar(
        "SELECT count(*) FROM oc_artifact_ledger WHERE surface='full_text' AND state='committed'",
    )
    .fetch_one(service.engine.relational().pool())
    .await
    .unwrap();
    assert_eq!(committed, 0);
    let registered: i64 = native_turso::query_scalar(
        "SELECT count(*) FROM oc_artifact_ledger WHERE surface='full_text'",
    )
    .fetch_one(service.engine.relational().pool())
    .await
    .unwrap();
    assert_eq!(registered, planned.len() as i64);
    assert_invisible(&service, &auth).await;
    assert_eq!(
        service
            .submit_message_batch(&auth, "full-text-restart", input.clone(), models.spec())
            .await
            .unwrap(),
        receipt
    );
    let calls = models.embeddings.load(Ordering::SeqCst);
    assert!(tokio::fs::metadata(&obstruction).await.unwrap().is_file());
    tokio::fs::remove_file(&obstruction).await.unwrap();
    // Model a due retry abandoned in processing. Production scheduling and
    // recovery policy stay unchanged; only this fixture's clock/state moves.
    assert_eq!(
        native_turso::query(
            "UPDATE oc_jobs SET state='processing',next_retry_at=0 WHERE id=? AND state='retry_wait'",
        )
        .bind(receipt.job_id)
        .execute(service.engine.relational().pool())
        .await
        .unwrap()
        .rows_affected(),
        1
    );
    service.engine.shutdown().await.unwrap();
    drop(service);

    let service = Service::open(dir.path(), models.models()).await.unwrap();
    let auth = service.auth(&token).await.unwrap();
    assert_eq!(payload(&service, &receipt).await, saved);
    assert!(worker::process_next(&service).await.unwrap());
    assert_eq!(models.embeddings.load(Ordering::SeqCst), calls);
    assert_eq!(models.chats.load(Ordering::SeqCst), 0);
    assert_eq!(
        service.message_batch_status(&auth, &receipt).await.unwrap(),
        PublicationStatus::Retrievable
    );
    let expected: BTreeSet<_> = planned.iter().map(|entry| entry.id).collect();
    assert_eq!(native_ids(&service, &auth, 1).await, expected);
    let committed: i64 = native_turso::query_scalar(
        "SELECT count(*) FROM oc_artifact_ledger WHERE surface='full_text' AND state='committed'",
    )
    .fetch_one(service.engine.relational().pool())
    .await
    .unwrap();
    assert_eq!(committed, planned.len() as i64);
    for mode in ["keyword", "vector", "hybrid"] {
        let result = service.search(&auth, query("Atlas", mode, false)).await.unwrap();
        let hits = result["hits"].as_array().unwrap();
        let actual: BTreeSet<Uuid> = hits
            .iter()
            .map(|hit| serde_json::from_value(hit["chunk_id"].clone()).unwrap())
            .collect();
        assert_eq!(actual, expected, "{mode}");
        assert_eq!(hits.len(), expected.len());
        let result = context(&service, &auth, "Atlas", mode).await;
        assert_eq!(result["items"].as_array().unwrap().len(), expected.len());
        assert!(result["items"].as_array().unwrap().iter().all(|item| {
            item["provenance"]["asset_id"] == json!(receipt.asset_id)
                && item["content"].as_str().unwrap().contains("Atlas")
        }));
    }
    assert_eq!(
        service
            .submit_message_batch(&auth, "full-text-restart", input.clone(), models.spec())
            .await
            .unwrap(),
        receipt
    );
    let mut changed = input;
    changed.messages[0].content.push_str(" changed");
    assert!(matches!(
        service
            .submit_message_batch(&auth, "full-text-restart", changed, models.spec())
            .await,
        Err(AppError::Conflict(_))
    ));
    assert!(matches!(
        service.job_action(&auth, "no-republish", receipt.job_id, "retry").await,
        Err(AppError::Conflict(_))
    ));
    assert_eq!(payload(&service, &receipt).await, saved);
    for (table, expected) in [
        ("oc_jobs", 1),
        ("oc_events", 1),
        ("oc_assets", 1),
        ("oc_versions", 1),
        ("oc_chunks", 2),
    ] {
        let count: i64 = native_turso::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(service.engine.relational().pool())
            .await
            .unwrap();
        assert_eq!(count, expected, "{table}");
    }
    assert!(!worker::process_next(&service).await.unwrap());
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn withdrawn_source_is_hidden_before_cleanup_and_cannot_be_resurrected_by_native_rows() {
    let models = FixtureModels::start().await;
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(dir.path(), models.models()).await.unwrap();
    let (token, auth) = provision(&service).await;
    let (_, other) = provision(&service).await;
    let receipt = publish(&service, &auth, &models, "withdraw", "Atlas retained original").await;
    let saved = payload(&service, &receipt).await;
    let indexed = entries(&receipt, &saved);
    let expected: BTreeSet<_> = indexed.iter().map(|entry| entry.id).collect();
    assert_eq!(native_ids(&service, &auth, 1).await, expected);
    service
        .delete(&auth, "withdraw-source", receipt.source_event_id, "event")
        .await
        .unwrap();
    // Native data is still present, but withdrawal is immediately authoritative.
    assert_eq!(native_ids(&service, &auth, 1).await, expected);
    assert_invisible(&service, &auth).await;
    assert_eq!(
        service.message_batch_status(&auth, &receipt).await.unwrap(),
        PublicationStatus::Invisible
    );
    assert!(worker::process_next(&service).await.unwrap());
    assert!(native_ids(&service, &auth, 1).await.is_empty());
    service
        .engine
        .vector()
        .upsert_full_text(scope(&auth), indexed.clone())
        .await
        .unwrap();
    // A residual/replayed index record is never permission to publish. Reusing
    // every native ID in another scope also cannot lend it relational authority.
    service
        .engine
        .vector()
        .upsert_full_text(scope(&other), indexed)
        .await
        .unwrap();
    assert_eq!(native_ids(&service, &auth, 1).await, expected);
    assert_eq!(native_ids(&service, &other, 1).await, expected);
    assert_invisible(&service, &auth).await;
    assert_invisible(&service, &other).await;
    assert_eq!(
        service
            .submit_message_batch(
                &auth,
                "withdraw",
                batch("withdraw", &["Atlas retained original"]),
                models.spec(),
            )
            .await
            .unwrap(),
        receipt
    );
    assert_eq!(
        service.message_batch_status(&auth, &receipt).await.unwrap(),
        PublicationStatus::Invisible
    );
    service.engine.shutdown().await.unwrap();
    drop(service);
    let service = Service::open(dir.path(), models.models()).await.unwrap();
    let auth = service.auth(&token).await.unwrap();
    assert_invisible(&service, &auth).await;
    assert_eq!(
        service.message_batch_status(&auth, &receipt).await.unwrap(),
        PublicationStatus::Invisible
    );
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_replays_vote_once_and_conflicting_committed_generations_fail_closed() {
    let models = FixtureModels::start().await;
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(dir.path(), models.models()).await.unwrap();
    let (_, auth) = provision(&service).await;
    let receipt = publish(&service, &auth, &models, "rrf", "Atlas single evidence").await;
    let entry = entries(&receipt, &payload(&service, &receipt).await).remove(0);
    let before = service.search(&auth, query("Atlas", "hybrid", false)).await.unwrap();
    let context_before = context(&service, &auth, "Atlas", "hybrid").await;
    assert_eq!(before["hits"].as_array().unwrap().len(), 1);
    assert!((before["hits"][0]["score"].as_f64().unwrap() - 2.0 / 61.0).abs() < 1e-12);
    assert_eq!(context_before["items"][0]["retrieval_routes"], json!(["keyword", "vector"]));
    for _ in 0..2 {
        service.engine.vector().upsert_full_text(scope(&auth), vec![entry.clone()]).await.unwrap();
        service.engine.vector().upsert(scope(&auth), vec![VectorEntry {
            id: entry.id,
            source: entry.source,
            generation: entry.generation,
            profile: models.spec().embedding_profile,
            dimension: 2,
            embedding: vec![1.0, 0.5],
        }]).await.unwrap();
    }
    let native = service.engine.vector().search_full_text(FullTextQuery {
        scope: scope(&auth),
        artifact_ids: Some(vec![entry.id]),
        generation: entry.generation,
        search_terms: parsing::lexical("Atlas"),
        limit: 100,
    }).await.unwrap();
    assert_eq!(native.len(), 1);
    assert_eq!(native[0].id, entry.id);
    assert_eq!(native[0].source, entry.source);
    assert_eq!(service.search(&auth, query("Atlas", "hybrid", false)).await.unwrap()["hits"], before["hits"]);
    assert_eq!(context(&service, &auth, "Atlas", "hybrid").await["items"], context_before["items"]);

    // Better-looking native rows under the same chunk UUID but a wrong source,
    // wrong version or uncommitted generation never lend a score or visibility.
    let mut wrong_version = entry.clone();
    wrong_version.source.version += 1;
    let mut wrong_source = entry.clone();
    wrong_source.source.source_id = Uuid::new_v4();
    let mut uncommitted = entry.clone();
    uncommitted.generation = 2;
    service.engine.vector().upsert_full_text(
        scope(&auth),
        vec![wrong_version.clone(), wrong_source.clone(), uncommitted.clone()],
    ).await.unwrap();
    let mut vectors = Vec::new();
    for native in [wrong_version, wrong_source, uncommitted.clone()] {
        vectors.push(VectorEntry {
            id: native.id,
            source: native.source,
            generation: native.generation,
            profile: models.spec().embedding_profile,
            dimension: 2,
            embedding: vec![1.0, 0.5],
        });
    }
    // If a forged candidate passes validation, its 1.0 would replace the only
    // legitimate score of 0.5 and make this boundary failure observable.
    vectors.push(VectorEntry {
        id: entry.id,
        source: entry.source,
        generation: entry.generation,
        profile: models.spec().embedding_profile,
        dimension: 2,
        embedding: vec![2.0, 0.5],
    });
    service.engine.vector().upsert(scope(&auth), vectors).await.unwrap();
    let vectors = service.search(&auth, query("Atlas", "vector", false)).await.unwrap();
    assert_eq!(vectors["hits"].as_array().unwrap().len(), 1);
    assert!((vectors["hits"][0]["score"].as_f64().unwrap() - 0.5).abs() < 1e-6);
    let vectors = context(&service, &auth, "Atlas", "vector").await;
    assert_eq!(vectors["items"].as_array().unwrap().len(), 1);
    assert!((vectors["items"][0]["score"].as_f64().unwrap() - 0.5).abs() < 1e-6);
    assert_eq!(service.search(&auth, query("Atlas", "hybrid", false)).await.unwrap()["hits"], before["hits"]);
    assert_eq!(context(&service, &auth, "Atlas", "hybrid").await["items"], context_before["items"]);

    // Normal plans never reuse one chunk UUID for two committed generations.
    // There is no active-generation pointer to choose between these records;
    // injecting conflicting authority must fail closed, never take max/first.
    let mut tx = service.engine.relational().begin(authorized(&auth)).await.unwrap();
    tx.check_permission(Permission::Write).await.unwrap();
    tx.index_ready(entry.id, &models.spec().embedding_profile, 2, uncommitted.generation)
        .await
        .unwrap();
    for surface in [Surface::FullText, Surface::Vector] {
        let key = LedgerKey {
            scope: scope(&auth),
            source: entry.source,
            artifact_id: entry.id,
            generation: uncommitted.generation,
            surface,
        };
        tx.register_pending(LedgerEntry {
            key,
            artifact_type: "chunk".into(),
            idempotency_key: ledger_idempotency_key(&key),
        }).await.unwrap();
        tx.confirm_committed(key).await.unwrap();
    }
    tx.commit().await.unwrap();
    for mode in ["keyword", "vector", "hybrid"] {
        assert!(matches!(service.search(&auth, query("Atlas", mode, false)).await, Err(AppError::Conflict(_))), "{mode}");
        let input = serde_json::from_value(json!({
            "query":"Atlas","mode":mode,"budget":32000,
            "routes":[{"route":"knowledge","limit":100,"max_bytes":32000}]
        })).unwrap();
        assert!(matches!(service.evidence_bundle(&auth, input).await, Err(AppError::Conflict(_))), "{mode}");
    }
    // This fixture never asserts membership at a tied top-k cutoff.
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn completed_receipt_requires_every_exact_full_text_ledger_member() {
    let models = FixtureModels::start().await;
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(dir.path(), models.models()).await.unwrap();
    let (_, auth) = provision(&service).await;
    let receipt = service
        .submit_message_batch(
            &auth,
            "full-text-readiness",
            batch("full-text-readiness", &["Atlas first", "Atlas second"]),
            models.spec(),
        )
        .await
        .unwrap();
    assert!(worker::process_next(&service).await.unwrap());
    let planned = entries(&receipt, &payload(&service, &receipt).await);
    assert_eq!(planned.len(), 2);
    assert_eq!(
        service.message_batch_status(&auth, &receipt).await.unwrap(),
        PublicationStatus::Retrievable
    );
    // The native index still contains both chunks throughout. A completed job
    // or another chunk's committed ledger cannot substitute for this member.
    for entry in planned {
        for state in ["pending", "retry_wait", "orphan"] {
            assert_eq!(
                native_turso::query(
                    "UPDATE oc_artifact_ledger SET state=? WHERE tenant_id=? AND workspace_id=? AND artifact_id=? AND surface='full_text'",
                )
                .bind(state)
                .bind(auth.tenant_id)
                .bind(auth.workspace_id)
                .bind(entry.id)
                .execute(service.engine.relational().pool())
                .await
                .unwrap()
                .rows_affected(),
                1
            );
            assert_eq!(
                service.message_batch_status(&auth, &receipt).await.unwrap(),
                PublicationStatus::Invisible,
                "{state}"
            );
        }
        native_turso::query(
            "UPDATE oc_artifact_ledger SET state='committed' WHERE tenant_id=? AND workspace_id=? AND artifact_id=? AND surface='full_text'",
        )
        .bind(auth.tenant_id)
        .bind(auth.workspace_id)
        .bind(entry.id)
        .execute(service.engine.relational().pool())
        .await
        .unwrap();
        assert_eq!(
            service.message_batch_status(&auth, &receipt).await.unwrap(),
            PublicationStatus::Retrievable
        );
    }
    service.engine.shutdown().await.unwrap();
}

fn scores(result: &Value) -> BTreeMap<Uuid, f64> {
    result["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hit| {
            (
                serde_json::from_value(hit["chunk_id"].clone()).unwrap(),
                hit["score"].as_f64().unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn summary_and_matches_across_rows_of_one_chunk_without_cross_chunk_leakage() {
    let models = FixtureModels::start().await;
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(dir.path(), models.models()).await.unwrap();
    let (_, auth) = provision(&service).await;
    let mut chunks = Vec::new();
    for (id, text) in [("both", "original first"), ("alpha", "original second"), ("beta", "original third")] {
        let receipt = publish(&service, &auth, &models, id, text).await;
        chunks.push(entries(&receipt, &payload(&service, &receipt).await)[0].id);
    }
    let mut tx = service.engine.relational().begin(authorized(&auth)).await.unwrap();
    tx.check_permission(Permission::Write).await.unwrap();
    for (chunk, text) in [
        (chunks[0], "alpha"),
        (chunks[0], "beta"),
        (chunks[1], "alpha"),
        (chunks[2], "beta"),
    ] {
        tx.insert_summary(Uuid::new_v4(), chunk, text, "fixture-summary", &parsing::lexical(text))
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    let mut tx = service.engine.relational().begin_read(authorized(&auth)).await.unwrap();
    tx.check_permission(Permission::Read).await.unwrap();
    let direct = tx.keyword_hits("alpha beta", true, None).await.unwrap();
    assert_eq!(direct.len(), 1);
    assert_eq!(direct[0].chunk_id, chunks[0]);
    assert!(tx.keyword_hits("alpha beta absent", true, None).await.unwrap().is_empty());
    tx.commit().await.unwrap();
    assert_eq!(
        service.search(&auth, query("alpha beta", "keyword", false)).await.unwrap()["hits"],
        json!([])
    );
    assert_eq!(context(&service, &auth, "alpha beta", "keyword").await["items"], json!([]));
    let without = service.search(&auth, query("alpha beta", "hybrid", false)).await.unwrap();
    let with = service.search(&auth, query("alpha beta", "hybrid", true)).await.unwrap();
    let (without, with) = (scores(&without), scores(&with));
    assert_eq!(without.len(), 3);
    assert_eq!(with.len(), 3);
    assert!((with[&chunks[0]] - without[&chunks[0]] - 1.0 / 61.0).abs() < 1e-12);
    for chunk in &chunks[1..] {
        assert_eq!(with[chunk], without[chunk]);
    }
    assert_eq!(models.chats.load(Ordering::SeqCst), 0);
    service.engine.shutdown().await.unwrap();
}
