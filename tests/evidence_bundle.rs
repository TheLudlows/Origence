#![cfg(feature = "local-storage")]

use axum::{Json, Router, extract::State, routing::post};
use origence::storage::native_turso as db;
use origence::{
    error::AppError,
    evidence::EvidenceInput,
    models::Models,
    service::Service,
    storage::{RelationalStore, Scope},
    types::AuthContext,
    worker,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;
use uuid::Uuid;

fn id(value: &Value, field: &str) -> Uuid {
    serde_json::from_value(value[field].clone()).unwrap()
}

async fn provision(s: &Service, tenant: Option<Uuid>) -> (AuthContext, String) {
    let scope = Scope {
        tenant_id: tenant.unwrap_or_else(Uuid::new_v4),
        workspace_id: Uuid::new_v4(),
    };
    s.engine
        .relational()
        .create_workspace(scope.tenant_id, scope.workspace_id, "evidence regression")
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

async fn finish(s: &Service, a: &AuthContext, receipt: &Value) {
    for _ in 0..100 {
        let job = s.job(a, id(receipt, "job_id")).await.unwrap();
        if job["state"] == "completed" {
            return;
        }
        assert_eq!(job["state"], "pending", "{job}");
        assert!(worker::process_next(s).await.unwrap());
    }
    panic!("fixture publication exceeded the serialized worker bound");
}

async fn memory(s: &Service, a: &AuthContext, key: &str, text: &str) -> Value {
    let receipt = s
        .memory(
            a,
            &Uuid::new_v4().to_string(),
            serde_json::from_value(json!({
                "fact_key":key,"content":text,"publish_if_authorized":true
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    finish(s, a, &receipt).await;
    receipt
}

async fn knowledge(s: &Service, a: &AuthContext, text: &str) -> Value {
    let receipt = s
        .knowledge(
            a,
            &Uuid::new_v4().to_string(),
            serde_json::from_value(json!({"title":"Original document","content":text})).unwrap(),
        )
        .await
        .unwrap();
    finish(s, a, &receipt).await;
    receipt
}

fn request(query: &str, routes: &[&str]) -> Value {
    json!({
        "query":query,"mode":"keyword","budget":32000,
        "routes":routes.iter().map(|route| json!({
            "route":route,"limit":100,"max_bytes":32000
        })).collect::<Vec<_>>()
    })
}

async fn bundle(s: &Service, a: &AuthContext, request: Value) -> Value {
    serde_json::to_value(
        s.evidence_bundle(a, serde_json::from_value(request).unwrap())
            .await
            .unwrap(),
    )
    .unwrap()
}

fn items(bundle: &Value) -> &[Value] {
    bundle["items"].as_array().unwrap()
}

fn has_asset(bundle: &Value, asset: Uuid) -> bool {
    items(bundle)
        .iter()
        .any(|item| item["provenance"]["asset_id"] == json!(asset))
}

fn accounting<'a>(bundle: &'a Value, route: &str) -> &'a Value {
    bundle["routes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["route"] == route)
        .unwrap()
}

/// The serialized byte budget includes the full citation and metadata, with no
/// detached citations, partial UTF-8 content, or unaccounted route bytes.
fn assert_accounted(bundle: &Value) {
    let text = bundle["rendered_context"].as_str().unwrap();
    assert_eq!(bundle["used_bytes"], json!(text.len()));
    assert!(text.len() <= bundle["budget"].as_u64().unwrap() as usize);
    let rendered_citations = text
        .lines()
        .filter(|line| line.starts_with("[chunk:") || line.starts_with("[turn:"))
        .count();
    assert_eq!(rendered_citations, items(bundle).len());
    let mut ids = BTreeSet::new();
    for item in items(bundle) {
        let evidence_id = item["evidence_id"].as_str().unwrap();
        assert!(ids.insert(evidence_id));
        assert_eq!(text.matches(&format!("[{evidence_id}] ")).count(), 1);
        assert!(text.contains(item["content"].as_str().unwrap()));
        assert!(text.contains(&item["provenance"].to_string()));
    }
    let sum: usize = bundle["routes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|route| {
            let used = route["used_bytes"].as_u64().unwrap() as usize;
            assert!(used <= route["max_bytes"].as_u64().unwrap() as usize);
            assert_eq!(
                route["retained"],
                json!(
                    items(bundle)
                        .iter()
                        .filter(|i| i["route"] == route["route"])
                        .count()
                )
            );
            used
        })
        .sum();
    assert_eq!(sum, text.len());
    for conflict in bundle["conflicts"].as_array().unwrap() {
        let members = conflict["evidence_ids"].as_array().unwrap();
        assert!(!members.is_empty());
        for evidence_id in members {
            let item = items(bundle)
                .iter()
                .find(|item| item["evidence_id"] == *evidence_id)
                .unwrap();
            assert_eq!(item["conflict_id"], conflict["conflict_id"]);
        }
        assert!(text.contains(conflict["warning"].as_str().unwrap()));
    }
    if items(bundle).is_empty() {
        assert!(text.is_empty());
        assert!(bundle["conflicts"].as_array().unwrap().is_empty());
    }
}

fn used(item: &Value) -> Value {
    let source = &item["provenance"];
    assert_eq!(source["type"], "asset_chunk");
    json!({
        "kind":"chunk","id":source["chunk_id"],"chunk_id":source["chunk_id"],
        "asset_id":source["asset_id"],"asset_version":source["asset_version"]
    })
}

async fn archive(
    s: &Service,
    a: &AuthContext,
    session: Uuid,
    text: &str,
    evidence: Vec<Value>,
) -> Value {
    s.archive_turn(
        a,
        &Uuid::new_v4().to_string(),
        session,
        serde_json::from_value(json!({
            "turn_id":Uuid::new_v4(),"question":text,"answer":text,
            "used_evidence":evidence
        }))
        .unwrap(),
    )
    .await
    .unwrap()
}

async fn counts(s: &Service) -> Vec<(String, i64)> {
    let pool = s.engine.relational().pool();
    let names: Vec<String> = db::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' AND name LIKE 'oc_%' ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    let mut result = Vec::new();
    for name in names {
        let count = db::query_scalar::<i64>(&format!("SELECT count(*) FROM \"{}\"", name.replace('"', "\"\"")))
            .fetch_one(pool)
            .await
            .unwrap();
        result.push((name, count));
    }
    result
}

struct Server(tokio::task::JoinHandle<()>);

impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn serve(router: Router) -> (String, Server) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (url, Server(server))
}

async fn embeddings(Json(body): Json<Value>) -> Json<Value> {
    // The target is deliberately below >100 cross-route distractors in native
    // vector rank, so post-top-k route filtering cannot satisfy this fixture.
    let weak = body["input"].as_str().unwrap().contains("weakcandidate");
    Json(json!({"data":[{"embedding":if weak {
        vec![0.0, 1.0, 0.0]
    } else {
        vec![1.0, 0.0, 0.0]
    }}]}))
}

async fn completions(Json(body): Json<Value>) -> Json<Value> {
    let prompt = body["messages"][0]["content"].as_str().unwrap();
    let content = if prompt.starts_with("Summarize") {
        "Fixture summary must not become original evidence.".to_string()
    } else {
        json!({"entities":[],"relations":[]}).to_string()
    };
    Json(json!({"choices":[{"message":{"content":content}}]}))
}

async fn models() -> (Models, Server) {
    let (url, server) = serve(
        Router::new()
            .route("/embeddings", post(embeddings))
            .route("/chat/completions", post(completions)),
    )
    .await;
    (
        Models::configured(
            url,
            "fixture".into(),
            Some("fixture".into()),
            Some("fixture".into()),
            3,
        )
        .unwrap(),
        server,
    )
}

#[test]
fn evidence_input_defaults_and_validation_reject_ambiguous_or_unbounded_requests() {
    let input: EvidenceInput = serde_json::from_value(json!({"query":"Atlas"})).unwrap();
    input.validate().unwrap();
    let defaults = serde_json::to_value(input).unwrap();
    assert_eq!(defaults["budget"], 4000);
    assert_eq!(defaults["mode"], "keyword");
    let routes: Vec<_> = defaults["routes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|route| route["route"].as_str().unwrap())
        .collect();
    assert_eq!(routes, ["memory", "knowledge", "learning"]);
    for invalid in [
        json!({"query":""}),
        json!({"query":"Atlas\u{0000}"}),
        json!({"query":"x".repeat(4001)}),
        json!({"query":"Atlas","budget":32001}),
        json!({"query":"Atlas","mode":"automatic"}),
        json!({"query":"Atlas","routes":[]}),
        json!({"query":"Atlas","routes":[{"route":"memory","limit":0}]}),
        json!({"query":"Atlas","routes":[{"route":"memory","limit":101}]}),
        json!({"query":"Atlas","routes":[{"route":"memory","max_bytes":32001}]}),
        json!({"query":"Atlas","routes":[{"route":"memory"},{"route":"memory"}]}),
        json!({"query":"Atlas","applicability_context":[""]}),
        json!({"query":"Atlas","session_id":Uuid::new_v4()}),
        json!({"query":"Atlas","exclude_session_id":Uuid::new_v4()}),
        json!({"query":"Atlas","routes":[{"route":"session"}],"session_id":Uuid::nil()}),
    ] {
        let parsed: EvidenceInput = serde_json::from_value(invalid.clone()).unwrap();
        assert!(parsed.validate().is_err(), "accepted {invalid}");
    }
    for invalid in [
        json!({"query":"Atlas","tenant_id":Uuid::new_v4()}),
        json!({"query":"Atlas","routes":[{"route":"memory","weight":10}]}),
        json!({"query":"Atlas","routes":[{"route":"graph"}]}),
    ] {
        assert!(serde_json::from_value::<EvidenceInput>(invalid).is_err());
    }
}

#[tokio::test]
async fn evidence_routes_filter_before_keyword_vector_and_hybrid_top_k() {
    let (models, _model) = models().await;
    for target_route in ["memory", "knowledge"] {
        let dir = tempfile::tempdir().unwrap();
        let s = Service::open(dir.path(), models.clone()).await.unwrap();
        let (a, _) = provision(&s, None).await;
        let other = if target_route == "memory" {
            "knowledge"
        } else {
            "memory"
        };
        // A hundred stronger candidates from another route must not starve the
        // route's only candidate. Distinct filler lowers lexical rank too.
        for index in 0..101 {
            if other == "memory" {
                memory(&s, &a, &format!("distractor-{index}"), "Starvationneedle").await;
            } else {
                knowledge(&s, &a, "Starvationneedle").await;
            }
        }
        let text = "Starvationneedle weakcandidate extra distinct filler words 中文 🦀";
        let target = if target_route == "memory" {
            memory(&s, &a, "only-target", text).await
        } else {
            knowledge(&s, &a, text).await
        };
        for mode in ["keyword", "vector", "hybrid"] {
            for enabled in [vec![target_route], vec!["memory", "knowledge"]] {
                let mut input = request("Starvationneedle", &enabled);
                input["mode"] = json!(mode);
                for route in input["routes"].as_array_mut().unwrap() {
                    route["limit"] = json!(1);
                }
                let result = bundle(&s, &a, input).await;
                assert_eq!(result["effective_mode"], mode);
                assert_eq!(result["warnings"], json!([]));
                assert!(has_asset(&result, id(&target, "asset_id")), "{result}");
                assert_eq!(items(&result).len(), enabled.len());
                assert_eq!(accounting(&result, target_route)["candidates"], 1);
                for item in items(&result) {
                    assert_eq!(item["provenance"]["type"], "asset_chunk");
                    assert!(
                        !item["content"]
                            .as_str()
                            .unwrap()
                            .contains("Fixture summary")
                    );
                    if mode == "hybrid" {
                        let origins = item["retrieval_routes"].as_array().unwrap();
                        assert!(origins.iter().any(|value| value == "keyword"));
                        assert!(origins.iter().any(|value| value == "vector"));
                    }
                }
                assert_accounted(&result);
            }
        }
    }
}

#[tokio::test]
async fn evidence_preserves_current_chunks_and_historical_archived_references() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s, None).await;
    let first = memory(&s, &a, "chronicle", "Chronicle original statement 中文 🦀").await;
    let before = bundle(&s, &a, request("Chronicle", &["memory"])).await;
    assert_eq!(items(&before).len(), 1);
    let old_item = &items(&before)[0];
    let session = Uuid::new_v4();
    let archived = archive(
        &s,
        &a,
        session,
        "Chronicle exact old answer 中文 🦀",
        vec![used(old_item)],
    )
    .await;
    let second = memory(&s, &a, "chronicle", "Chronicle revised statement é").await;
    assert_eq!(second["asset_id"], first["asset_id"]);
    let historical = s.get(&a, id(&first, "asset_id"), Some(1)).await.unwrap();
    assert_eq!(historical["version"], 1);
    let current = bundle(&s, &a, request("Chronicle", &["memory", "session"])).await;
    assert_eq!(items(&current).len(), 2);
    let memory_item = items(&current)
        .iter()
        .find(|item| item["route"] == "memory")
        .unwrap();
    assert_eq!(memory_item["provenance"]["asset_version"], 2);
    assert_eq!(
        memory_item["provenance"]["source_event_id"],
        second["source_event_id"]
    );
    assert!(memory_item["content"].as_str().unwrap().contains("revised"));
    let session_item = items(&current)
        .iter()
        .find(|item| item["route"] == "session")
        .unwrap();
    assert_eq!(session_item["provenance"]["turn_id"], archived["turn_id"]);
    let reference = &session_item["provenance"]["used_evidence"][0];
    assert_eq!(reference["reference"]["asset_version"], 1);
    assert_eq!(reference["source_event_id"], first["source_event_id"]);
    assert_eq!(reference["source_version"], 1);
    assert_eq!(reference["locator"], old_item["provenance"]["locator"]);
    assert!(
        session_item["content"]
            .as_str()
            .unwrap()
            .contains("exact old answer")
    );
    assert_accounted(&current);
    // With cleanup still queued, the old source invalidates the archived whole
    // turn but must not replace the current memory with its historical version.
    s.delete(
        &a,
        "withdraw-chronicle",
        id(&first, "source_event_id"),
        "event",
    )
    .await
    .unwrap();
    let after = bundle(&s, &a, request("Chronicle", &["memory", "session"])).await;
    assert_eq!(items(&after).len(), 1);
    assert_eq!(items(&after)[0]["provenance"]["asset_version"], 2);
    assert_eq!(items(&after)[0]["route"], "memory");
    assert!(
        !after["rendered_context"]
            .as_str()
            .unwrap()
            .contains("exact old answer")
    );
    assert_accounted(&after);
}

#[tokio::test]
async fn evidence_session_scope_permissions_live_keys_and_http_match_service() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, token) = provision(&s, None).await;
    let (neighbor, _) = provision(&s, Some(a.tenant_id)).await;
    let (foreign, _) = provision(&s, None).await;
    let reader_key = s.issue_key(&a, "reader").await.unwrap();
    let reader_token = reader_key["token"].as_str().unwrap();
    let reader = s.auth(reader_token).await.unwrap();
    let writer_key = s.issue_key(&a, "writer").await.unwrap();
    let writer = s.auth(writer_key["token"].as_str().unwrap()).await.unwrap();
    memory(&s, &a, "scope", "Scopeneedle public workspace memory").await;
    let session = Uuid::new_v4();
    archive(&s, &a, session, "Scopeneedle private raw session", vec![]).await;
    let default = bundle(&s, &reader, json!({"query":"Scopeneedle","budget":32000})).await;
    assert_eq!(items(&default).len(), 1);
    assert!(
        items(&default)
            .iter()
            .all(|item| item["route"] != "session")
    );
    assert!(!default.to_string().contains("private raw session"));
    let input = request("Scopeneedle", &["memory", "session"]);
    let expected = bundle(&s, &a, input.clone()).await;
    assert_eq!(expected["tenant_id"], json!(a.tenant_id));
    assert_eq!(expected["workspace_id"], json!(a.workspace_id));
    assert_eq!(items(&expected).len(), 2);
    for other in [&neighbor, &foreign] {
        let isolated = bundle(&s, other, input.clone()).await;
        assert!(items(&isolated).is_empty());
        assert!(!isolated.to_string().contains("private raw session"));
        assert_eq!(isolated["tenant_id"], json!(other.tenant_id));
        assert_eq!(isolated["workspace_id"], json!(other.workspace_id));
    }
    assert!(matches!(
        s.evidence_bundle(&reader, serde_json::from_value(input.clone()).unwrap())
            .await,
        Err(AppError::Forbidden)
    ));
    // A serialized bundle request is read-only across every relational table.
    let before = counts(&s).await;
    let (base, _api) = serve(origence::api::router(s.clone())).await;
    let http = reqwest::Client::new();
    let response = http
        .post(format!("{base}/v1/context"))
        .bearer_auth(&token)
        .json(&input)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.json::<Value>().await.unwrap(), expected);
    let response = http
        .post(format!("{base}/v1/context"))
        .bearer_auth(reader_token)
        .json(&input)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
    let denied: Value = response.json().await.unwrap();
    assert_eq!(denied["error"]["code"], "FORBIDDEN");
    assert!(!denied.to_string().contains("private raw session"));
    assert_eq!(
        http.post(format!("{base}/v1/context"))
            .json(&input)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let response = http
        .post(format!("{base}/v1/context"))
        .bearer_auth(&token)
        .json(&json!({"query":"Scopeneedle","budget":32001}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 422);
    assert_eq!(counts(&s).await, before);
    db::query("UPDATE oc_api_keys SET role='reader' WHERE id=?")
        .bind(writer.id)
        .execute(s.engine.relational().pool())
        .await
        .unwrap();
    assert!(matches!(
        s.evidence_bundle(&writer, serde_json::from_value(input).unwrap())
            .await,
        Err(AppError::Forbidden)
    ));
    s.revoke_key(&a, reader.id).await.unwrap();
    assert!(matches!(
        s.evidence_bundle(
            &reader,
            serde_json::from_value(json!({"query":"Scopeneedle"})).unwrap()
        )
        .await,
        Err(AppError::Forbidden)
    ));
    let response = http
        .post(format!("{base}/v1/context"))
        .bearer_auth(reader_token)
        .json(&json!({"query":"Scopeneedle"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn evidence_session_scan_is_bounded_and_session_filters_precede_the_scan() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s, None).await;
    let target_session = Uuid::new_v4();
    let noise_session = Uuid::new_v4();
    let oldest = archive(
        &s,
        &a,
        target_session,
        "Ancientneedle archived answer",
        vec![],
    )
    .await;
    // Fixture timestamps establish strict recency independent of machine speed.
    db::query("UPDATE oc_session_turns SET created_at=1 WHERE source_event_id=?")
        .bind(id(&oldest, "source_event_id"))
        .execute(s.engine.relational().pool())
        .await
        .unwrap();
    for _ in 0..512 {
        archive(
            &s,
            &a,
            noise_session,
            "Recent unrelated question and answer",
            vec![],
        )
        .await;
    }
    let input = request("Ancientneedle", &["session"]);
    let bounded = bundle(&s, &a, input.clone()).await;
    assert!(items(&bounded).is_empty());
    assert_eq!(accounting(&bounded, "session")["scanned"], 512);
    assert_eq!(accounting(&bounded, "session")["scan_exhausted"], true);
    for filter in ["session_id", "exclude_session_id"] {
        let mut input = input.clone();
        input[filter] = json!(if filter == "session_id" {
            target_session
        } else {
            noise_session
        });
        let filtered = bundle(&s, &a, input).await;
        assert_eq!(items(&filtered).len(), 1, "{filtered}");
        assert_eq!(
            items(&filtered)[0]["provenance"]["turn_id"],
            oldest["turn_id"]
        );
        assert_eq!(accounting(&filtered, "session")["scanned"], 1);
        assert_eq!(accounting(&filtered, "session")["scan_exhausted"], false);
        assert_accounted(&filtered);
    }
    let mut excluded = input;
    excluded["session_id"] = json!(target_session);
    excluded["exclude_session_id"] = json!(target_session);
    assert!(items(&bundle(&s, &a, excluded).await).is_empty());
}

#[tokio::test]
async fn evidence_utf8_whole_blocks_obey_global_and_route_byte_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s, None).await;
    memory(
        &s,
        &a,
        "unicode",
        "Unicodeneedle 中文 🦀 👨‍👩‍👧‍👦 é\n原文字节必须完整。",
    )
    .await;
    let input = request("Unicodeneedle", &["memory"]);
    let full = bundle(&s, &a, input.clone()).await;
    assert_eq!(items(&full).len(), 1);
    assert_accounted(&full);
    let bytes = full["rendered_context"].as_str().unwrap().len();
    assert!(bytes > full["rendered_context"].as_str().unwrap().chars().count());
    for ceiling in [0, bytes - 1, bytes, bytes + 1] {
        for route_ceiling in [false, true] {
            let mut input = input.clone();
            if route_ceiling {
                input["routes"][0]["max_bytes"] = json!(ceiling);
            } else {
                input["budget"] = json!(ceiling);
            }
            let result = bundle(&s, &a, input).await;
            assert_accounted(&result);
            if ceiling >= bytes {
                assert_eq!(result["rendered_context"], full["rendered_context"]);
                assert_eq!(result["items"], full["items"]);
                assert_eq!(accounting(&result, "memory")["omitted_for_budget"], 0);
            } else {
                assert!(items(&result).is_empty());
                assert_eq!(accounting(&result, "memory")["omitted_for_budget"], 1);
            }
        }
    }
    // A route with a zero ceiling cannot consume another route's budget.
    let document = knowledge(&s, &a, "Unicodeneedle independent original document").await;
    let mut mixed = request("Unicodeneedle", &["memory", "knowledge"]);
    mixed["routes"][0]["max_bytes"] = json!(0);
    let result = bundle(&s, &a, mixed).await;
    assert_eq!(items(&result).len(), 1);
    assert!(has_asset(&result, id(&document, "asset_id")));
    assert_eq!(accounting(&result, "memory")["used_bytes"], 0);
    assert_accounted(&result);
}

fn identity(predicate: &str, environment: &str) -> Value {
    json!({
        "subject":{"kind":"service","stable_id":"Atlas"},
        "predicate":predicate,"context":{"env":environment}
    })
}

async fn property(s: &Service, a: &AuthContext, predicate: &str, cardinality: &str) {
    s.register_memory_property(
        a,
        &Uuid::new_v4().to_string(),
        serde_json::from_value(json!({
            "predicate":predicate,"cardinality":cardinality,"required_context":["env"]
        }))
        .unwrap(),
    )
    .await
    .unwrap();
}

async fn semantic(
    s: &Service,
    a: &AuthContext,
    identity: Value,
    value: &str,
    operation: &str,
    version: i32,
    query: &str,
) -> Value {
    let quote = format!("{query} 原文 🦀 é {value}");
    let content = format!("前言\n{quote}");
    let start = content.find(&quote).unwrap();
    let receipt = s
        .semantic_memory(
            a,
            &Uuid::new_v4().to_string(),
            serde_json::from_value(json!({
                "identity":identity,"content":content,"operation":operation,"expected_version":version,
                "statements":[{"value":value,"quote":quote,"byte_start":start,"byte_end":start+quote.len()}]
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    finish(s, a, &receipt).await;
    receipt
}

#[tokio::test]
async fn evidence_explicit_single_value_conflicts_are_atomic_and_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s, None).await;
    property(&s, &a, "database.engine", "single").await;
    let identity = identity("database.engine", "production");
    let confirmed = semantic(
        &s,
        &a,
        identity.clone(),
        "PostgreSQL",
        "assert",
        0,
        "Conflictneedle",
    )
    .await;
    let conflicting = semantic(
        &s,
        &a,
        identity.clone(),
        "MySQL",
        "assert",
        1,
        "Conflictneedle",
    )
    .await;
    assert_eq!(conflicting["semantic_status"], "conflict");
    let input = request("Conflictneedle", &["memory"]);
    let full = bundle(&s, &a, input.clone()).await;
    assert_eq!(items(&full).len(), 2);
    assert!(has_asset(&full, id(&confirmed, "asset_id")));
    assert!(has_asset(&full, id(&conflicting, "asset_id")));
    assert_eq!(full["conflicts"].as_array().unwrap().len(), 1);
    let conflict = &full["conflicts"][0];
    assert_eq!(conflict["identity"], identity);
    assert_eq!(conflict["evidence_ids"].as_array().unwrap().len(), 2);
    let values: BTreeSet<_> = items(&full)
        .iter()
        .map(|item| item["semantic_value"].to_string())
        .collect();
    assert_eq!(
        values,
        BTreeSet::from([json!("PostgreSQL").to_string(), json!("MySQL").to_string()])
    );
    assert_accounted(&full);
    assert_eq!(bundle(&s, &a, input.clone()).await, full);
    // Exact-identity recall must retain the explicit opposing suggestion too;
    // filtering only the normalized asset would silently choose a winner.
    let mut scoped = input.clone();
    scoped["memory_identity"] = identity;
    let scoped = bundle(&s, &a, scoped).await;
    assert_eq!(scoped["items"], full["items"]);
    assert_eq!(scoped["conflicts"], full["conflicts"]);
    let bytes = full["used_bytes"].as_u64().unwrap() as usize;
    for ceiling in [0, bytes - 1, bytes] {
        for route_ceiling in [false, true] {
            let mut limited = input.clone();
            if route_ceiling {
                limited["routes"][0]["max_bytes"] = json!(ceiling);
            } else {
                limited["budget"] = json!(ceiling);
            }
            let result = bundle(&s, &a, limited).await;
            assert_accounted(&result);
            if ceiling < bytes {
                assert!(
                    items(&result).is_empty(),
                    "must not select one side: {result}"
                );
                assert_eq!(result["omitted_conflict_groups"], 1);
                assert_eq!(accounting(&result, "memory")["omitted_for_budget"], 2);
                assert!(!result["warnings"].as_array().unwrap().is_empty());
            } else {
                assert_eq!(result["rendered_context"], full["rendered_context"]);
            }
        }
    }
    // Withdrawal removes the unsupported alternative immediately, before its
    // queued cleanup, and must remove every dangling conflict annotation.
    s.delete(
        &a,
        "withdraw-conflict",
        id(&conflicting, "source_event_id"),
        "event",
    )
    .await
    .unwrap();
    let remaining = bundle(&s, &a, input).await;
    assert_eq!(items(&remaining).len(), 1);
    assert_eq!(items(&remaining)[0]["semantic_value"], "PostgreSQL");
    assert!(items(&remaining)[0]["conflict_id"].is_null());
    assert!(remaining["conflicts"].as_array().unwrap().is_empty());
    assert_accounted(&remaining);
}

#[tokio::test]
async fn evidence_does_not_infer_conflicts_for_sets_contexts_proposals_or_prose() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s, None).await;
    property(&s, &a, "service.languages", "multi").await;
    property(&s, &a, "database.engine", "single").await;
    let set = identity("service.languages", "production");
    semantic(&s, &a, set.clone(), "Rust", "assert", 0, "Distinctneedle").await;
    semantic(&s, &a, set, "Python", "supplement", 1, "Distinctneedle").await;
    let prod = identity("database.engine", "production");
    semantic(
        &s,
        &a,
        prod.clone(),
        "PostgreSQL",
        "assert",
        0,
        "Distinctneedle",
    )
    .await;
    semantic(
        &s,
        &a,
        identity("database.engine", "test"),
        "MySQL",
        "assert",
        0,
        "Distinctneedle",
    )
    .await;
    for operation in ["propose", "negate", "ambiguous"] {
        semantic(
            &s,
            &a,
            prod.clone(),
            "SQLite",
            operation,
            1,
            "Distinctneedle",
        )
        .await;
    }
    // Identical prose at distinct immutable locations remains two evidences.
    let first = knowledge(&s, &a, "Distinctneedle production database is Oracle.").await;
    let second = knowledge(&s, &a, "Distinctneedle production database is Oracle.").await;
    let result = bundle(&s, &a, request("Distinctneedle", &["memory", "knowledge"])).await;
    assert_eq!(items(&result).len(), 9);
    assert!(
        result["conflicts"].as_array().unwrap().is_empty(),
        "{result}"
    );
    assert!(
        items(&result)
            .iter()
            .all(|item| item["conflict_id"].is_null())
    );
    assert!(has_asset(&result, id(&first, "asset_id")));
    assert!(has_asset(&result, id(&second, "asset_id")));
    for status in ["proposed", "negated", "ambiguous"] {
        let item = items(&result)
            .iter()
            .find(|item| item["semantic_status"] == status)
            .unwrap();
        assert_eq!(item["normalization_status"], "unnormalized");
    }
    assert_accounted(&result);
    // An exact Memory identity is applied before recall, while independent
    // knowledge routes remain available; context=production never means test.
    let mut filtered = request("Distinctneedle", &["memory", "knowledge"]);
    filtered["memory_identity"] = prod.clone();
    let filtered = bundle(&s, &a, filtered).await;
    assert_eq!(items(&filtered).len(), 3);
    for item in items(&filtered)
        .iter()
        .filter(|item| item["route"] == "memory")
    {
        assert_eq!(item["identity"], prod);
        assert_eq!(item["semantic_value"], "PostgreSQL");
    }
    assert!(has_asset(&filtered, id(&first, "asset_id")));
    assert!(has_asset(&filtered, id(&second, "asset_id")));
}

#[tokio::test]
async fn evidence_whole_snapshot_withdrawal_hides_unmatched_members_and_archived_turns() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s, None).await;
    property(&s, &a, "service.languages", "multi").await;
    let identity = identity("service.languages", "production");
    let first = semantic(&s, &a, identity.clone(), "Rust", "assert", 0, "Rustneedle").await;
    let second = semantic(&s, &a, identity, "Python", "supplement", 1, "Pythonneedle").await;
    let original_member = bundle(&s, &a, request("Rustneedle", &["memory"])).await;
    assert_eq!(items(&original_member).len(), 1);
    let provenance = &items(&original_member)[0]["provenance"];
    assert_eq!(provenance["asset_version"], 2);
    assert_eq!(provenance["source_version"], 1);
    assert_eq!(provenance["source_event_id"], first["source_event_id"]);
    assert_eq!(
        provenance["version_root_source_event_id"],
        second["source_event_id"]
    );
    let before = bundle(&s, &a, request("Pythonneedle", &["memory"])).await;
    assert_eq!(items(&before).len(), 1);
    let item = &items(&before)[0];
    assert_eq!(item["provenance"]["asset_version"], 2);
    assert_ne!(
        item["provenance"]["source_event_id"],
        first["source_event_id"]
    );
    let archived = archive(
        &s,
        &a,
        Uuid::new_v4(),
        "Pythonneedle actual answer",
        vec![used(item)],
    )
    .await;
    let full = bundle(&s, &a, request("Pythonneedle", &["memory", "session"])).await;
    assert_eq!(items(&full).len(), 2);
    s.delete(
        &a,
        "withdraw-unmatched-member",
        id(&first, "source_event_id"),
        "event",
    )
    .await
    .unwrap();
    let hidden = bundle(&s, &a, request("Pythonneedle", &["memory", "session"])).await;
    assert!(items(&hidden).is_empty(), "{hidden}");
    assert_accounted(&hidden);
    assert!(matches!(
        s.session_turn(&a, id(&archived, "session_id"), id(&archived, "turn_id"))
            .await,
        Err(AppError::NotFound)
    ));
}

