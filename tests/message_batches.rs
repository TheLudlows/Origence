//! Public, non-protocol message ingestion through the real Worker/search pipeline.
//! Deterministic localhost embeddings exercise engineering, not model quality.
#![cfg(feature = "local-storage")]
use axum::{Json, Router, routing::post};
use origence::{
    error::AppError,
    messages::{
        AcceptedMessageSpec, IngestReceipt, MessageBatch, PublicationStatus, SourceMessage,
    },
    models::Models,
    service::Service,
    storage::{RelationalStore, Scope},
    types::{AuthContext, RetrievalComponents, SearchInput},
    worker,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{sync::Notify, task::JoinHandle};
use uuid::Uuid;

struct ModelServer {
    base: String,
    calls: Arc<AtomicUsize>,
    chats: Arc<AtomicUsize>,
    fail_from: Arc<AtomicUsize>,
    hold: Arc<AtomicBool>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
    task: JoinHandle<()>,
}
impl ModelServer {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let chats = Arc::new(AtomicUsize::new(0));
        let fail_from = Arc::new(AtomicUsize::new(usize::MAX));
        let hold = Arc::new(AtomicBool::new(false));
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let (c, f, h, e, r) = (
            calls.clone(),
            fail_from.clone(),
            hold.clone(),
            entered.clone(),
            release.clone(),
        );
        let chat_calls = chats.clone();
        let app = Router::new()
            .route(
                "/embeddings",
                post(move |Json(input): Json<Value>| {
                    let (c, f, h, e, r) = (c.clone(), f.clone(), h.clone(), e.clone(), r.clone());
                    async move {
                        assert_eq!(input["dimensions"], 2);
                        let call = c.fetch_add(1, Ordering::SeqCst) + 1;
                        if h.swap(false, Ordering::SeqCst) {
                            e.notify_one();
                            r.notified().await;
                        }
                        if call >= f.load(Ordering::SeqCst) {
                            return (
                                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                                Json(json!({"error":"private provider diagnostic"})),
                            );
                        }
                        (
                            axum::http::StatusCode::OK,
                            Json(json!({"data":[{"embedding":[1.0,0.5]}]})),
                        )
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
                            Json(json!({"error":"raw-only must not call extraction"})),
                        )
                    }
                }),
            );
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            base,
            calls,
            chats,
            fail_from,
            hold,
            entered,
            release,
            task,
        }
    }
    fn models(&self) -> Models {
        Models::configured(
            self.base.clone(),
            "fixture".into(),
            Some("fixture-v1".into()),
            Some("must-not-run".into()),
            2,
        )
        .unwrap()
    }
    fn spec(&self) -> AcceptedMessageSpec {
        AcceptedMessageSpec::raw_only(self.models().profile.unwrap())
    }
}
impl Drop for ModelServer {
    fn drop(&mut self) {
        self.task.abort();
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
        .create_workspace(scope.tenant_id, scope.workspace_id, "Generic messages")
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
fn batch(id: &str) -> MessageBatch {
    MessageBatch {
        batch_id: id.into(),
        session_id: "shared-session".into(),
        messages: vec![
            SourceMessage {
                role: "system".into(),
                content: "Atlas 原文 中文🙂 café é\n".repeat(220),
                timestamp: Some(20),
            },
            SourceMessage {
                role: "tool".into(),
                content: "Atlas tool response".into(),
                timestamp: Some(-10),
            },
            SourceMessage {
                role: "agent:Planner".into(),
                content: "Atlas unspecified time".into(),
                timestamp: None,
            },
        ],
    }
}
fn query(mode: &str) -> SearchInput {
    SearchInput {
        query: "Atlas".into(),
        limit: 100,
        mode: mode.into(),
        allow_partial: false,
        memory_identity: None,
        components: RetrievalComponents {
            summaries: false,
            graph: false,
        },
    }
}
async fn count(service: &Service, table: &str) -> i64 {
    // Only fixed test-owned table literals are passed to this helper.
    sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
        .fetch_one(service.engine.relational().pool())
        .await
        .unwrap()
}
async fn status(
    service: &Service,
    auth: &AuthContext,
    receipt: &IngestReceipt,
) -> PublicationStatus {
    service.message_batch_status(auth, receipt).await.unwrap()
}
async fn payload(service: &Service, receipt: &IngestReceipt) -> Value {
    let text: String = sqlx::query_scalar("SELECT payload FROM oc_jobs WHERE id=?")
        .bind(receipt.job_id)
        .fetch_one(service.engine.relational().pool())
        .await
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

#[tokio::test]
async fn generic_system_tool_batches_publish_raw_source_and_search_all_modes() {
    let models = ModelServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(dir.path(), models.models()).await.unwrap();
    let (_, auth) = provision(&service).await;
    let (_, other) = provision(&service).await;
    let mut receipts = Vec::new();
    for id in ["one", "two"] {
        let input = batch(id);
        let receipt = service
            .submit_message_batch(&auth, id, input.clone(), models.spec())
            .await
            .unwrap();
        assert_eq!(
            status(&service, &auth, &receipt).await,
            PublicationStatus::Pending
        );
        assert!(worker::process_next(&service).await.unwrap());
        assert_eq!(
            status(&service, &auth, &receipt).await,
            PublicationStatus::Retrievable
        );
        let original = service
            .source_event(&auth, receipt.source_event_id)
            .await
            .unwrap();
        let source: Value = serde_json::from_str(original["content"].as_str().unwrap()).unwrap();
        assert_eq!(source, json!(input));
        assert!(source["messages"][2].get("timestamp").is_none());
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT content,locator FROM oc_chunks WHERE asset_id=? ORDER BY ordinal",
        )
        .bind(receipt.asset_id)
        .fetch_all(service.engine.relational().pool())
        .await
        .unwrap();
        for (index, message) in input.messages.iter().enumerate() {
            let parts: Vec<_> = rows
                .iter()
                .filter(|(_, locator)| {
                    serde_json::from_str::<Value>(locator).unwrap()["message_index"] == index
                })
                .collect();
            assert_eq!(
                parts
                    .iter()
                    .map(|(text, _)| text.as_str())
                    .collect::<String>(),
                message.content
            );
            for (text, locator) in parts {
                let locator: Value = serde_json::from_str(locator).unwrap();
                let start = locator["byte_start"].as_u64().unwrap() as usize;
                let end = locator["byte_end"].as_u64().unwrap() as usize;
                assert_eq!(text, &message.content[start..end]);
                assert_eq!(locator["parser"], origence::messages::MESSAGE_PARSER);
                assert_eq!(locator["role"], message.role);
                assert_eq!(locator["timestamp"], json!(message.timestamp));
                assert_eq!(locator["external_batch_id"], id);
                assert_eq!(locator["source_event_id"], json!(receipt.source_event_id));
            }
        }
        receipts.push(receipt);
    }
    for mode in ["keyword", "vector", "hybrid"] {
        let result = service.search(&auth, query(mode)).await.unwrap();
        let hits = result["hits"].as_array().unwrap();
        assert!(hits.iter().any(|hit| hit["locator"]["role"] == "system"));
        assert!(hits.iter().any(|hit| hit["locator"]["role"] == "tool"));
        for receipt in &receipts {
            assert!(
                hits.iter()
                    .any(|hit| hit["asset_id"] == json!(receipt.asset_id))
            );
        }
        assert!(
            service.search(&other, query(mode)).await.unwrap()["hits"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    assert!(matches!(
        service.message_batch_status(&other, &receipts[0]).await,
        Err(AppError::NotFound)
    ));
    assert_eq!(models.chats.load(Ordering::SeqCst), 0);
    assert_eq!(count(&service, "oc_summaries").await, 0);
    assert_eq!(count(&service, "oc_commands").await, 2);
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn workspace_replay_preserves_creator_and_all_spec_input_conflicts() {
    let models = ModelServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(dir.path(), models.models()).await.unwrap();
    let (_, auth) = provision(&service).await;
    let replacement = service.issue_key(&auth, "admin").await.unwrap();
    let other = service
        .auth(replacement["token"].as_str().unwrap())
        .await
        .unwrap();
    let input = batch("full:external:batch");
    let spec = models.spec();
    let (first, duplicate) = tokio::join!(
        service.submit_message_batch(&auth, "same-key", input.clone(), spec.clone()),
        service.submit_message_batch(&other, "same-key", input.clone(), spec.clone()),
    );
    let receipt = first.unwrap();
    assert_eq!(receipt, duplicate.unwrap());
    let creator: Uuid = sqlx::query_scalar("SELECT created_by FROM oc_jobs WHERE id=?")
        .bind(receipt.job_id)
        .fetch_one(service.engine.relational().pool())
        .await
        .unwrap();
    let command_creator: Uuid = sqlx::query_scalar(
        "SELECT principal_id FROM oc_commands WHERE operation='message_batch.ingest'",
    )
    .fetch_one(service.engine.relational().pool())
    .await
    .unwrap();
    assert_eq!(creator, command_creator);
    assert!([auth.id, other.id].contains(&creator));
    for change in 0..7 {
        let mut changed = input.clone();
        match change {
            0 => changed.batch_id.push('!'),
            1 => changed.session_id.push('!'),
            2 => changed.messages[0].content.push('!'),
            3 => changed.messages[0].role = "tool".into(),
            4 => changed.messages[0].timestamp = None,
            5 => changed.messages.swap(0, 1),
            _ => changed.messages[2].timestamp = Some(0),
        }
        assert!(matches!(
            service
                .submit_message_batch(&other, "same-key", changed, spec.clone())
                .await,
            Err(AppError::Conflict(_))
        ));
    }
    for change in 0..5 {
        let mut changed = spec.clone();
        match change {
            0 => changed.parser = "future-parser".into(),
            1 => changed.parser_version += 1,
            2 => changed.chunking_version += 1,
            3 => changed.raw_only = false,
            _ => changed.embedding_profile = "future-profile".into(),
        }
        assert!(matches!(
            service
                .submit_message_batch(&other, "same-key", input.clone(), changed)
                .await,
            Err(AppError::Conflict(_))
        ));
    }
    assert_eq!(
        payload(&service, &receipt).await["accepted_spec"],
        json!(spec)
    );
    assert_eq!(
        service
            .wait_message_batch(&auth, &receipt, Duration::from_millis(5))
            .await
            .unwrap(),
        PublicationStatus::Pending
    );
    assert!(
        tokio::time::timeout(
            Duration::from_millis(5),
            service.wait_message_batch(&auth, &receipt, Duration::from_secs(1))
        )
        .await
        .is_err()
    );
    assert_eq!(count(&service, "oc_jobs").await, 1);
    assert_eq!(count(&service, "oc_events").await, 1);
    assert_eq!(count(&service, "oc_assets").await, 1);
    assert_eq!(count(&service, "oc_commands").await, 1);
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn model_failure_is_atomic_and_explicit_retry_retains_original_receipt_spec() {
    let models = ModelServer::start().await;
    models.fail_from.store(2, Ordering::SeqCst);
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(dir.path(), models.models()).await.unwrap();
    let (_, auth) = provision(&service).await;
    let receipt = service
        .submit_message_batch(&auth, "retry", batch("retry"), models.spec())
        .await
        .unwrap();
    assert!(worker::process_next(&service).await.unwrap());
    assert_eq!(
        status(&service, &auth, &receipt).await,
        PublicationStatus::Failed
    );
    assert_eq!(count(&service, "oc_versions").await, 0);
    assert_eq!(count(&service, "oc_chunks").await, 0);
    assert!(
        service.search(&auth, query("keyword")).await.unwrap()["hits"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !service
            .job(&auth, receipt.job_id)
            .await
            .unwrap()
            .to_string()
            .contains("private provider")
    );
    assert_eq!(
        service
            .submit_message_batch(&auth, "retry", batch("retry"), models.spec())
            .await
            .unwrap(),
        receipt
    );
    models.fail_from.store(usize::MAX, Ordering::SeqCst);
    service
        .job_action(&auth, "explicit-retry", receipt.job_id, "retry")
        .await
        .unwrap();
    assert!(worker::process_next(&service).await.unwrap());
    assert_eq!(
        status(&service, &auth, &receipt).await,
        PublicationStatus::Retrievable
    );
    assert_eq!(
        service.job(&auth, receipt.job_id).await.unwrap()["generation"],
        2
    );
    assert_eq!(
        payload(&service, &receipt).await["accepted_spec"],
        json!(receipt.accepted_spec)
    );
    assert_eq!(count(&service, "oc_jobs").await, 1);
    assert_eq!(count(&service, "oc_versions").await, 1);
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn prepared_publication_recovers_same_generation_without_model_reexecution() {
    let models = ModelServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(dir.path(), models.models()).await.unwrap();
    let (token, auth) = provision(&service).await;
    let receipt = service
        .submit_message_batch(&auth, "recover", batch("recover"), models.spec())
        .await
        .unwrap();
    // Fail relational publication after the durable plan/native index prepare.
    sqlx::query("CREATE TRIGGER test_fail_chunks BEFORE INSERT ON oc_chunks BEGIN SELECT RAISE(ABORT,'test publication failure'); END")
        .execute(service.engine.relational().pool()).await.unwrap();
    assert!(worker::process_next(&service).await.unwrap());
    assert_eq!(count(&service, "oc_versions").await, 0);
    let saved = payload(&service, &receipt).await;
    let accepted = saved["accepted_spec"].clone();
    let ids: Vec<_> = saved["publication"]["memories"][0]["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|chunk| chunk["id"].clone())
        .collect();
    assert!(!ids.is_empty());
    let calls = models.calls.load(Ordering::SeqCst);
    sqlx::query("DROP TRIGGER test_fail_chunks")
        .execute(service.engine.relational().pool())
        .await
        .unwrap();
    // Model an already-due retry that was abandoned while processing. The
    // injected storage failure schedules retry_wait with a future retry time;
    // changing only its state would leave that backoff active after reopening.
    // Keep production recovery/backoff policy unchanged and make the fixture due.
    sqlx::query("UPDATE oc_jobs SET state='processing',next_retry_at=0 WHERE id=?")
        .bind(receipt.job_id)
        .execute(service.engine.relational().pool())
        .await
        .unwrap();
    service.engine.shutdown().await.unwrap();
    drop(service);
    let service = Service::open(dir.path(), models.models()).await.unwrap();
    let auth = service.auth(&token).await.unwrap();
    assert_eq!(
        service
            .submit_message_batch(&auth, "recover", batch("recover"), models.spec())
            .await
            .unwrap(),
        receipt
    );
    assert_eq!(
        status(&service, &auth, &receipt).await,
        PublicationStatus::Pending
    );
    assert!(worker::process_next(&service).await.unwrap());
    assert_eq!(
        status(&service, &auth, &receipt).await,
        PublicationStatus::Retrievable
    );
    assert_eq!(models.calls.load(Ordering::SeqCst), calls);
    let actual: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM oc_chunks WHERE asset_id=? ORDER BY ordinal")
            .bind(receipt.asset_id)
            .fetch_all(service.engine.relational().pool())
            .await
            .unwrap();
    assert_eq!(json!(actual), json!(ids));
    assert_eq!(payload(&service, &receipt).await["accepted_spec"], accepted);
    assert_eq!(count(&service, "oc_versions").await, 1);
    assert!(!worker::process_next(&service).await.unwrap());
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn changed_worker_configuration_fails_without_replacing_accepted_spec() {
    let models = ModelServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(dir.path(), models.models()).await.unwrap();
    let (token, auth) = provision(&service).await;
    let receipt = service
        .submit_message_batch(
            &auth,
            "configuration",
            batch("configuration"),
            models.spec(),
        )
        .await
        .unwrap();
    service.engine.shutdown().await.unwrap();
    drop(service);
    let mut changed = models.models();
    changed.profile = Some("changed-profile".into());
    let service = Service::open(dir.path(), changed).await.unwrap();
    let auth = service.auth(&token).await.unwrap();
    assert!(matches!(
        service
            .submit_message_batch(
                &auth,
                "configuration",
                batch("configuration"),
                AcceptedMessageSpec::raw_only("changed-profile")
            )
            .await,
        Err(AppError::Conflict(_))
    ));
    assert!(worker::process_next(&service).await.unwrap());
    assert_eq!(
        status(&service, &auth, &receipt).await,
        PublicationStatus::Failed
    );
    assert_eq!(
        payload(&service, &receipt).await["accepted_spec"],
        json!(receipt.accepted_spec)
    );
    assert_eq!(models.calls.load(Ordering::SeqCst), 0);
    assert_eq!(count(&service, "oc_jobs").await, 1);
    service.engine.shutdown().await.unwrap();
    drop(service);
    let service = Service::open(dir.path(), models.models()).await.unwrap();
    let auth = service.auth(&token).await.unwrap();
    service
        .job_action(&auth, "retry-original", receipt.job_id, "retry")
        .await
        .unwrap();
    assert!(worker::process_next(&service).await.unwrap());
    assert_eq!(
        status(&service, &auth, &receipt).await,
        PublicationStatus::Retrievable
    );
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn revoked_creator_cannot_publish_or_transfer_work_through_replay() {
    let models = ModelServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(dir.path(), models.models()).await.unwrap();
    let (_, auth) = provision(&service).await;
    let replacement = service.issue_key(&auth, "admin").await.unwrap();
    let rotated = service
        .auth(replacement["token"].as_str().unwrap())
        .await
        .unwrap();
    let receipt = service
        .submit_message_batch(&auth, "revocation", batch("revocation"), models.spec())
        .await
        .unwrap();
    models.hold.store(true, Ordering::SeqCst);
    let running = service.clone();
    let task = tokio::spawn(async move { worker::process_next(&running).await });
    tokio::time::timeout(Duration::from_secs(5), models.entered.notified())
        .await
        .unwrap();
    service.revoke_key(&rotated, auth.id).await.unwrap();
    assert_eq!(
        service
            .submit_message_batch(&rotated, "revocation", batch("revocation"), models.spec())
            .await
            .unwrap(),
        receipt
    );
    models.release.notify_one();
    assert!(task.await.unwrap().unwrap());
    assert_eq!(
        status(&service, &rotated, &receipt).await,
        PublicationStatus::Failed
    );
    assert!(matches!(
        service.message_batch_status(&auth, &receipt).await,
        Err(AppError::Forbidden)
    ));
    assert_eq!(count(&service, "oc_versions").await, 0);
    assert!(
        service
            .job_action(&rotated, "no-transfer", receipt.job_id, "retry")
            .await
            .is_err()
    );
    assert_eq!(
        service.job(&rotated, receipt.job_id).await.unwrap()["generation"],
        1
    );
    assert!(!worker::process_next(&service).await.unwrap());
    assert_eq!(
        status(&service, &rotated, &receipt).await,
        PublicationStatus::Failed
    );
    let creator: Uuid = sqlx::query_scalar("SELECT created_by FROM oc_jobs WHERE id=?")
        .bind(receipt.job_id)
        .fetch_one(service.engine.relational().pool())
        .await
        .unwrap();
    assert_eq!(creator, auth.id);
    assert_eq!(count(&service, "oc_jobs").await, 1);
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn live_receipt_status_requires_every_chunk_profile_generation_and_source() {
    let models = ModelServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(dir.path(), models.models()).await.unwrap();
    let (_, auth) = provision(&service).await;
    let receipt = service
        .submit_message_batch(&auth, "visibility", batch("visibility"), models.spec())
        .await
        .unwrap();
    assert!(worker::process_next(&service).await.unwrap());
    assert_eq!(
        status(&service, &auth, &receipt).await,
        PublicationStatus::Retrievable
    );
    let chunk: Uuid =
        sqlx::query_scalar("SELECT id FROM oc_chunks WHERE asset_id=? ORDER BY ordinal LIMIT 1")
            .bind(receipt.asset_id)
            .fetch_one(service.engine.relational().pool())
            .await
            .unwrap();
    for (break_sql, restore_sql) in [
        (
            "UPDATE oc_index_entries SET state='removed' WHERE artifact_id=?",
            "UPDATE oc_index_entries SET state='ready' WHERE artifact_id=?",
        ),
        (
            "UPDATE oc_index_entries SET model_id='wrong' WHERE artifact_id=?",
            "UPDATE oc_index_entries SET model_id=(SELECT json_extract(payload,'$.accepted_spec.embedding_profile') FROM oc_jobs WHERE operation='message_batch_ingest') WHERE artifact_id=?",
        ),
        (
            "UPDATE oc_index_entries SET generation=99 WHERE artifact_id=?",
            "UPDATE oc_index_entries SET generation=1 WHERE artifact_id=?",
        ),
        (
            "UPDATE oc_artifact_ledger SET generation=99 WHERE artifact_id=?",
            "UPDATE oc_artifact_ledger SET generation=1 WHERE artifact_id=?",
        ),
        (
            "UPDATE oc_artifact_ledger SET state='orphan' WHERE artifact_id=?",
            "UPDATE oc_artifact_ledger SET state='committed' WHERE artifact_id=?",
        ),
    ] {
        sqlx::query(break_sql)
            .bind(chunk)
            .execute(service.engine.relational().pool())
            .await
            .unwrap();
        assert_eq!(
            status(&service, &auth, &receipt).await,
            PublicationStatus::Invisible
        );
        assert_eq!(
            service
                .submit_message_batch(&auth, "visibility", batch("visibility"), models.spec())
                .await
                .unwrap(),
            receipt
        );
        sqlx::query(restore_sql)
            .bind(chunk)
            .execute(service.engine.relational().pool())
            .await
            .unwrap();
        assert_eq!(
            status(&service, &auth, &receipt).await,
            PublicationStatus::Retrievable
        );
    }
    for (break_sql, restore_sql, id) in [
        (
            "UPDATE oc_events SET state='retracted' WHERE id=?",
            "UPDATE oc_events SET state='active' WHERE id=?",
            receipt.source_event_id,
        ),
        (
            "UPDATE oc_assets SET current_version=2 WHERE id=?",
            "UPDATE oc_assets SET current_version=1 WHERE id=?",
            receipt.asset_id,
        ),
        (
            "UPDATE oc_assets SET deleted=1 WHERE id=?",
            "UPDATE oc_assets SET deleted=0 WHERE id=?",
            receipt.asset_id,
        ),
    ] {
        sqlx::query(break_sql)
            .bind(id)
            .execute(service.engine.relational().pool())
            .await
            .unwrap();
        assert_eq!(
            status(&service, &auth, &receipt).await,
            PublicationStatus::Invisible
        );
        sqlx::query(restore_sql)
            .bind(id)
            .execute(service.engine.relational().pool())
            .await
            .unwrap();
        assert_eq!(
            status(&service, &auth, &receipt).await,
            PublicationStatus::Retrievable
        );
    }
    sqlx::query("DELETE FROM oc_chunks WHERE id=?")
        .bind(chunk)
        .execute(service.engine.relational().pool())
        .await
        .unwrap();
    assert_eq!(
        status(&service, &auth, &receipt).await,
        PublicationStatus::Invisible
    );
    let mut forged = receipt.clone();
    forged.accepted_spec.embedding_profile = "other".into();
    assert!(matches!(
        service.message_batch_status(&auth, &forged).await,
        Err(AppError::Conflict(_))
    ));
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn service_and_worker_reject_unbounded_source_and_cancelled_status_is_terminal() {
    let models = ModelServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(dir.path(), models.models()).await.unwrap();
    let (_, auth) = provision(&service).await;
    for invalid_role in ["".to_owned(), "tool\n".into(), "x".repeat(65)] {
        let mut input = batch("invalid");
        input.messages[0].role = invalid_role;
        assert!(matches!(
            service
                .submit_message_batch(&auth, "invalid", input, models.spec())
                .await,
            Err(AppError::Invalid(_))
        ));
    }
    assert_eq!(count(&service, "oc_jobs").await, 0);
    let receipt = service
        .submit_message_batch(
            &auth,
            "worker-invalid",
            batch("worker-invalid"),
            models.spec(),
        )
        .await
        .unwrap();
    // Simulates a corrupt importer bypassing the public service boundary.
    sqlx::query("UPDATE oc_events SET content=json_set(content,'$.messages[0].role',?) WHERE id=?")
        .bind("x".repeat(65))
        .bind(receipt.source_event_id)
        .execute(service.engine.relational().pool())
        .await
        .unwrap();
    assert!(worker::process_next(&service).await.unwrap());
    assert_eq!(
        status(&service, &auth, &receipt).await,
        PublicationStatus::Failed
    );
    assert_eq!(models.calls.load(Ordering::SeqCst), 0);
    let cancelled = service
        .submit_message_batch(&auth, "cancel", batch("cancel"), models.spec())
        .await
        .unwrap();
    service
        .job_action(&auth, "cancel-action", cancelled.job_id, "cancel")
        .await
        .unwrap();
    assert_eq!(
        service
            .wait_message_batch(&auth, &cancelled, Duration::from_secs(1))
            .await
            .unwrap(),
        PublicationStatus::CancelledOrSuperseded
    );
    assert!(!worker::process_next(&service).await.unwrap());
    assert_eq!(count(&service, "oc_chunks").await, 0);
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn source_withdrawal_during_model_io_prevents_the_entire_publication() {
    let models = ModelServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(dir.path(), models.models()).await.unwrap();
    let (_, auth) = provision(&service).await;
    let receipt = service
        .submit_message_batch(&auth, "withdraw", batch("withdraw"), models.spec())
        .await
        .unwrap();
    models.hold.store(true, Ordering::SeqCst);
    let running = service.clone();
    let task = tokio::spawn(async move { worker::process_next(&running).await });
    tokio::time::timeout(Duration::from_secs(5), models.entered.notified())
        .await
        .unwrap();
    service
        .delete(&auth, "withdraw-source", receipt.source_event_id, "event")
        .await
        .unwrap();
    models.release.notify_one();
    assert!(task.await.unwrap().unwrap());
    assert_eq!(
        status(&service, &auth, &receipt).await,
        PublicationStatus::Invisible
    );
    assert_eq!(count(&service, "oc_versions").await, 0);
    assert_eq!(count(&service, "oc_chunks").await, 0);
    assert!(
        service.search(&auth, query("keyword")).await.unwrap()["hits"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn owned_writer_capability_and_role_are_rechecked_after_embedding_io() {
    for change in [
        "UPDATE oc_api_keys SET allow_owned_workspaces=0 WHERE id=?",
        "UPDATE oc_api_keys SET role='reader' WHERE id=?",
    ] {
        let models = ModelServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let service = Service::open(dir.path(), models.models()).await.unwrap();
        let (_, root) = provision(&service).await;
        let key = service.issue_key_with_capabilities(
            &root,
            "writer",
            origence::storage::KeyCapabilities { allow_owned_workspaces: true },
        ).await.unwrap();
        let capable = service.auth(key["token"].as_str().unwrap()).await.unwrap();
        let child = service.ensure_owned_workspace(&capable, Uuid::new_v4(), "Child").await.unwrap();
        // Independent live authority can inspect the failed original task, but
        // cannot take over the revoked/downgraded creator's execution identity.
        let observer_key = service.engine.relational().issue_key(
            Scope { tenant_id: child.tenant_id, workspace_id: child.workspace_id }, "admin",
        ).await.unwrap();
        let observer = service.auth(&observer_key.token).await.unwrap();
        let receipt = service.submit_message_batch(&child, "owned-io", batch("owned-io"), models.spec()).await.unwrap();
        models.hold.store(true, Ordering::SeqCst);
        let running = service.clone();
        let task = tokio::spawn(async move { worker::process_next(&running).await });
        tokio::time::timeout(Duration::from_secs(5), models.entered.notified()).await.unwrap();
        sqlx::query(change).bind(capable.id).execute(service.engine.relational().pool()).await.unwrap();
        models.release.notify_one();
        assert!(task.await.unwrap().unwrap());
        assert_eq!(status(&service, &observer, &receipt).await, PublicationStatus::Failed);
        assert_eq!(service.job(&observer, receipt.job_id).await.unwrap()["error_code"], "FORBIDDEN");
        assert_eq!(count(&service, "oc_versions").await, 0);
        assert_eq!(count(&service, "oc_chunks").await, 0);
        assert!(matches!(service.job_action(&observer, "no-owned-transfer", receipt.job_id, "retry").await, Err(AppError::Forbidden)));
        assert!(service.search(&observer, query("keyword")).await.unwrap()["hits"].as_array().unwrap().is_empty());
        let creator: Uuid = sqlx::query_scalar("SELECT created_by FROM oc_jobs WHERE id=?")
            .bind(receipt.job_id).fetch_one(service.engine.relational().pool()).await.unwrap();
        assert_eq!(creator, capable.id);
        service.engine.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn owned_reader_search_rechecks_capability_after_query_embedding_io() {
    let models = ModelServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(dir.path(), models.models()).await.unwrap();
    let (_, root) = provision(&service).await;
    let writer_key = service.issue_key_with_capabilities(
        &root, "writer", origence::storage::KeyCapabilities { allow_owned_workspaces: true },
    ).await.unwrap();
    let writer = service.auth(writer_key["token"].as_str().unwrap()).await.unwrap();
    let child = service.ensure_owned_workspace(&writer, Uuid::new_v4(), "Search child").await.unwrap();
    let receipt = service.submit_message_batch(&child, "reader-io", batch("reader-io"), models.spec()).await.unwrap();
    assert!(worker::process_next(&service).await.unwrap());
    assert_eq!(status(&service, &child, &receipt).await, PublicationStatus::Retrievable);
    let reader_key = service.issue_key_with_capabilities(
        &root, "reader", origence::storage::KeyCapabilities { allow_owned_workspaces: true },
    ).await.unwrap();
    let reader = service.auth(reader_key["token"].as_str().unwrap()).await.unwrap();
    let reading_child = service.lookup_owned_workspace(&reader, child.workspace_id).await.unwrap().unwrap();
    assert!(!service.search(&reading_child, query("vector")).await.unwrap()["hits"].as_array().unwrap().is_empty());
    models.hold.store(true, Ordering::SeqCst);
    let searching = service.clone();
    let task = tokio::spawn(async move { searching.search(&reading_child, query("vector")).await });
    tokio::time::timeout(Duration::from_secs(5), models.entered.notified()).await.unwrap();
    sqlx::query("UPDATE oc_api_keys SET allow_owned_workspaces=0 WHERE id=?")
        .bind(reader.id).execute(service.engine.relational().pool()).await.unwrap();
    models.release.notify_one();
    assert!(matches!(task.await.unwrap(), Err(AppError::Forbidden)));
    // Revoking only the reader's derived access neither hides nor republishes
    // the valid source for a still-authorized writer.
    assert_eq!(status(&service, &child, &receipt).await, PublicationStatus::Retrievable);
    assert_eq!(count(&service, "oc_versions").await, 1);
    service.engine.shutdown().await.unwrap();
}
