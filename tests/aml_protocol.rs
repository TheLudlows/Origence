//! Public AML library and real HTTP regressions with a deterministic model server.
#![cfg(feature = "local-storage")]
use origence::storage::native_turso as db;
use axum::{Json, Router, routing::post};
use origence::{
    api::aml::{self, AddInput},
    error::AppError,
    messages::{AcceptedMessageSpec, IngestReceipt, MESSAGE_PARSER, PublicationStatus},
    models::Models,
    service::Service,
    storage::{KeyCapabilities, RelationalStore, Scope},
    types::AuthContext,
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
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use uuid::Uuid;

struct ModelServer {
    base: String,
    fail: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
    fail_from: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}
impl ModelServer {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let fail = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let fail_from = Arc::new(AtomicUsize::new(usize::MAX));
        let (f, c, after) = (fail.clone(), calls.clone(), fail_from.clone());
        let router = Router::new().route(
            "/embeddings",
            post(move |Json(input): Json<Value>| {
                let (f, c, after) = (f.clone(), c.clone(), after.clone());
                async move {
                    let call = c.fetch_add(1, Ordering::SeqCst) + 1;
                    if f.load(Ordering::SeqCst) || call >= after.load(Ordering::SeqCst) {
                        return (
                            axum::http::StatusCode::SERVICE_UNAVAILABLE,
                            Json(json!({"error":"private provider diagnostic"})),
                        );
                    }
                    assert_eq!(input["dimensions"], 2);
                    // All source chunks remain candidates; ordering is tested for stability.
                    (
                        axum::http::StatusCode::OK,
                        Json(json!({"data":[{"embedding":[1.0,0.5]}]})),
                    )
                }
            }),
        );
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            base,
            fail,
            calls,
            fail_from,
            task,
        }
    }
    fn models(&self) -> Models {
        Models::configured(
            self.base.clone(),
            "test-only".into(),
            Some("fixture-v1".into()),
            None,
            2,
        )
        .unwrap()
    }
}
impl Drop for ModelServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn namespace(s: &Service) -> (String, AuthContext) {
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    s.engine
        .relational()
        .create_workspace(scope.tenant_id, scope.workspace_id, "AML test")
        .await
        .unwrap();
    let key = s
        .engine
        .relational()
        .issue_key_with_capabilities(
            scope,
            "admin",
            KeyCapabilities {
                allow_owned_workspaces: true,
            },
        )
        .await
        .unwrap();
    let auth = s.auth(&key.token).await.unwrap();
    (key.token, auth)
}
fn add(user: &str, request: &str, session: &str, content: &str) -> AddInput {
    serde_json::from_value(json!({"request_id":request,"user_id":user,"session_id":session,"messages":[{"role":"user","content":content,"timestamp":1720000000000i64},{"role":"assistant","content":"Acknowledged. 已记录。"}]})).unwrap()
}
fn search(user: &str, top_k: usize) -> aml::SearchInput {
    aml::SearchInput {
        query: "Atlas approval".into(),
        user_id: user.into(),
        top_k,
        options: Some(vec!["never store this option".into()]),
    }
}
async fn count(s: &Service, table: &str) -> i64 {
    // Only fixed test table names enter this query.
    db::query_scalar(&format!("SELECT count(*) FROM {table}"))
        .fetch_one(s.engine.relational().pool())
        .await
        .unwrap()
}

async fn user_scope(s: &Service, root: &AuthContext, user: &str) -> AuthContext {
    s.lookup_owned_workspace(
        root,
        aml::workspace_id(root.tenant_id, root.workspace_id, user),
    )
    .await
    .unwrap()
    .unwrap()
}
async fn submit(
    s: &Service,
    root: &AuthContext,
    input: AddInput,
) -> origence::error::Result<IngestReceipt> {
    input.validate()?;
    let target = aml::workspace_id(root.tenant_id, root.workspace_id, &input.user_id);
    let user = s
        .ensure_owned_workspace(root, target, "AML test user")
        .await?;
    s.submit_message_batch(
        &user,
        &aml::add_command_key(&input.request_id),
        input.into_batch(),
        AcceptedMessageSpec::raw_only(s.models.profile.clone().unwrap()),
    )
    .await
}

