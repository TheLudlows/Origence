#![cfg(feature = "local-storage")]
use super::semantic_memory::{assert_evidence_budget, assert_evidence_matrix};
use origence::{
    error::AppError,
    models::Models,
    service::Service,
    storage::{RelationalStore, Scope},
    types::AuthContext,
    worker,
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use uuid::Uuid;

struct Server(tokio::task::JoinHandle<()>);
impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn serve(router: axum::Router) -> (String, Server) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (base, Server(task))
}
struct Stub {
    output: Mutex<Value>,
    calls: AtomicUsize,
    requests: Mutex<Vec<Value>>,
    fail_embedding: AtomicBool,
    hold_embedding: AtomicBool,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
impl Stub {
    fn respond(&self, candidates: Vec<Value>) {
        *self.output.lock().unwrap() = json!({"candidates":candidates});
    }
}
async fn fixture(
    vector: bool,
) -> (
    tempfile::TempDir,
    Service,
    AuthContext,
    String,
    Arc<Stub>,
    Server,
) {
    let stub = Arc::new(Stub {
        output: Mutex::new(json!({"candidates":[]})),
        calls: AtomicUsize::new(0),
        requests: Mutex::new(Vec::new()),
        fail_embedding: AtomicBool::new(false),
        hold_embedding: AtomicBool::new(false),
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    let extract = stub.clone();
    let embed = stub.clone();
    let (base,server)=serve(axum::Router::new().route("/chat/completions",axum::routing::post(move |axum::Json(body):axum::Json<Value>|{
        let stub=extract.clone(); async move {
            stub.calls.fetch_add(1,Ordering::SeqCst); stub.requests.lock().unwrap().push(body);
            axum::Json(json!({"choices":[{"message":{"content":stub.output.lock().unwrap().to_string()}}]}))
        }
    })).route("/embeddings",axum::routing::post(move ||{
        let stub=embed.clone(); async move {
            if stub.hold_embedding.swap(false,Ordering::SeqCst){stub.entered.notify_one();stub.release.notified().await;}
            if stub.fail_embedding.load(Ordering::SeqCst){(axum::http::StatusCode::SERVICE_UNAVAILABLE,axum::Json(json!({})))}
            else {(axum::http::StatusCode::OK,axum::Json(json!({"data":[{"embedding":[1.0,0.5,0.25]}]})))}
        }
    }))).await;
    let models = Models::configured(
        base,
        "fixture".into(),
        vector.then(|| "fixture".into()),
        Some("extract-fixture".into()),
        3,
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), models).await.unwrap();
    let (a, token) = provision(&s, None).await;
    (dir, s, a, token, stub, server)
}
async fn provision(s: &Service, tenant: Option<Uuid>) -> (AuthContext, String) {
    let scope = Scope {
        tenant_id: tenant.unwrap_or_else(Uuid::new_v4),
        workspace_id: Uuid::new_v4(),
    };
    s.engine
        .relational()
        .create_workspace(scope.tenant_id, scope.workspace_id, "capture")
        .await
        .unwrap();
    let key = s
        .engine
        .relational()
        .issue_key(scope, "admin")
        .await
        .unwrap();
    (s.auth(&key.token).await.unwrap(), key.token)
}
fn identity(predicate: &str) -> Value {
    json!({"subject":{"kind":"service","stable_id":"Atlas"},"predicate":predicate,"context":{"env":"production"}})
}
fn target(predicate: &str, version: i32, allow_update: bool) -> Value {
    json!({"identity":identity(predicate),"expected_version":version,"allow_update":allow_update})
}
fn candidate(
    content: &str,
    target: Option<usize>,
    operation: &str,
    value: &str,
    quote: &str,
) -> Value {
    let start = content.find(quote).unwrap();
    json!({"target":target,"operation":operation,"statements":[{"value":value,"quote":quote,"byte_start":start,"byte_end":start+quote.len()}]})
}
fn request(content: &str, targets: Vec<Value>) -> Value {
    json!({"content":content,"targets":targets})
}
fn id(v: &Value, key: &str) -> Uuid {
    serde_json::from_value(v[key].clone()).unwrap()
}
async fn property(s: &Service, a: &AuthContext, predicate: &str, cardinality: &str) {
    s.register_memory_property(
        a,
        &Uuid::new_v4().to_string(),
        serde_json::from_value(
            json!({"predicate":predicate,"cardinality":cardinality,"required_context":["env"]}),
        )
        .unwrap(),
    )
    .await
    .unwrap();
}
async fn submit(s: &Service, a: &AuthContext, request: Value) -> Value {
    s.semantic_capture(
        a,
        &Uuid::new_v4().to_string(),
        serde_json::from_value(request).unwrap(),
    )
    .await
    .unwrap()
}
async fn terminal(s: &Service, a: &AuthContext, r: &Value) -> Value {
    for _ in 0..100 {
        let job = s.job(a, id(r, "job_id")).await.unwrap();
        if job["state"] != "pending" {
            return job;
        }
        assert!(worker::process_next(s).await.unwrap());
    }
    panic!("fixture queue did not settle");
}
async fn finish(s: &Service, a: &AuthContext, r: &Value) -> Value {
    let job = terminal(s, a, r).await;
    assert_eq!(job["state"], "completed", "{job}");
    job["result"].clone()
}
async fn lookup(s: &Service, a: &AuthContext, predicate: &str) -> Value {
    s.lookup_memory(a, serde_json::from_value(identity(predicate)).unwrap())
        .await
        .unwrap()
}
async fn http(base: &str, token: &str, key: &str, body: Value) -> (u16, Value) {
    let r = reqwest::Client::new()
        .post(format!("{base}/v1/captures/semantic"))
        .bearer_auth(token)
        .header("Idempotency-Key", key)
        .json(&body)
        .send()
        .await
        .unwrap();
    (r.status().as_u16(), r.json().await.unwrap())
}

#[tokio::test]
async fn semantic_capture_http_multilingual_search_isolation_and_updates() {
    let (_dir, s, a, token, stub, _model) = fixture(true).await;
    property(&s, &a, "database.engine", "single").await;
    property(&s, &a, "service.languages", "multi").await;
    let text = "用户：Atlas 生产使用 PostgreSQL；支持 Rust 🦀 é。助手：可能改用 MySQL。";
    stub.respond(vec![
        candidate(text, Some(0), "assert", "PostgreSQL", "生产使用 PostgreSQL"),
        candidate(text, Some(1), "supplement", "Rust", "支持 Rust 🦀 é"),
        candidate(text, None, "propose", "MySQL", "可能改用 MySQL"),
    ]);
    let req = request(
        text,
        vec![
            target("database.engine", 0, false),
            target("service.languages", 0, false),
        ],
    );
    let (base, _api) = serve(origence::api::router(s.clone())).await;
    let reader = s
        .engine
        .relational()
        .issue_key(
            Scope {
                tenant_id: a.tenant_id,
                workspace_id: a.workspace_id,
            },
            "reader",
        )
        .await
        .unwrap();
    assert_eq!(http(&base, &reader.token, "no", req.clone()).await.0, 403);
    let (status, r) = http(&base, &token, "capture", req.clone()).await;
    assert_eq!(status, 200);
    assert_eq!(http(&base, &token, "capture", req.clone()).await.1, r);
    let mut different = req.clone();
    different["content"] = json!("different");
    assert_eq!(http(&base, &token, "capture", different).await.0, 409);
    let result = finish(&s, &a, &r).await;
    assert_eq!(result["memories"].as_array().unwrap().len(), 3);
    assert_eq!(result["memories"][2]["semantic_status"], "proposed");
    assert_eq!(stub.calls.load(Ordering::SeqCst), 1);
    let model_request = stub.requests.lock().unwrap()[0].clone();
    assert_eq!(
        model_request["messages"][0]["content"],
        origence::models::SEMANTIC_CAPTURE_PROMPT
    );
    let envelope: Value =
        serde_json::from_str(model_request["messages"][1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(envelope["source"], text);
    assert_eq!(envelope["targets"], req["targets"]);
    let event = s.source_event(&a, id(&r, "source_event_id")).await.unwrap();
    assert_eq!(event["content"], text);
    // Independent caller in a second session uses the persisted workspace memory.
    let second = s
        .auth(
            &s.engine
                .relational()
                .issue_key(
                    Scope {
                        tenant_id: a.tenant_id,
                        workspace_id: a.workspace_id,
                    },
                    "reader",
                )
                .await
                .unwrap()
                .token,
        )
        .await
        .unwrap();
    for mode in ["keyword", "vector", "hybrid"] {
        let found=s.search(&second,serde_json::from_value(json!({"query":"PostgreSQL","mode":mode,"memory_identity":identity("database.engine")})).unwrap()).await.unwrap();
        let hits = found["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 1, "{found}");
        let hit = &hits[0];
        let l = &hit["locator"];
        assert_eq!(hit["source_event_id"], r["source_event_id"]);
        assert_eq!(l["extraction"]["parser"], "semantic-capture:v1");
        assert_eq!(l["extraction"]["model"], "extract-fixture");
        assert_eq!(
            &text[l["byte_start"].as_u64().unwrap() as usize
                ..l["byte_end"].as_u64().unwrap() as usize],
            hit["content"].as_str().unwrap()
        );
    }
    let resolved = s
        .resolve(
            &second,
            serde_json::from_value(json!({"query":"MySQL","budget_tokens":2000})).unwrap(),
        )
        .await
        .unwrap();
    assert!(resolved.to_string().contains("proposed"));
    let original = lookup(&s, &a, "database.engine").await;
    assert_evidence_matrix(&s, &a, &original, "PostgreSQL", true).await;
    assert_evidence_budget(&s, &a, &identity("database.engine"), "PostgreSQL").await;
    for tenant in [None, Some(a.tenant_id)] {
        let (other, other_token) = provision(&s, tenant).await;
        assert!(matches!(
            s.job(&other, id(&r, "job_id")).await,
            Err(AppError::NotFound)
        ));
        assert_eq!(
            http(&base, &other_token, "foreign", req.clone()).await.0,
            422
        );
        let found = s
            .search(
                &other,
                serde_json::from_value(json!({"query":"PostgreSQL","mode":"hybrid"})).unwrap(),
            )
            .await
            .unwrap();
        assert!(found["hits"].as_array().unwrap().is_empty());
        assert_evidence_matrix(&s, &other, &original, "PostgreSQL", false).await;
    }
    let text = "Atlas 生产从 PostgreSQL 改用 MySQL。";
    stub.respond(vec![candidate(text, Some(0), "update", "MySQL", text)]);
    let unapproved = submit(
        &s,
        &a,
        request(text, vec![target("database.engine", 1, false)]),
    )
    .await;
    let unapproved_result = finish(&s, &a, &unapproved).await;
    assert_eq!(unapproved_result["memories"][0]["semantic_status"], "ambiguous");
    let ambiguous = s
        .get(&a, id(&unapproved_result["memories"][0], "asset_id"), None)
        .await
        .unwrap();
    assert!(ambiguous["identity"].is_null());
    assert_eq!(ambiguous["normalization_status"], "unnormalized");
    assert_evidence_matrix(&s, &a, &ambiguous, "MySQL", true).await;
    assert_eq!(lookup(&s, &a, "database.engine").await, original);
    stub.respond(vec![candidate(text, Some(0), "assert", "MySQL", text)]);
    let conflict = submit(
        &s,
        &a,
        request(text, vec![target("database.engine", 1, false)]),
    )
    .await;
    let conflict_result = finish(&s, &a, &conflict).await;
    assert_eq!(conflict_result["memories"][0]["semantic_status"], "conflict");
    let conflicting = s
        .get(&a, id(&conflict_result["memories"][0], "asset_id"), None)
        .await
        .unwrap();
    assert!(conflicting["identity"].is_null());
    assert_eq!(conflicting["normalization_status"], "unnormalized");
    assert_evidence_matrix(&s, &a, &conflicting, "MySQL", true).await;
    assert_eq!(lookup(&s, &a, "database.engine").await, original);
    stub.respond(vec![candidate(text, Some(0), "update", "MySQL", text)]);
    let update = submit(
        &s,
        &a,
        request(text, vec![target("database.engine", 1, true)]),
    )
    .await;
    finish(&s, &a, &update).await;
    let updated = lookup(&s, &a, "database.engine").await;
    assert_eq!(updated["version"], 2);
    stub.respond(vec![candidate(text, Some(0), "assert", "MySQL", text)]);
    let duplicate = submit(
        &s,
        &a,
        request(text, vec![target("database.engine", 2, false)]),
    )
    .await;
    assert_eq!(
        finish(&s, &a, &duplicate).await["memories"][0]["state"],
        "duplicate"
    );
    assert_ne!(duplicate["source_event_id"], update["source_event_id"]);
    assert_eq!(lookup(&s, &a, "database.engine").await, updated);
    s.delete(
        &a,
        "withdraw-duplicate",
        id(&duplicate, "source_event_id"),
        "event",
    )
    .await
    .unwrap();
    assert_evidence_matrix(&s, &a, &updated, "MySQL", true).await;
    // Suggested identity is not support: withdrawing uncertain evidence cannot
    // withdraw the still-live confirmed fact that it refers to.
    let uncertainty_withdrawal = s
        .delete(
            &a,
            "withdraw-ambiguous",
            id(&unapproved, "source_event_id"),
            "event",
        )
        .await
        .unwrap();
    assert_eq!(
        s.job(&a, id(&uncertainty_withdrawal, "cleanup_job_id"))
            .await
            .unwrap()["state"],
        "pending"
    );
    assert_evidence_matrix(&s, &a, &ambiguous, "MySQL", false).await;
    assert_evidence_matrix(&s, &a, &updated, "MySQL", true).await;
    // Withdrawing publication support removes the fact from all current retrieval paths.
    let withdrawal = s
        .delete(
            &a,
            "withdraw-update",
            id(&update, "source_event_id"),
            "event",
        )
        .await
        .unwrap();
    assert_eq!(
        s.job(&a, id(&withdrawal, "cleanup_job_id")).await.unwrap()["state"],
        "pending"
    );
    assert_evidence_matrix(&s, &a, &updated, "MySQL", false).await;
    assert_evidence_matrix(&s, &a, &conflicting, "MySQL", true).await;
    // Only the withdrawn version loses historical reads; no fallback becomes current.
    assert_eq!(
        s.get(&a, id(&original, "asset_id"), Some(1)).await.unwrap(),
        original
    );
    assert!(matches!(
        s.lookup_memory(
            &a,
            serde_json::from_value(identity("database.engine")).unwrap()
        )
        .await,
        Err(AppError::NotFound)
    ));
    let found = s
        .search(
            &a,
            serde_json::from_value(
                json!({"query":"MySQL","mode":"hybrid","memory_identity":identity("database.engine")}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(found["hits"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn semantic_capture_set_sources_survive_union_and_withdrawal() {
    let (_dir, s, a, _, stub, _model) = fixture(true).await;
    property(&s, &a, "service.languages", "multi").await;
    stub.respond(vec![candidate(
        "Rust",
        Some(0),
        "supplement",
        "Rust",
        "Rust",
    )]);
    let first = submit(
        &s,
        &a,
        request("Rust", vec![target("service.languages", 0, false)]),
    )
    .await;
    finish(&s, &a, &first).await;
    stub.respond(vec![candidate(
        "Python",
        Some(0),
        "supplement",
        "Python",
        "Python",
    )]);
    let next = submit(
        &s,
        &a,
        request("Python", vec![target("service.languages", 1, false)]),
    )
    .await;
    finish(&s, &a, &next).await;
    let fact = lookup(&s, &a, "service.languages").await;
    assert_eq!(fact["semantic_values"].as_array().unwrap().len(), 2);
    assert_eq!(
        fact["semantic_values"][0]["source_event_id"],
        first["source_event_id"]
    );
    assert_eq!(
        fact["semantic_values"][0]["locator"]["extraction"]["parser"],
        "semantic-capture:v1"
    );
    assert_evidence_matrix(&s, &a, &fact, "Python", true).await;
    let withdrawal = s
        .delete(&a, "withdraw-old", id(&first, "source_event_id"), "event")
        .await
        .unwrap();
    assert_eq!(
        s.job(&a, id(&withdrawal, "cleanup_job_id")).await.unwrap()["state"],
        "pending"
    );
    assert_evidence_matrix(&s, &a, &fact, "Python", false).await;
    for mode in ["keyword", "vector", "hybrid"] {
        let found = s
            .search(
                &a,
                serde_json::from_value(json!({"query":"Python","mode":mode})).unwrap(),
            )
            .await
            .unwrap();
        assert!(found["hits"].as_array().unwrap().is_empty());
    }
    let rebuild = submit(
        &s,
        &a,
        request("Python", vec![target("service.languages", 2, false)]),
    )
    .await;
    finish(&s, &a, &rebuild).await;
    let fact = lookup(&s, &a, "service.languages").await;
    assert_eq!(fact["version"], 3);
    assert_eq!(fact["semantic_values"].as_array().unwrap().len(), 1);
    assert_eq!(
        fact["semantic_values"][0]["source_event_id"],
        next["source_event_id"]
    );
    assert_eq!(
        fact["semantic_values"][0]["locator"]["extraction"]["model"],
        "extract-fixture"
    );
    assert_evidence_matrix(&s, &a, &fact, "Python", true).await;
}

#[tokio::test]
async fn semantic_capture_retry_reuses_plan_across_service_restart() {
    use origence::storage::JobQueue;
    let (dir, s, a, token, stub, _model) = fixture(true).await;
    property(&s, &a, "database.engine", "single").await;
    stub.respond(vec![candidate(
        "PostgreSQL",
        Some(0),
        "assert",
        "PostgreSQL",
        "PostgreSQL",
    )]);
    stub.fail_embedding.store(true, Ordering::SeqCst);
    let req = request("PostgreSQL", vec![target("database.engine", 0, false)]);
    let r = submit(&s, &a, req.clone()).await;
    assert_eq!(terminal(&s, &a, &r).await["state"], "failed");
    assert!(matches!(
        s.lookup_memory(
            &a,
            serde_json::from_value(identity("database.engine")).unwrap()
        )
        .await,
        Err(AppError::NotFound)
    ));
    assert_eq!(stub.calls.load(Ordering::SeqCst), 1);
    // A changed model answer must not replace the saved classification on retry.
    stub.respond(Vec::new());
    stub.fail_embedding.store(false, Ordering::SeqCst);
    s.job_action(&a, "retry", id(&r, "job_id"), "retry")
        .await
        .unwrap();
    let claim = s.engine.queue().claim_next().await.unwrap().unwrap();
    assert_eq!(claim.kind, "semantic_extract");
    let models = s.models.clone();
    s.engine.relational().pool().close().await;
    drop(s);
    let s = Service::open(dir.path(), models).await.unwrap();
    let a = s.auth(&token).await.unwrap();
    finish(&s, &a, &r).await;
    assert_eq!(stub.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        lookup(&s, &a, "database.engine").await["semantic_values"][0]["value"],
        "PostgreSQL"
    );
    assert_eq!(lookup(&s, &a, "database.engine").await["version"], 1);
}

#[tokio::test]
async fn semantic_capture_rejects_bad_batches_and_handles_empty_or_unknown() {
    let (_dir, s, a, _, stub, _model) = fixture(false).await;
    property(&s, &a, "database.engine", "single").await;
    let good = candidate("Rust 🦀", Some(0), "assert", "Rust", "Rust 🦀");
    let req = request("Rust 🦀", vec![target("database.engine", 0, false)]);
    let mut bad = good.clone();
    bad["target"] = json!(999);
    let mut span = good.clone();
    span["statements"][0]["byte_end"] = json!(6);
    let mut forged = good.clone();
    forged["tenant_id"] = json!(Uuid::new_v4());
    for output in [
        json!({"candidates":[good.clone(),bad]}),
        json!({"candidates":[span]}),
        json!({"candidates":[forged]}),
        json!({"candidates":[good.clone(),good.clone()]}),
        json!({"candidates":[good.clone()],"publish":true}),
    ] {
        *stub.output.lock().unwrap() = output;
        let r = submit(&s, &a, req.clone()).await;
        let job = terminal(&s, &a, &r).await;
        assert_eq!(job["state"], "failed");
        assert!(!job.to_string().contains("Rust"));
        let found = s
            .search(&a, serde_json::from_value(json!({"query":"Rust"})).unwrap())
            .await
            .unwrap();
        assert!(found["hits"].as_array().unwrap().is_empty());
    }
    stub.respond(vec![]);
    let empty = submit(&s, &a, req).await;
    assert!(
        finish(&s, &a, &empty).await["memories"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    for op in ["assert", "negate", "propose", "ambiguous"] {
        stub.respond(vec![candidate("Rust 🦀", None, op, "Rust", "Rust 🦀")]);
        let r = submit(&s, &a, request("Rust 🦀", vec![])).await;
        assert_eq!(
            finish(&s, &a, &r).await["memories"][0]["normalization_status"],
            "unnormalized"
        );
    }
}

#[tokio::test]
async fn semantic_capture_changed_unused_or_uncertain_target_does_not_block_publication() {
    for operation in ["unused", "propose", "negate", "ambiguous", "update"] {
        let (_dir, s, a, _, stub, _model) = fixture(true).await;
        property(&s, &a, "database.engine", "single").await;
        property(&s, &a, "service.languages", "multi").await;
        stub.respond(vec![candidate("Rust", Some(0), "assert", "Rust", "Rust")]);
        let first = submit(
            &s,
            &a,
            request("Rust", vec![target("database.engine", 0, false)]),
        )
        .await;
        // Hold the first publication outside SQL, so the later request passes
        // admission at version 0 without relying on equal-timestamp queue order.
        stub.hold_embedding.store(true, Ordering::SeqCst);
        let worker_service = s.clone();
        let running =
            tokio::spawn(async move { worker::process_next(&worker_service).await.unwrap() });
        tokio::time::timeout(std::time::Duration::from_secs(10), stub.entered.notified())
            .await
            .unwrap();
        let later = submit(
            &s,
            &a,
            request(
                "Go",
                vec![
                    target("database.engine", 0, false),
                    target("service.languages", 0, false),
                ],
            ),
        )
        .await;
        stub.release.notify_one();
        running.await.unwrap();
        finish(&s, &a, &first).await;
        let (index, requested) = if operation == "unused" {
            (1, "supplement")
        } else {
            (0, operation)
        };
        stub.respond(vec![candidate("Go", Some(index), requested, "Go", "Go")]);
        let result = finish(&s, &a, &later).await;
        let memory = &result["memories"][0];
        assert_eq!(memory["state"], "published", "{operation}");
        assert_eq!(memory["requested_operation"], requested);
        assert_eq!(lookup(&s, &a, "database.engine").await["version"], 1);
        if operation == "unused" {
            assert_eq!(memory["normalization_status"], "explicit_identity");
            assert_eq!(lookup(&s, &a, "service.languages").await["version"], 1);
        } else {
            // An unauthorized update is downgraded to ambiguous evidence;
            // all uncertain routes preserve provenance without binding identity.
            assert_eq!(memory["normalization_status"], "unnormalized");
            assert!(memory["identity"].is_null());
            let expected_status = match operation {
                "propose" => "proposed",
                "negate" => "negated",
                _ => "ambiguous",
            };
            assert_eq!(memory["semantic_status"], expected_status);
        }
    }
}

#[tokio::test]
async fn semantic_capture_races_cancel_and_live_revocation_cannot_publish() {
    let (_dir, s, a, _, stub, _model) = fixture(true).await;
    property(&s, &a, "database.engine", "single").await;
    stub.respond(vec![candidate("Rust", Some(0), "assert", "Rust", "Rust")]);
    let req = request("Rust", vec![target("database.engine", 0, false)]);
    let first = submit(&s, &a, req.clone()).await;
    let stale = submit(&s, &a, req.clone()).await;
    finish(&s, &a, &first).await;
    assert_eq!(terminal(&s, &a, &stale).await["state"], "superseded");
    // Pause outside any SQL transaction at embedding; revoke source before commit.
    stub.respond(vec![candidate("Go", Some(0), "update", "Go", "Go")]);
    stub.hold_embedding.store(true, Ordering::SeqCst);
    let pending = submit(
        &s,
        &a,
        request("Go", vec![target("database.engine", 1, true)]),
    )
    .await;
    let worker_service = s.clone();
    let running = tokio::spawn(async move { worker::process_next(&worker_service).await.unwrap() });
    tokio::time::timeout(std::time::Duration::from_secs(10), stub.entered.notified())
        .await
        .unwrap();
    s.delete(
        &a,
        "withdraw-pending",
        id(&pending, "source_event_id"),
        "event",
    )
    .await
    .unwrap();
    stub.release.notify_one();
    running.await.unwrap();
    assert_eq!(
        s.job(&a, id(&pending, "job_id")).await.unwrap()["state"],
        "cancelled"
    );
    assert_eq!(lookup(&s, &a, "database.engine").await["version"], 1);
    let cancelled = submit(
        &s,
        &a,
        request("Go", vec![target("database.engine", 1, true)]),
    )
    .await;
    s.job_action(&a, "cancel", id(&cancelled, "job_id"), "cancel")
        .await
        .unwrap();
    s.job_action(&a, "retry-cancel", id(&cancelled, "job_id"), "retry")
        .await
        .unwrap();
    finish(&s, &a, &cancelled).await;
    assert_eq!(lookup(&s, &a, "database.engine").await["version"], 2);
    let writer = s
        .engine
        .relational()
        .issue_key(
            Scope {
                tenant_id: a.tenant_id,
                workspace_id: a.workspace_id,
            },
            "writer",
        )
        .await
        .unwrap();
    let wa = s.auth(&writer.token).await.unwrap();
    let revoked = submit(
        &s,
        &wa,
        request("Go", vec![target("database.engine", 2, false)]),
    )
    .await;
    stub.hold_embedding.store(true, Ordering::SeqCst);
    let worker_service = s.clone();
    let running = tokio::spawn(async move { worker::process_next(&worker_service).await.unwrap() });
    tokio::time::timeout(std::time::Duration::from_secs(10), stub.entered.notified())
        .await
        .unwrap();
    s.revoke_key(&a, wa.id).await.unwrap();
    stub.release.notify_one();
    running.await.unwrap();
    assert_eq!(
        s.job(&a, id(&revoked, "job_id")).await.unwrap()["state"],
        "failed"
    );
    assert_eq!(lookup(&s, &a, "database.engine").await["version"], 2);
}

#[tokio::test]
async fn semantic_capture_target_validation_and_atomic_route_failure() {
    let (_dir, s, a, _, stub, _model) = fixture(false).await;
    let req = request("Rust", vec![target("database.engine", 0, false)]);
    assert!(
        s.semantic_capture(
            &a,
            "unregistered",
            serde_json::from_value(req.clone()).unwrap()
        )
        .await
        .is_err()
    );
    property(&s, &a, "database.engine", "single").await;
    property(&s, &a, "service.languages", "multi").await;
    let mut no_context = req.clone();
    no_context["targets"][0]["identity"]["context"] = json!({});
    assert!(
        s.semantic_capture(
            &a,
            "missing-context",
            serde_json::from_value(no_context).unwrap()
        )
        .await
        .is_err()
    );
    let mut disabled = s.clone();
    disabled.models = Models::disabled();
    assert!(matches!(
        disabled
            .semantic_capture(&a, "disabled", serde_json::from_value(req.clone()).unwrap())
            .await,
        Err(AppError::Unavailable(_))
    ));
    assert_eq!(stub.calls.load(Ordering::SeqCst), 0);
    // A valid first candidate cannot escape an incompatible second route.
    stub.respond(vec![
        candidate("Rust", Some(0), "supplement", "Rust", "Rust"),
        candidate("Rust", Some(1), "supplement", "Rust", "Rust"),
    ]);
    let r = submit(
        &s,
        &a,
        request(
            "Rust",
            vec![
                target("service.languages", 0, false),
                target("database.engine", 0, false),
            ],
        ),
    )
    .await;
    assert_eq!(terminal(&s, &a, &r).await["state"], "failed");
    assert!(matches!(
        s.lookup_memory(
            &a,
            serde_json::from_value(identity("service.languages")).unwrap()
        )
        .await,
        Err(AppError::NotFound)
    ));
    // Distinct subjects/environments remain separate allowed targets.
    let mut second = target("database.engine", 0, false);
    second["identity"]["context"]["env"] = json!("staging");
    let mut third = target("database.engine", 0, false);
    third["identity"]["subject"]["stable_id"] = json!("Other Atlas");
    stub.respond(vec![
        candidate("Rust Go Java", Some(0), "assert", "Rust", "Rust"),
        candidate("Rust Go Java", Some(1), "assert", "Go", "Go"),
        candidate("Rust Go Java", Some(2), "assert", "Java", "Java"),
    ]);
    let r = submit(
        &s,
        &a,
        request(
            "Rust Go Java",
            vec![
                target("database.engine", 0, false),
                second.clone(),
                third.clone(),
            ],
        ),
    )
    .await;
    let result = finish(&s, &a, &r).await;
    let assets: std::collections::BTreeSet<_> = result["memories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| id(m, "asset_id"))
        .collect();
    assert_eq!(assets.len(), 3);
    assert_eq!(
        lookup(&s, &a, "database.engine").await["semantic_values"][0]["value"],
        "Rust"
    );
    for (target, value) in [(second, "Go"), (third, "Java")] {
        assert_eq!(
            s.lookup_memory(
                &a,
                serde_json::from_value(target["identity"].clone()).unwrap()
            )
            .await
            .unwrap()["semantic_values"][0]["value"],
            value
        );
    }
}

#[tokio::test]
async fn semantic_capture_rechecks_retained_member_sources_and_model_profile() {
    let (_dir, s, a, _, stub, _model) = fixture(true).await;
    property(&s, &a, "service.languages", "multi").await;
    stub.respond(vec![candidate(
        "Rust",
        Some(0),
        "supplement",
        "Rust",
        "Rust",
    )]);
    let first = submit(
        &s,
        &a,
        request("Rust", vec![target("service.languages", 0, false)]),
    )
    .await;
    finish(&s, &a, &first).await;
    stub.respond(vec![candidate(
        "Python",
        Some(0),
        "supplement",
        "Python",
        "Python",
    )]);
    let pending = submit(
        &s,
        &a,
        request("Python", vec![target("service.languages", 1, false)]),
    )
    .await;
    stub.hold_embedding.store(true, Ordering::SeqCst);
    let worker_service = s.clone();
    let running = tokio::spawn(async move { worker::process_next(&worker_service).await.unwrap() });
    tokio::time::timeout(std::time::Duration::from_secs(10), stub.entered.notified())
        .await
        .unwrap();
    // This is a previous member source, not pending.source_event_id: deleting it
    // does not cancel the new claim. The saved plan guard must detect its loss.
    s.delete(
        &a,
        "withdraw-retained",
        id(&first, "source_event_id"),
        "event",
    )
    .await
    .unwrap();
    stub.release.notify_one();
    running.await.unwrap();
    assert_eq!(
        s.job(&a, id(&pending, "job_id")).await.unwrap()["state"],
        "superseded"
    );
    let found = s
        .search(
            &a,
            serde_json::from_value(json!({"query":"Python","mode":"hybrid"})).unwrap(),
        )
        .await
        .unwrap();
    assert!(found["hits"].as_array().unwrap().is_empty());
    while worker::process_next(&s).await.unwrap() {}
    let next = submit(
        &s,
        &a,
        request("Python", vec![target("service.languages", 1, false)]),
    )
    .await;
    let count = stub.calls.load(Ordering::SeqCst);
    let mut different = s.clone();
    different.models = Models::configured(
        "http://127.0.0.1:1".into(),
        "".into(),
        None,
        Some("changed".into()),
        3,
    )
    .unwrap();
    // No configured embedding must also reject a job accepted with embeddings.
    assert!(worker::process_next(&different).await.unwrap());
    assert_eq!(
        s.job(&a, id(&next, "job_id")).await.unwrap()["state"],
        "failed"
    );
    assert_eq!(stub.calls.load(Ordering::SeqCst), count);

    // Check the extraction-only fingerprint independently of the embedding guard.
    let mut no_vectors = s.clone();
    no_vectors.models = Models::configured(
        "http://127.0.0.1:1".into(),
        "".into(),
        None,
        Some("original".into()),
        3,
    )
    .unwrap();
    let next = submit(
        &no_vectors,
        &a,
        request("Python", vec![target("service.languages", 1, false)]),
    )
    .await;
    assert!(worker::process_next(&different).await.unwrap());
    assert_eq!(
        s.job(&a, id(&next, "job_id")).await.unwrap()["state"],
        "failed"
    );
    assert_eq!(stub.calls.load(Ordering::SeqCst), count);
}