#[tokio::test]
async fn evidence_partial_mode_is_explicit_and_consistent_across_routes() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s, None).await;
    memory(&s, &a, "fallback", "Fallbackneedle original memory").await;
    knowledge(&s, &a, "Fallbackneedle original knowledge").await;
    for mode in ["vector", "hybrid"] {
        let mut input = request("Fallbackneedle", &["memory", "knowledge"]);
        input["mode"] = json!(mode);
        assert!(matches!(
            s.evidence_bundle(&a, serde_json::from_value(input.clone()).unwrap())
                .await,
            Err(AppError::Unavailable(_))
        ));
        input["allow_partial"] = json!(true);
        let result = bundle(&s, &a, input).await;
        assert_eq!(result["requested_mode"], mode);
        assert_eq!(result["effective_mode"], "keyword");
        assert_eq!(result["warnings"].as_array().unwrap().len(), 1);
        assert_eq!(items(&result).len(), 2);
        for item in items(&result) {
            assert_eq!(item["retrieval_routes"], json!(["keyword"]));
        }
        assert_accounted(&result);
    }
}

#[tokio::test]
async fn evidence_session_citations_use_the_full_session_and_turn_identity() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s, None).await;
    let turn = Uuid::new_v4();
    let sessions = [Uuid::new_v4(), Uuid::new_v4()];
    for session in sessions {
        s.archive_turn(
            &a,
            &Uuid::new_v4().to_string(),
            session,
            serde_json::from_value(json!({
                "turn_id":turn,"question":"Compositeidentity question",
                "answer":"Compositeidentity answer","used_evidence":[]
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    }
    let result = bundle(&s, &a, request("Compositeidentity", &["session"])).await;
    assert_eq!(items(&result).len(), 2);
    let citations: BTreeSet<_> = items(&result)
        .iter()
        .map(|item| item["evidence_id"].as_str().unwrap())
        .collect();
    assert_eq!(citations.len(), 2);
    for item in items(&result) {
        assert_eq!(item["provenance"]["turn_id"], json!(turn));
        assert_eq!(
            item["archive_roundtrip"],
            "unsupported_session_turn_reference"
        );
        assert_eq!(
            item["evidence_id"],
            json!(format!(
                "turn:{}:{turn}",
                item["provenance"]["session_id"].as_str().unwrap()
            ))
        );
        assert_eq!(item["provenance"]["used_evidence"], json!([]));
    }
    assert_accounted(&result);
}

#[derive(Clone, Default)]
struct EmbeddingGate {
    block: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

async fn gated_embedding(
    State(gate): State<EmbeddingGate>,
    Json(_body): Json<Value>,
) -> Json<Value> {
    gate.calls.fetch_add(1, Ordering::SeqCst);
    if gate.block.load(Ordering::SeqCst) {
        gate.entered.notify_one();
        gate.release.notified().await;
    }
    Json(json!({"data":[{"embedding":[1.0,0.5,0.25]}]}))
}

#[tokio::test]
async fn evidence_embeds_once_and_rechecks_visibility_and_keys_after_external_io() {
    let gate = EmbeddingGate::default();
    let (base, _model) = serve(
        Router::new()
            .route("/embeddings", post(gated_embedding))
            .route("/chat/completions", post(completions))
            .with_state(gate.clone()),
    )
    .await;
    let models = Models::configured(
        base,
        "fixture".into(),
        Some("fixture".into()),
        Some("fixture".into()),
        3,
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), models).await.unwrap();
    let (a, _) = provision(&s, None).await;
    let original = memory(
        &s,
        &a,
        "io-guard",
        "Guardneedle source withdrawn during query",
    )
    .await;
    let surviving = knowledge(&s, &a, "Guardneedle independently supported document").await;
    let reader_key = s.issue_key(&a, "reader").await.unwrap();
    let reader = s.auth(reader_key["token"].as_str().unwrap()).await.unwrap();
    let mut input = request("Guardneedle", &["memory", "knowledge", "learning"]);
    input["mode"] = json!("hybrid");
    let before_calls = gate.calls.load(Ordering::SeqCst);
    let initial = bundle(&s, &reader, input.clone()).await;
    assert_eq!(items(&initial).len(), 2);
    assert_eq!(gate.calls.load(Ordering::SeqCst) - before_calls, 1);
    assert_eq!(initial["effective_mode"], "hybrid");
    // A forbidden session request is rejected before sending even the query to
    // the provider; three other enabled routes do not downgrade this boundary.
    let mut forbidden = input.clone();
    forbidden["routes"].as_array_mut().unwrap().push(json!({
        "route":"session","limit":20,"max_bytes":4000
    }));
    let before_calls = gate.calls.load(Ordering::SeqCst);
    assert!(matches!(
        s.evidence_bundle(&reader, serde_json::from_value(forbidden).unwrap())
            .await,
        Err(AppError::Forbidden)
    ));
    assert_eq!(gate.calls.load(Ordering::SeqCst), before_calls);
    gate.block.store(true, Ordering::SeqCst);
    let pending = {
        let service = s.clone();
        let auth = reader.clone();
        let request = input.clone();
        tokio::spawn(async move {
            service
                .evidence_bundle(&auth, serde_json::from_value(request).unwrap())
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(15), gate.entered.notified())
        .await
        .expect("query did not reach the embedding barrier");
    // This runs while model IO is pending, after the request's first authorized
    // read. No cleanup worker is processed before the response is assembled.
    s.delete(
        &a,
        "withdraw-during-context-io",
        id(&original, "source_event_id"),
        "event",
    )
    .await
    .unwrap();
    gate.release.notify_one();
    let after = tokio::time::timeout(Duration::from_secs(15), pending)
        .await
        .expect("context did not resume after embedding")
        .unwrap()
        .unwrap();
    let after = serde_json::to_value(after).unwrap();
    assert_eq!(items(&after).len(), 1);
    assert!(has_asset(&after, id(&surviving, "asset_id")));
    assert!(!has_asset(&after, id(&original, "asset_id")));
    assert!(
        !after["rendered_context"]
            .as_str()
            .unwrap()
            .contains("withdrawn during query")
    );
    assert_accounted(&after);
    let pending = {
        let service = s.clone();
        let auth = reader.clone();
        tokio::spawn(async move {
            service
                .evidence_bundle(&auth, serde_json::from_value(input).unwrap())
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(15), gate.entered.notified())
        .await
        .expect("second query did not reach the embedding barrier");
    s.revoke_key(&a, reader.id).await.unwrap();
    gate.release.notify_one();
    let denied = tokio::time::timeout(Duration::from_secs(15), pending)
        .await
        .expect("revoked request did not resume after embedding")
        .unwrap();
    assert!(matches!(denied, Err(AppError::Forbidden)), "{denied:?}");
}