#[tokio::test]
async fn aml_routes_are_disabled_by_default_and_namespace_endpoint_is_removed() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = origence::api::router(s.clone());
    let serving = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    for path in ["/aml/add", "/aml/search", "/admin/aml/namespace"] {
        assert_eq!(
            reqwest::Client::new()
                .post(format!("{base}{path}"))
                .json(&json!({}))
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
    }
    serving.abort();
    s.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn aml_http_add_search_isolation_and_immediate_visibility() {
    let models = ModelServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), models.models()).await.unwrap();
    let (token, auth) = namespace(&s).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (stop, stopped) = oneshot::channel();
    let serving = tokio::spawn(origence::host::serve_with_options(
        s.clone(),
        listener,
        origence::api::ApiOptions { enable_aml: true },
        async {
            let _ = stopped.await;
        },
    ));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    let response = client
        .post(format!("{base}/admin/aml/namespace"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 404);
    let long_id = format!("run:甲:{}", "a".repeat(400));
    let original = add(
        &long_id,
        "request:完整",
        "session-one",
        &"Atlas 必须两人审批。🙂 café é\n".repeat(160),
    );
    for input in [
        original.clone(),
        add(
            &long_id,
            "second",
            "session-two",
            "Atlas launched on Monday.",
        ),
        add(
            &long_id,
            "third-same-session",
            "session-one",
            "Atlas additional evidence in a second batch for this session.",
        ),
        add(
            "run:乙",
            "request:完整",
            "session-one",
            "Atlas 不需要审批。",
        ),
    ] {
        let response = client
            .post(format!("{base}/aml/add"))
            .bearer_auth(&token)
            .json(&input)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let result: Value = response.json().await.unwrap();
        assert_eq!(status, 200, "{result}");
        assert_eq!(
            result,
            json!({"success":true,"request_id":input.request_id,"user_id":input.user_id,"session_id":input.session_id})
        );
        let results: Value = client
            .post(format!("{base}/aml/search"))
            .bearer_auth(&token)
            .json(&search(&input.user_id, 100))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        let mut ranges = Vec::new();
        for hit in results["data"].as_array().unwrap() {
            let (header, content) = hit["content"].as_str().unwrap().split_once('\n').unwrap();
            let metadata: Value =
                serde_json::from_str(header.strip_prefix("Source metadata: ").unwrap()).unwrap();
            if metadata["external_batch_id"] != input.request_id {
                continue;
            }
            let index = metadata["message_index"].as_u64().unwrap() as usize;
            let message = &input.messages[index];
            let start = metadata["byte_start"].as_u64().unwrap() as usize;
            let end = metadata["byte_end"].as_u64().unwrap() as usize;
            assert_eq!(metadata["parser"], MESSAGE_PARSER);
            assert_eq!(metadata["session_id"], input.session_id);
            assert_eq!(metadata["parser_version"], 1);
            assert_eq!(metadata["chunking_version"], 1);
            Uuid::parse_str(metadata["source_event_id"].as_str().unwrap()).unwrap();
            assert_eq!(metadata["byte_basis"], "message_content_utf8");
            assert_eq!(
                metadata["source_path"],
                format!("/messages/{index}/content")
            );
            assert_eq!(metadata["message_count"], input.messages.len());
            assert_eq!(metadata["timestamp"], json!(message.timestamp));
            assert_eq!(metadata["role"], json!(message.role));
            assert_eq!(content, &message.content[start..end]);
            if index == 0 {
                ranges.push((start, end));
            }
        }
        ranges.sort_unstable();
        let mut cursor = 0;
        for (start, end) in ranges {
            assert_eq!(start, cursor);
            cursor = end;
        }
        assert_eq!(cursor, input.messages[0].content.len());
    }
    let found = aml::search(&s, &auth, search(&long_id, 100)).await.unwrap();
    assert_eq!(
        found["data"].as_array().unwrap().len(),
        origence::messages::chunks(
            &serde_json::to_string(&original.clone().into_batch()).unwrap(),
            &AcceptedMessageSpec::raw_only("fixture"),
            Uuid::nil(),
        )
        .unwrap()
        .len()
            + 4
    );
    assert!(!found.to_string().contains("不需要审批"));
    assert!(!found.to_string().contains("never store"));
    assert!(found.to_string().contains("1720000000000"));
    assert!(found.to_string().contains("session-one") && found.to_string().contains("session-two"));
    assert_eq!(
        found,
        aml::search(&s, &auth, search(&long_id, 100)).await.unwrap()
    );
    assert_eq!(
        aml::search(&s, &auth, search(&long_id, 1)).await.unwrap()["data"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let before = count(&s, "oc_workspaces").await;
    assert_eq!(
        aml::search(&s, &auth, search("unknown", 100))
            .await
            .unwrap(),
        json!({"data":[]})
    );
    assert_eq!(count(&s, "oc_workspaces").await, before);
    let replay: Value = client
        .post(format!("{base}/aml/add"))
        .bearer_auth(&token)
        .json(&original)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(replay["success"], true);
    assert_eq!(count(&s, "oc_commands").await, 4);
    assert_eq!(count(&s, "oc_versions").await, 4);
    let mut conflict = original.clone();
    conflict.messages[0].content.push('!');
    assert_eq!(
        client
            .post(format!("{base}/aml/add"))
            .bearer_auth(&token)
            .json(&conflict)
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    let bad = json!({"request_id":"invalid","user_id":"private-secret","session_id":"s","messages":[{"role":"system-secret","content":"private-source"}]});
    let rejected = client
        .post(format!("{base}/aml/add"))
        .bearer_auth(&token)
        .json(&bad)
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), 422);
    let body = rejected.text().await.unwrap();
    assert!(!body.contains("secret") && !body.contains("private-source"));
    assert_eq!(
        client
            .post(format!("{base}/aml/search"))
            .json(&search(&long_id, 1))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let reader = s.issue_key(&auth, "reader").await.unwrap();
    assert_eq!(
        client
            .post(format!("{base}/aml/add"))
            .bearer_auth(reader["token"].as_str().unwrap())
            .json(&original)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    // New keys do not inherit root capability, including admin keys. Even an
    // unknown user's empty result requires authorization before lookup.
    let ordinary = s.issue_key(&auth, "admin").await.unwrap();
    for key in [
        reader["token"].as_str().unwrap(),
        ordinary["token"].as_str().unwrap(),
    ] {
        assert_eq!(
            client
                .post(format!("{base}/aml/search"))
                .bearer_auth(key)
                .json(&search("unknown", 1))
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
    }
    let capable_reader: Value = client.post(format!("{base}/admin/keys"))
        .bearer_auth(&token)
        .json(&json!({"workspace_id":auth.workspace_id,"role":"reader","allow_owned_workspaces":true}))
        .send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
    let reader_token = capable_reader["token"].as_str().unwrap();
    assert_eq!(
        client
            .post(format!("{base}/aml/search"))
            .bearer_auth(reader_token)
            .json(&search(&long_id, 1))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        client
            .post(format!("{base}/aml/add"))
            .bearer_auth(reader_token)
            .json(&original)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(count(&s, "oc_workspaces").await, before);
    models.fail.store(true, Ordering::SeqCst);
    assert_eq!(
        client
            .post(format!("{base}/aml/search"))
            .bearer_auth(&token)
            .json(&search(&long_id, 1))
            .send()
            .await
            .unwrap()
            .status(),
        503
    );
    assert!(models.calls.load(Ordering::SeqCst) >= 6);
    stop.send(()).unwrap();
    serving.await.unwrap().unwrap();
}

#[tokio::test]
async fn aml_pending_retry_restart_and_key_rotation_reuse_one_batch() {
    let models = ModelServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), models.models()).await.unwrap();
    let (token, auth) = namespace(&s).await;
    let input = add(
        "Alice",
        "same-id",
        "session-one",
        "Atlas 决策 requires approval.",
    );
    let (a, b) = tokio::join!(
        submit(&s, &auth, input.clone()),
        submit(&s, &auth, input.clone())
    );
    let record = a.unwrap();
    assert_eq!(record.job_id, b.unwrap().job_id);
    assert!(matches!(
        s.wait_message_batch(
            &user_scope(&s, &auth, "Alice").await,
            &record,
            Duration::from_millis(10)
        )
        .await,
        Ok(PublicationStatus::Pending)
    ));
    // Dropping an observer models a lost HTTP connection without cancelling the job.
    assert!(
        tokio::time::timeout(
            Duration::from_millis(10),
            aml::add(&s, &auth, input.clone())
        )
        .await
        .is_err()
    );
    let alice = user_scope(&s, &auth, "Alice").await;
    assert_eq!(
        s.job(&alice, record.job_id).await.unwrap()["state"],
        "pending"
    );
    assert_eq!(count(&s, "oc_jobs").await, 1);
    s.engine.shutdown().await.unwrap();
    drop(s);
    let s = Service::open(dir.path(), models.models()).await.unwrap();
    let auth = s.auth(&token).await.unwrap();
    let replacement = s
        .issue_key_with_capabilities(
            &auth,
            "admin",
            KeyCapabilities {
                allow_owned_workspaces: true,
            },
        )
        .await
        .unwrap();
    let rotated = s
        .auth(replacement["token"].as_str().unwrap())
        .await
        .unwrap();
    assert_eq!(
        submit(&s, &rotated, input.clone()).await.unwrap().job_id,
        record.job_id
    );
    assert!(worker::process_next(&s).await.unwrap());
    assert!(
        s.wait_message_batch(
            &user_scope(&s, &rotated, "Alice").await,
            &record,
            Duration::from_secs(1)
        )
        .await
        .unwrap()
            == PublicationStatus::Retrievable
    );
    assert_eq!(
        submit(&s, &rotated, input.clone()).await.unwrap().asset_id,
        record.asset_id
    );
    assert_eq!(count(&s, "oc_versions").await, 1);
    assert_eq!(count(&s, "oc_events").await, 1);
    let content: String = db::query_scalar("SELECT content FROM oc_events WHERE id=?")
        .bind(record.source_event_id)
        .fetch_one(s.engine.relational().pool())
        .await
        .unwrap();
    let source: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(
        source["messages"][0]["content"],
        "Atlas 决策 requires approval."
    );
    assert!(source["messages"][1].get("timestamp").is_none());
    let locators: Vec<(String, String)> =
        db::query_as("SELECT content,locator FROM oc_chunks WHERE asset_id=? ORDER BY ordinal")
            .bind(record.asset_id)
            .fetch_all(s.engine.relational().pool())
            .await
            .unwrap();
    for (content, loc) in locators {
        let loc: Value = serde_json::from_str(&loc).unwrap();
        assert_eq!(loc["parser"], MESSAGE_PARSER);
        let original = source
            .pointer(loc["source_path"].as_str().unwrap())
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(
            content,
            original[loc["byte_start"].as_u64().unwrap() as usize
                ..loc["byte_end"].as_u64().unwrap() as usize]
        );
    }
    let mut changed = models.models();
    changed.profile = Some("different-profile".into());
    let mismatched = Service {
        models: changed,
        ..s.clone()
    };
    assert!(matches!(
        submit(
            &mismatched,
            &rotated,
            add(
                "Alice",
                "same-id",
                "session-one",
                "Atlas 决策 requires approval."
            )
        )
        .await,
        Err(AppError::Conflict(_))
    ));
    let original_receipt: String = db::query_scalar(
        "SELECT response FROM oc_commands WHERE operation='message_batch.ingest' AND dedup_scope='workspace'",
    )
    .fetch_one(s.engine.relational().pool())
    .await
    .unwrap();
    // Removed evidence must never turn an old successful receipt into false success.
    db::query("UPDATE oc_events SET state='retracted' WHERE id=?")
        .bind(record.source_event_id)
        .execute(s.engine.relational().pool())
        .await
        .unwrap();
    assert!(matches!(
        s.wait_message_batch(
            &user_scope(&s, &rotated, "Alice").await,
            &record,
            Duration::from_secs(1)
        )
        .await,
        Ok(PublicationStatus::Invisible)
    ));
    // Exercise the public replay response after withdrawal, not only the
    // generic observer: an old durable receipt is never cached HTTP success.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = origence::api::router_with_options(
        s.clone(),
        origence::api::ApiOptions { enable_aml: true },
    );
    let (stop, stopped) = oneshot::channel();
    let serving = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let replay = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap()
        .post(format!("{base}/aml/add"))
        .bearer_auth(replacement["token"].as_str().unwrap())
        .json(&input)
        .send()
        .await
        .unwrap();
    assert_eq!(replay.status(), 409);
    let error: Value = replay.json().await.unwrap();
    assert_eq!(
        error["error"]["message"],
        "AML evidence is no longer searchable"
    );
    let after_receipt: String = db::query_scalar(
        "SELECT response FROM oc_commands WHERE operation='message_batch.ingest' AND dedup_scope='workspace'",
    )
    .fetch_one(s.engine.relational().pool())
    .await
    .unwrap();
    assert_eq!(after_receipt, original_receipt);
    assert_eq!(count(&s, "oc_commands").await, 1);
    for (table, expected) in [
        ("oc_jobs", record.job_id),
        ("oc_assets", record.asset_id),
        ("oc_events", record.source_event_id),
    ] {
        // Only fixed test table names enter this query.
        let ids: Vec<Uuid> = db::query_scalar(&format!("SELECT id FROM {table}"))
            .fetch_all(s.engine.relational().pool())
            .await
            .unwrap();
        assert_eq!(ids, vec![expected]);
    }
    let state: String = db::query_scalar("SELECT state FROM oc_events WHERE id=?")
        .bind(record.source_event_id)
        .fetch_one(s.engine.relational().pool())
        .await
        .unwrap();
    assert_eq!(state, "retracted");
    assert_eq!(count(&s, "oc_versions").await, 1);
    stop.send(()).unwrap();
    serving.await.unwrap();
    s.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn aml_model_failure_never_publishes_partial_batch_and_retry_uses_original_job() {
    let models = ModelServer::start().await;
    models.fail_from.store(2, Ordering::SeqCst);
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), models.models()).await.unwrap();
    let (_, auth) = namespace(&s).await;
    let input = add("Alice", "retry", "s", "Atlas 多语言 evidence");
    let record = submit(&s, &auth, input.clone()).await.unwrap();
    assert!(worker::process_next(&s).await.unwrap());
    assert_eq!(count(&s, "oc_versions").await, 0);
    assert_eq!(count(&s, "oc_chunks").await, 0);
    assert_eq!(
        submit(&s, &auth, input).await.unwrap().job_id,
        record.job_id
    );
    let alice = user_scope(&s, &auth, "Alice").await;
    let job = s.job(&alice, record.job_id).await.unwrap();
    assert!(matches!(
        job["state"].as_str(),
        Some("retry_wait" | "failed")
    ));
    assert!(!job.to_string().contains("private provider"));
    models.fail_from.store(usize::MAX, Ordering::SeqCst);
    assert_eq!(job["state"], "failed");
    assert!(matches!(
        s.wait_message_batch(&alice, &record, Duration::from_secs(1))
            .await,
        Ok(PublicationStatus::Failed)
    ));
    s.job_action(&alice, "retry-model", record.job_id, "retry")
        .await
        .unwrap();
    assert!(worker::process_next(&s).await.unwrap());
    assert!(
        s.wait_message_batch(&alice, &record, Duration::from_secs(1))
            .await
            .unwrap()
            == PublicationStatus::Retrievable
    );
    assert_eq!(count(&s, "oc_versions").await, 1);
    assert_eq!(count(&s, "oc_jobs").await, 1);
    s.engine.shutdown().await.unwrap();
}
