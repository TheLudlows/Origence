//! Public AML library and real HTTP regressions with a deterministic model server.
#![cfg(feature = "local-storage")]
use axum::{Json, Router, routing::post};
use origence::{
    aml::{self, AddInput},
    error::AppError,
    models::Models,
    service::Service,
    storage::{RelationalStore, Scope},
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
        .issue_key(scope, "admin")
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
    sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
        .fetch_one(s.engine.relational().pool())
        .await
        .unwrap()
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
    let serving = tokio::spawn(origence::host::serve(s.clone(), listener, async {
        let _ = stopped.await;
    }));
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
    assert_eq!(response.status(), 200);
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
            if metadata["session_id"] != input.session_id {
                continue;
            }
            let index = metadata["message_index"].as_u64().unwrap() as usize;
            let message = &input.messages[index];
            let start = metadata["byte_start"].as_u64().unwrap() as usize;
            let end = metadata["byte_end"].as_u64().unwrap() as usize;
            assert_eq!(metadata["parser"], aml::MESSAGE_PARSER);
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
    let found = s.aml_search(&auth, search(&long_id, 100)).await.unwrap();
    assert_eq!(
        found["data"].as_array().unwrap().len(),
        aml::chunks(&serde_json::to_string(&original).unwrap())
            .unwrap()
            .len()
            + 2
    );
    assert!(!found.to_string().contains("不需要审批"));
    assert!(!found.to_string().contains("never store"));
    assert!(found.to_string().contains("1720000000000"));
    assert!(found.to_string().contains("session-one") && found.to_string().contains("session-two"));
    assert_eq!(
        found,
        s.aml_search(&auth, search(&long_id, 100)).await.unwrap()
    );
    assert_eq!(
        s.aml_search(&auth, search(&long_id, 1)).await.unwrap()["data"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let before = count(&s, "oc_aml_users").await;
    assert_eq!(
        s.aml_search(&auth, search("unknown", 100)).await.unwrap(),
        json!({"data":[]})
    );
    assert_eq!(count(&s, "oc_aml_users").await, before);
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
    assert_eq!(count(&s, "oc_aml_adds").await, 3);
    assert_eq!(count(&s, "oc_versions").await, 3);
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
    s.enable_aml_namespace(&auth).await.unwrap();
    let input = add(
        "Alice",
        "same-id",
        "session-one",
        "Atlas 决策 requires approval.",
    );
    let (a, b) = tokio::join!(
        s.submit_aml_add(&auth, input.clone()),
        s.submit_aml_add(&auth, input.clone())
    );
    let record = a.unwrap();
    assert_eq!(record.job_id, b.unwrap().job_id);
    assert!(matches!(
        s.wait_aml_add(&auth, "Alice", "same-id", Duration::from_millis(10))
            .await,
        Err(AppError::Unavailable(_))
    ));
    // Dropping an observer models a lost HTTP connection without cancelling the job.
    assert!(
        tokio::time::timeout(Duration::from_millis(10), s.aml_add(&auth, input.clone()))
            .await
            .is_err()
    );
    let alice = s.lookup_aml_user(&auth, "Alice").await.unwrap().unwrap();
    assert_eq!(
        s.job(&alice, record.job_id).await.unwrap()["state"],
        "pending"
    );
    assert_eq!(count(&s, "oc_jobs").await, 1);
    s.engine.shutdown().await.unwrap();
    drop(s);
    let s = Service::open(dir.path(), models.models()).await.unwrap();
    let auth = s.auth(&token).await.unwrap();
    let replacement = s.issue_key(&auth, "admin").await.unwrap();
    let rotated = s
        .auth(replacement["token"].as_str().unwrap())
        .await
        .unwrap();
    assert_eq!(
        s.submit_aml_add(&rotated, input.clone())
            .await
            .unwrap()
            .job_id,
        record.job_id
    );
    assert!(worker::process_next(&s).await.unwrap());
    assert!(
        s.wait_aml_add(&rotated, "Alice", "same-id", Duration::from_secs(1))
            .await
            .unwrap()
            .success
    );
    assert_eq!(
        s.submit_aml_add(&rotated, input).await.unwrap().asset_id,
        record.asset_id
    );
    assert_eq!(count(&s, "oc_versions").await, 1);
    assert_eq!(count(&s, "oc_events").await, 1);
    let content: String = sqlx::query_scalar("SELECT content FROM oc_events WHERE id=?")
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
        sqlx::query_as("SELECT content,locator FROM oc_chunks WHERE asset_id=? ORDER BY ordinal")
            .bind(record.asset_id)
            .fetch_all(s.engine.relational().pool())
            .await
            .unwrap();
    for (content, loc) in locators {
        let loc: Value = serde_json::from_str(&loc).unwrap();
        assert_eq!(loc["parser"], aml::MESSAGE_PARSER);
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
        mismatched
            .wait_aml_add(&rotated, "Alice", "same-id", Duration::from_secs(1))
            .await,
        Err(AppError::Conflict(_))
    ));
    // Removed evidence must never turn an old successful receipt into false success.
    sqlx::query("UPDATE oc_events SET state='retracted' WHERE id=?")
        .bind(record.source_event_id)
        .execute(s.engine.relational().pool())
        .await
        .unwrap();
    assert!(matches!(
        s.wait_aml_add(&rotated, "Alice", "same-id", Duration::from_secs(1))
            .await,
        Err(AppError::Conflict(_))
    ));
    s.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn aml_model_failure_never_publishes_partial_batch_and_retry_uses_original_job() {
    let models = ModelServer::start().await;
    models.fail_from.store(2, Ordering::SeqCst);
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), models.models()).await.unwrap();
    let (_, auth) = namespace(&s).await;
    s.enable_aml_namespace(&auth).await.unwrap();
    let input = add("Alice", "retry", "s", "Atlas 多语言 evidence");
    let record = s.submit_aml_add(&auth, input.clone()).await.unwrap();
    assert!(worker::process_next(&s).await.unwrap());
    assert_eq!(count(&s, "oc_versions").await, 0);
    assert_eq!(count(&s, "oc_chunks").await, 0);
    assert_eq!(
        s.submit_aml_add(&auth, input).await.unwrap().job_id,
        record.job_id
    );
    let alice = s.lookup_aml_user(&auth, "Alice").await.unwrap().unwrap();
    let job = s.job(&alice, record.job_id).await.unwrap();
    assert!(matches!(
        job["state"].as_str(),
        Some("retry_wait" | "failed")
    ));
    assert!(!job.to_string().contains("private provider"));
    models.fail_from.store(usize::MAX, Ordering::SeqCst);
    assert_eq!(job["state"], "failed");
    assert!(matches!(
        s.wait_aml_add(&auth, "Alice", "retry", Duration::from_secs(1))
            .await,
        Err(AppError::Unavailable(_))
    ));
    s.job_action(&alice, "retry-model", record.job_id, "retry")
        .await
        .unwrap();
    assert!(worker::process_next(&s).await.unwrap());
    assert!(
        s.wait_aml_add(&auth, "Alice", "retry", Duration::from_secs(1))
            .await
            .unwrap()
            .success
    );
    assert_eq!(count(&s, "oc_versions").await, 1);
    assert_eq!(count(&s, "oc_jobs").await, 1);
    s.engine.shutdown().await.unwrap();
}
