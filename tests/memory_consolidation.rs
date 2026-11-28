use crate::fixture_keys::FixtureKeys;
mod support;

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use axum::{Json, Router, http::StatusCode, routing::post};
use origence::{
    consolidation::RecallInput,
    error::AppError,
    memory_identity::{MemoryIdentity, MemorySubject, SubjectKind},
    models::Models,
    service::Service,
    storage::{JobQueue, Scope, sqlite_db},
    types::{AuthContext, CaptureInput},
    worker,
};
use serde_json::{Value, json};
use uuid::Uuid;

const PREDICATE: &str = "travel.hotel_cap";
const END_OF_NOVEMBER: i64 = 1_795_996_800_000;
const DECEMBER_START: i64 = 1_796_083_200_000;

const ORIGINAL: &str = "通知：云舟境内酒店限额自2026-01-01起为每晚100 元。";
const UPCOMING: &str = "通知：云舟境内酒店限额自2026-12-01起为每晚80 元。";

struct Server(tokio::task::JoinHandle<()>);

impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn serve(router: Router) -> (String, Server) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (base, Server(task))
}

#[derive(Default)]
struct Stub {
    output: Mutex<Value>,
    calls: AtomicUsize,
    unavailable: AtomicBool,
}

impl Stub {
    fn respond(&self, facts: Vec<Value>) {
        *self.output.lock().unwrap() = json!({"facts":facts});
    }
}

struct Fixture {
    service: Service,
    auth: AuthContext,
    token: String,
    stub: Arc<Stub>,
    _server: Server,
    // Drop SQLite handles before its temp directory, including on Windows.
    dir: tempfile::TempDir,
}

impl Fixture {
    async fn recall(
        &self,
        region: &str,
        version: Option<i32>,
        time_ms: i64,
    ) -> Result<Value, AppError> {
        recall(&self.service, &self.auth, region, version, time_ms).await
    }
}

async fn setup() -> Fixture {
    let stub = Arc::new(Stub::default());
    let endpoint = stub.clone();
    let (base, server) = serve(Router::new().route(
        "/chat/completions",
        post(move || {
            let endpoint = endpoint.clone();
            async move {
                endpoint.calls.fetch_add(1, Ordering::SeqCst);
                if endpoint.unavailable.load(Ordering::SeqCst) {
                    return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({})));
                }
                let content = endpoint.output.lock().unwrap().to_string();
                (
                    StatusCode::OK,
                    Json(json!({"choices":[{"message":{"content":content}}]})),
                )
            }
        }),
    ))
    .await;
    let models =
        Models::configured(base, "fixture".into(), None, Some("fixture".into()), 3).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let service = support::open_service(dir.path(), models).await.unwrap();
    let (auth, token) = provision(&service, Uuid::new_v4()).await;
    service
        .register_memory_property(
            &auth,
            "hotel-cap-property",
            serde_json::from_value(json!({
                "predicate":PREDICATE,
                "cardinality":"single",
                "required_context":["region"]
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    Fixture {
        dir,
        service,
        auth,
        token,
        stub,
        _server: server,
    }
}

async fn provision(service: &Service, tenant_id: Uuid) -> (AuthContext, String) {
    let workspace_id = Uuid::new_v4();
    let store = service.engine.relational();
    store
        .create_workspace(tenant_id, workspace_id, "memory policy fixture")
        .await
        .unwrap();
    let key = store
        .seed_workspace_key(Scope {
            tenant_id,
            workspace_id,
        })
        .await
        .unwrap();
    (service.auth(&key.token).await.unwrap(), key.token)
}

fn identity(region: &str) -> MemoryIdentity {
    MemoryIdentity {
        subject: MemorySubject {
            kind: SubjectKind::Project,
            stable_id: "云舟".into(),
        },
        predicate: PREDICATE.into(),
        context: [("region".into(), region.into())].into(),
    }
}

fn fact(content: &str, region: &str, value: &str, effective_date: Option<&str>) -> Value {
    let start = content.find('云').unwrap();
    let mut output = json!({
        "identity":identity(region),
        "statement":{
            "value":value,
            "quote":&content[start..],
            "byte_start":start,
            "byte_end":content.len()
        }
    });
    if let Some(date) = effective_date {
        output["effective_date"] = json!(date);
    }
    output
}

fn id(data: &Value, name: &str) -> Uuid {
    serde_json::from_value(data[name].clone()).unwrap()
}

async fn retain(fixture: &Fixture, key: &str, content: &str) -> Value {
    fixture
        .service
        .retain_memory(
            &fixture.auth,
            key,
            CaptureInput {
                content: content.into(),
            },
        )
        .await
        .unwrap()
}

async fn settle(fixture: &Fixture, receipt: &Value) -> Value {
    for _ in 0..12 {
        let job = fixture
            .service
            .job(&fixture.auth, id(receipt, "job_id"))
            .await
            .unwrap();
        if job["state"] != "pending" {
            return job;
        }
        assert!(worker::process_next(&fixture.service).await.unwrap());
    }
    panic!("memory consolidation did not settle")
}

async fn publish(
    fixture: &Fixture,
    key: &str,
    content: &str,
    region: &str,
    value: &str,
    date: Option<&str>,
) -> Value {
    fixture
        .stub
        .respond(vec![fact(content, region, value, date)]);
    let receipt = retain(fixture, key, content).await;
    let job = settle(fixture, &receipt).await;
    assert_eq!(job["state"], "completed", "{job}");
    receipt
}

async fn recall(
    service: &Service,
    auth: &AuthContext,
    region: &str,
    version: Option<i32>,
    time_ms: i64,
) -> Result<Value, AppError> {
    let view = service
        .recall_long_term_memory(
            auth,
            RecallInput {
                identity: identity(region),
                version,
                as_of_ms: Some(time_ms),
            },
        )
        .await?;
    Ok(serde_json::to_value(view).unwrap())
}

fn assert_current(view: &Value, value: &str) {
    assert_eq!(view["state"], json!({"status":"current","value":value}));
}

const POLICY_QUESTION: &str = "云舟境内酒店报销上限是多少？";

async fn publish_explicit_policy(fixture: &Fixture, identity: MemoryIdentity, amount: &str) {
    let content = format!("{POLICY_QUESTION} 每晚{amount}。");
    let receipt = fixture.service.semantic_memory(
        &fixture.auth,
        &Uuid::new_v4().to_string(),
        serde_json::from_value(json!({
            "identity":identity, "content":content, "operation":"assert", "expected_version":0,
            "statements":[{"value":amount,"quote":content,"byte_start":0,"byte_end":content.len()}]
        })).unwrap(),
    ).await.unwrap();
    let job = settle(fixture, &receipt).await;
    assert_eq!(job["state"], "completed", "{job}");
}

#[tokio::test]
async fn ordinary_context_recall_keeps_dated_conclusions_and_citations_together() {
    let fixture = setup().await;
    let (base, _api) = serve(origence::api::router(fixture.service.clone())).await;
    let (_, other_key) = provision(&fixture.service, fixture.auth.tenant_id).await;
    let client = reqwest::Client::new();
    let args = |as_of_ms, budget| {
        json!({
            "query":POLICY_QUESTION,
            "as_of_ms": as_of_ms,
            "budget": budget,
        })
    };
    let ask = |key: &str, time, budget| {
        let request = client
            .post(format!("{base}/v1/context"))
            .bearer_auth(key)
            .json(&args(time, budget));
        async move {
            request
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .json::<Value>()
                .await
                .unwrap()
        }
    };
    let mcp_ask = |time, budget| {
        let request = client
            .post(format!("{base}/mcp"))
            .bearer_auth(&fixture.token)
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", "2025-11-25")
            .json(
                &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
                    "name":"context_bundle","arguments":args(time, budget)
                }}),
            );
        async move {
            let reply: Value = request
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .json()
                .await
                .unwrap();
            serde_json::from_str::<Value>(reply["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap()
        }
    };
    let nothing = ask(&fixture.token, DECEMBER_START, 4000).await;
    assert_eq!(nothing["items"], json!([]));

    publish_explicit_policy(&fixture, identity("境内"), "160元").await;
    let standalone = ask(&fixture.token, DECEMBER_START, 4000).await;
    assert_eq!(standalone["items"][0]["semantic_status"], "confirmed");
    assert_eq!(standalone["items"][0]["semantic_value"], "160元");
    let legacy_bytes = standalone["used_bytes"].as_u64().unwrap() as usize;

    let original = publish(
        &fixture,
        "original",
        ORIGINAL,
        "境内",
        "100 元",
        Some("2026-01-01"),
    )
    .await;
    let upcoming = publish(
        &fixture,
        "upcoming",
        UPCOMING,
        "境内",
        "80 元",
        Some("2026-12-01"),
    )
    .await;
    let missing = ask(&fixture.token, 0, 4000).await;
    let before = ask(&fixture.token, END_OF_NOVEMBER, 4000).await;
    let after = ask(&fixture.token, DECEMBER_START, 4000).await;
    let item = &before["items"][0];
    let pinned = &item["provenance"];
    assert_eq!(before["items"].as_array().unwrap().len(), 1);
    assert_eq!(item["identity"]["context"], json!({"region":"境内"}));
    assert_eq!(item["applicability"], "unverified");
    assert_eq!(pinned["type"], "long_term_memory");
    assert_eq!(pinned["as_of_ms"], END_OF_NOVEMBER);
    assert_eq!(pinned["timeline"][1]["effective_from_ms"], DECEMBER_START);
    assert_eq!(
        pinned["timeline"][1]["state"],
        json!({"status":"current","value":"80 元"})
    );
    assert_current(pinned, "100 元");
    assert_current(&after["items"][0]["provenance"], "80 元");
    assert_eq!(
        missing["items"][0]["provenance"]["state"],
        json!({"status":"unknown"})
    );
    assert!(missing["items"][0]["semantic_value"].is_null());
    assert_eq!(
        pinned["citations"][0]["source_event_id"],
        original["source_event_id"]
    );
    assert_eq!(
        pinned["citations"][1]["source_event_id"],
        upcoming["source_event_id"]
    );
    for citation in pinned["citations"].as_array().unwrap() {
        let source_id = id(citation, "source_event_id");
        let get = |key: &str| {
            client
                .get(format!("{base}/v1/events/{source_id}"))
                .bearer_auth(key)
        };
        let source: Value = get(&fixture.token)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        let statement = &citation["statement"];
        let start = statement["byte_start"].as_u64().unwrap() as usize;
        let end = statement["byte_end"].as_u64().unwrap() as usize;
        assert_eq!(
            source["content"].as_str().unwrap().get(start..end).unwrap(),
            statement["quote"]
        );
        assert_eq!(get(&other_key).send().await.unwrap().status(), 404);
    }

    for (time, bundle) in [
        (0, &missing),
        (END_OF_NOVEMBER, &before),
        (DECEMBER_START, &after),
    ] {
        assert_eq!(mcp_ask(time, 4000).await["items"], bundle["items"]);
        assert_eq!(bundle["items"].as_array().unwrap().len(), 1);
        assert!(!bundle.to_string().contains("160元"));
    }
    assert_eq!(
        ask(&other_key, DECEMBER_START, 4000).await["items"],
        json!([])
    );

    let session = fixture
        .service
        .private_session(&fixture.auth)
        .await
        .unwrap();
    let session_id = id(&session, "session_id");
    let turn_id = Uuid::new_v4();
    let archive = client
        .post(format!("{base}/v1/sessions/{session_id}/turns"))
        .bearer_auth(&fixture.token)
        .header("Idempotency-Key", "dated-context-private-fixture")
        .json(&json!({"turn_id":turn_id,"question":POLICY_QUESTION,
            "answer":"合成私有会话答复","used_evidence":[]}))
        .send()
        .await
        .unwrap();
    assert_eq!(archive.status(), 200);
    for (date, expected) in [(END_OF_NOVEMBER, "100 元"), (DECEMBER_START, "80 元")] {
        let mut mixed = args(date, 8000);
        mixed["routes"] = json!([{"route":"memory"},{"route":"session"}]);
        let ask = |token| {
            client
                .post(format!("{base}/v1/context"))
                .bearer_auth(token)
                .json(&mixed)
        };
        let result: Value = ask(&fixture.token)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        let items = result["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_current(&items[0]["provenance"], expected);
        assert_eq!(items[1]["provenance"]["turn_id"], json!(turn_id));
        let isolated: Value = ask(&other_key)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(isolated["items"], json!([]));
    }

    let dispute = "通知：云舟境内酒店限额自2026-12-01起为每晚120 元。";
    let disputed_source = publish(
        &fixture,
        "dispute",
        dispute,
        "境内",
        "120 元",
        Some("2026-12-01"),
    )
    .await;
    let conflicting = ask(&fixture.token, DECEMBER_START, 4000).await;
    let conflict = &conflicting["items"][0]["provenance"];
    assert_eq!(conflict["state"], json!({"status":"conflicting"}));
    assert_eq!(conflicting["items"].as_array().unwrap().len(), 1);
    assert!(conflicting["items"][0]["semantic_value"].is_null());
    assert_eq!(conflicting["conflicts"].as_array().unwrap().len(), 1);
    assert_eq!(
        conflicting["conflicts"][0]["conflict_id"],
        conflicting["items"][0]["conflict_id"]
    );
    assert_eq!(
        conflicting["conflicts"][0]["evidence_ids"],
        json!([conflicting["items"][0]["evidence_id"]])
    );
    assert_eq!(conflicting["omitted_conflict_groups"], 0);
    let mcp_conflict = mcp_ask(DECEMBER_START, 4000).await;
    assert_eq!(mcp_conflict["items"], conflicting["items"]);
    assert_eq!(mcp_conflict["conflicts"], conflicting["conflicts"]);
    assert_eq!(
        conflict["citations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["effective_at_ms"] == DECEMBER_START)
            .count(),
        2
    );
    let bytes = conflicting["rendered_context"].as_str().unwrap().len();
    assert_eq!(conflicting["used_bytes"], bytes);
    assert!(legacy_bytes < bytes);
    let omitted = ask(&fixture.token, DECEMBER_START, legacy_bytes).await;
    assert_eq!(omitted["items"], json!([]));
    assert_eq!(omitted["rendered_context"], "");
    assert_eq!(omitted["conflicts"], json!([]));
    assert_eq!(omitted["omitted_conflict_groups"], 1);
    let mcp_omitted = mcp_ask(DECEMBER_START, legacy_bytes).await;
    assert_eq!(mcp_omitted["items"], omitted["items"]);
    assert_eq!(mcp_omitted["omitted_conflict_groups"], 1);

    // The future conflict is present in the timeline, outside this request's date.
    let earlier = ask(&fixture.token, END_OF_NOVEMBER, 4000).await;
    assert_current(&earlier["items"][0]["provenance"], "100 元");
    assert_eq!(
        earlier["items"][0]["provenance"]["timeline"][1]["state"]["status"],
        "conflicting"
    );
    assert_eq!(earlier["conflicts"], json!([]));
    for date in [0, END_OF_NOVEMBER] {
        let omitted = ask(&fixture.token, date, legacy_bytes).await;
        assert_eq!(omitted["items"], json!([]));
        assert_eq!(omitted["omitted_conflict_groups"], 0);
        assert_eq!(omitted["routes"][0]["omitted_for_budget"], 1);
    }

    let mut distinct = identity("境内");
    distinct.context.insert("traveler".into(), "实习生".into());
    publish_explicit_policy(&fixture, distinct.clone(), "180元").await;
    let mixed = ask(&fixture.token, DECEMBER_START, 4000).await;
    let mixed = mixed["items"].as_array().unwrap();
    assert_eq!(mixed.len(), 2);
    let scoped = mixed
        .iter()
        .find(|item| item["provenance"]["type"] == "asset_chunk")
        .unwrap();
    assert_eq!(scoped["identity"], json!(distinct));
    assert_eq!(scoped["semantic_value"], "180元");

    let removed = id(&disputed_source, "source_event_id");
    let withdrawn = client
        .delete(format!("{base}/v1/events/{removed}"))
        .bearer_auth(&fixture.token)
        .header("Idempotency-Key", "remove-conflicting-source")
        .send()
        .await
        .unwrap();
    assert!(withdrawn.status().is_success());
    let revised = ask(&fixture.token, DECEMBER_START, 4000).await;
    let current = &revised["items"][0]["provenance"];
    assert_current(current, "80 元");
    assert_eq!(current["memory_id"], conflict["memory_id"]);
    assert!(current["version"].as_i64() > conflict["version"].as_i64());
    assert!(!revised.to_string().contains(&removed.to_string()));
    let historical = client
        .get(format!(
            "{base}/v1/memories/long-term/{}?version={}",
            id(conflict, "memory_id"),
            conflict["version"]
        ))
        .bearer_auth(&fixture.token)
        .send()
        .await
        .unwrap();
    assert_eq!(historical.status(), 404);
    assert_eq!(
        client
            .get(format!("{base}/v1/events/{removed}"))
            .bearer_auth(&fixture.token)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
}

#[tokio::test]
async fn retain_replays_and_materializes_a_dated_policy_through_http() {
    let fixture = setup().await;
    let baseline = "通知：云舟境内酒店每晚最多报100 元。";
    let (base, _api) = serve(origence::api::router(fixture.service.clone())).await;
    let client = reqwest::Client::new();
    let post_retain = |content: &str, token: &str| {
        client
            .post(format!("{base}/v1/memories/retain"))
            .bearer_auth(token)
            .header("Idempotency-Key", "original-hotel-policy")
            .json(&json!({"content":content}))
    };
    let initial = post_retain(baseline, &fixture.token).send().await.unwrap();
    assert!(initial.status().is_success());
    let first: Value = initial.json().await.unwrap();
    let retry: Value = post_retain(baseline, &fixture.token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(first, retry);
    assert_eq!(
        post_retain(UPCOMING, &fixture.token)
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    assert_eq!(fixture.stub.calls.load(Ordering::SeqCst), 0);
    assert!(matches!(
        fixture.recall("境内", None, DECEMBER_START).await,
        Err(AppError::NotFound)
    ));
    let source = fixture
        .service
        .source_event(&fixture.auth, id(&first, "source_event_id"))
        .await
        .unwrap();
    assert_eq!(source["content"], baseline);

    fixture
        .stub
        .respond(vec![fact(baseline, "境内", "100 元", None)]);
    assert_eq!(settle(&fixture, &first).await["state"], "completed");
    let initial_view = fixture.recall("境内", None, DECEMBER_START).await.unwrap();
    let second = publish(
        &fixture,
        "later",
        UPCOMING,
        "境内",
        "80 元",
        Some("2026-12-01"),
    )
    .await;
    let as_of = |time_ms| {
        client
            .post(format!("{base}/v1/memories/long-term/lookup"))
            .bearer_auth(&fixture.token)
            .json(&json!({"identity":identity("境内"),"as_of_ms":time_ms}))
    };
    let before: Value = as_of(END_OF_NOVEMBER)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let after: Value = as_of(DECEMBER_START)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_current(&before, "100 元");
    assert_current(&after, "80 元");
    assert_eq!(after["memory_id"], initial_view["memory_id"]);
    assert_eq!(
        after["version"],
        initial_view["version"].as_i64().unwrap() + 1
    );
    assert_eq!(before["version"], after["version"]);
    assert_eq!(
        before["timeline"][0],
        json!({"effective_from_ms":null,"state":{"status":"current","value":"100 元"}})
    );
    let future = before["timeline"]
        .as_array()
        .unwrap()
        .iter()
        .find(|period| period["effective_from_ms"] == DECEMBER_START)
        .unwrap();
    assert_eq!(future["state"], json!({"status":"current","value":"80 元"}));
    let by_id = client
        .get(format!(
            "{base}/v1/memories/long-term/{}?version=1&as_of_ms={DECEMBER_START}",
            id(&after, "memory_id")
        ))
        .bearer_auth(&fixture.token)
        .send()
        .await
        .unwrap();
    assert_eq!(by_id.status(), 200);
    assert_eq!(by_id.json::<Value>().await.unwrap(), initial_view);
    let sources: Vec<&Value> = after["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| &e["source_event_id"])
        .collect();
    assert!(sources.contains(&&first["source_event_id"]));
    assert!(sources.contains(&&second["source_event_id"]));

    // A backdated report can fill a historical interval while the new rule stays in force.
    let late = "通知：云舟境内酒店限额自2026-07-01起为每晚90 元。";
    publish(
        &fixture,
        "arrived-late",
        late,
        "境内",
        "90 元",
        Some("2026-07-01"),
    )
    .await;
    let after_late = fixture.recall("境内", None, DECEMBER_START).await.unwrap();
    assert_current(&after_late, "80 元");
    assert_current(
        &fixture.recall("境内", None, END_OF_NOVEMBER).await.unwrap(),
        "90 元",
    );
    let old_version = fixture
        .recall("境内", Some(1), DECEMBER_START)
        .await
        .unwrap();
    assert_current(&old_version, "100 元");
    assert_eq!(old_version["evidence"].as_array().unwrap().len(), 1);
    assert_eq!(fixture.stub.calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn conflicting_evidence_and_withdrawal_stay_within_the_exact_scope() {
    let fixture = setup().await;
    publish(
        &fixture,
        "baseline",
        ORIGINAL,
        "境内",
        "100 元",
        Some("2026-01-01"),
    )
    .await;
    publish(
        &fixture,
        "incoming",
        UPCOMING,
        "境内",
        "80 元",
        Some("2026-12-01"),
    )
    .await;
    let abroad = "通知：云舟境外酒店限额自2026-12-01起为每晚300 元。";
    let sole_source = publish(
        &fixture,
        "abroad",
        abroad,
        "境外",
        "300 元",
        Some("2026-12-01"),
    )
    .await;
    let dispute = "通知：云舟境内酒店限额自2026-12-01起为每晚120 元。";
    let disputed_source = publish(
        &fixture,
        "dispute",
        dispute,
        "境内",
        "120 元",
        Some("2026-12-01"),
    )
    .await;
    let previous = fixture.recall("境内", None, DECEMBER_START).await.unwrap();
    assert_eq!(previous["state"]["status"], "conflicting");
    assert!(previous["state"].get("value").is_none());
    assert_current(
        &fixture.recall("境内", None, END_OF_NOVEMBER).await.unwrap(),
        "100 元",
    );
    assert_current(
        &fixture.recall("境外", None, DECEMBER_START).await.unwrap(),
        "300 元",
    );
    let (other, _) = provision(&fixture.service, fixture.auth.tenant_id).await;
    assert!(matches!(
        recall(&fixture.service, &other, "境内", None, DECEMBER_START).await,
        Err(AppError::NotFound)
    ));

    let removed = id(&disputed_source, "source_event_id");
    fixture
        .service
        .delete(&fixture.auth, "withdraw-dispute", removed, "event")
        .await
        .unwrap();
    let remaining = fixture.recall("境内", None, DECEMBER_START).await.unwrap();
    assert_current(&remaining, "80 元");
    assert_eq!(remaining["memory_id"], previous["memory_id"]);
    assert!(remaining["version"].as_i64().unwrap() > previous["version"].as_i64().unwrap());
    assert!(
        remaining["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| id(e, "source_event_id") != removed)
    );
    let historical = fixture
        .recall(
            "境内",
            Some(previous["version"].as_i64().unwrap() as i32),
            DECEMBER_START,
        )
        .await;
    match historical {
        Ok(view) => {
            assert!(
                view["evidence"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|e| id(e, "source_event_id") != removed)
            );
            assert!(!view.to_string().contains("120 元"));
        }
        Err(AppError::NotFound) => {}
        Err(error) => panic!("unexpected historical recall error: {error}"),
    }

    fixture
        .service
        .delete(
            &fixture.auth,
            "withdraw-only-support",
            id(&sole_source, "source_event_id"),
            "event",
        )
        .await
        .unwrap();
    for version in [None, Some(1)] {
        assert!(matches!(
            fixture.recall("境外", version, DECEMBER_START).await,
            Err(AppError::NotFound)
        ));
    }
    assert_eq!(fixture.stub.calls.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn insufficient_or_fabricated_evidence_cannot_change_the_current_memory() {
    let fixture = setup().await;
    publish(
        &fixture,
        "baseline",
        ORIGINAL,
        "境内",
        "100 元",
        Some("2026-01-01"),
    )
    .await;
    let ambiguous = "收到了云舟酒店最多报80 元的消息。";
    fixture.stub.respond(vec![]);
    let ambiguous_source = retain(&fixture, "insufficient", ambiguous).await;
    assert_eq!(
        settle(&fixture, &ambiguous_source).await["state"],
        "completed"
    );
    assert_eq!(
        fixture
            .service
            .source_event(&fixture.auth, id(&ambiguous_source, "source_event_id"))
            .await
            .unwrap()["content"],
        ambiguous
    );

    // Missing a date must not activate a future rule early or extend a finite rule indefinitely.
    let limited = "通知：云舟境内酒店限额自2026-12-01起为每晚80 元，截至2026-12-31。";
    for (key, content, start) in [
        ("missing-effective-date", UPCOMING, None),
        ("omitted-expiry", limited, Some("2026-12-01")),
    ] {
        fixture
            .stub
            .respond(vec![fact(content, "境内", "80 元", start)]);
        let retained = retain(&fixture, key, content).await;
        let date_job = settle(&fixture, &retained).await;
        assert_eq!(date_job["state"], "completed", "{date_job}");
        assert_eq!(date_job["result"]["fact_count"], 0);
        assert_eq!(
            fixture
                .service
                .source_event(&fixture.auth, id(&retained, "source_event_id"))
                .await
                .unwrap()["content"],
            content
        );
    }

    let valid = fact(UPCOMING, "境内", "80 元", Some("2026-12-01"));
    let mut malformed = valid.clone();
    malformed["statement"]["byte_start"] = json!(1);
    fixture.stub.respond(vec![valid, malformed]);
    let invalid = retain(&fixture, "fake-span", UPCOMING).await;
    let job = settle(&fixture, &invalid).await;
    assert_eq!(job["state"], "failed", "{job}");

    let different_price = "通知：云舟境内酒店限额自2026-12-01起为每晚180 元。";
    fixture.stub.respond(vec![fact(
        different_price,
        "境内",
        "80 元",
        Some("2026-12-01"),
    )]);
    let truncated = retain(&fixture, "substring-is-not-a-value", different_price).await;
    assert_eq!(settle(&fixture, &truncated).await["state"], "failed");

    let digit_clip = "通知：180 元是云舟境内酒店自2026-12-01起的每晚上限。";
    let mut clipped = fact(digit_clip, "境内", "80 元", Some("2026-12-01"));
    let clip_start = digit_clip.find("80 元").unwrap();
    clipped["statement"]["byte_start"] = json!(clip_start);
    clipped["statement"]["quote"] = json!(&digit_clip[clip_start..]);
    fixture.stub.respond(vec![clipped]);
    let clipped_source = retain(&fixture, "clipped-quote-is-not-a-value", digit_clip).await;
    assert_eq!(settle(&fixture, &clipped_source).await["state"], "failed");

    let visible = fixture.recall("境内", None, DECEMBER_START).await.unwrap();
    assert_current(&visible, "100 元");
    assert_eq!(visible["version"], 1);
    assert_eq!(visible["evidence"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn acknowledged_source_and_saved_plan_survive_a_worker_restart() {
    let mut fixture = setup().await;
    fixture
        .stub
        .respond(vec![fact(UPCOMING, "境内", "80 元", Some("2026-12-01"))]);
    let receipt = retain(&fixture, "future-restart", UPCOMING).await;
    let job_id = id(&receipt, "job_id");
    let pool = fixture.service.engine.relational().pool();
    sqlite_db::raw_sql(
        "CREATE TRIGGER fixture_reject_conclusion BEFORE INSERT ON oc_long_term_memories
         BEGIN SELECT RAISE(ABORT,'fixture conclusion failure'); END;",
    )
    .execute(pool)
    .await
    .unwrap();
    assert!(worker::process_next(&fixture.service).await.unwrap());
    assert_eq!(
        fixture.service.job(&fixture.auth, job_id).await.unwrap()["state"],
        "retry_wait"
    );
    assert!(matches!(
        fixture.recall("境内", None, DECEMBER_START).await,
        Err(AppError::NotFound)
    ));
    sqlite_db::raw_sql("DROP TRIGGER fixture_reject_conclusion")
        .execute(pool)
        .await
        .unwrap();
    for action in ["cancel", "retry"] {
        fixture
            .service
            .job_action(&fixture.auth, action, job_id, action)
            .await
            .unwrap();
    }
    let claim = fixture
        .service
        .engine
        .queue()
        .claim_next()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claim.job_id, job_id);
    assert_eq!(
        claim.payload["consolidation_plan"][0]["statement"]["value"],
        "80 元"
    );
    assert_eq!(fixture.stub.calls.load(Ordering::SeqCst), 1);
    // A replay that accidentally calls the model will produce no facts.
    fixture.stub.respond(Vec::new());
    let models = fixture.service.models.clone();
    fixture.service.engine.relational().pool().close().await;
    drop(fixture.service);
    fixture.service = support::open_service(fixture.dir.path(), models)
        .await
        .unwrap();
    fixture.auth = fixture.service.auth(&fixture.token).await.unwrap();
    assert_eq!(retain(&fixture, "future-restart", UPCOMING).await, receipt);
    assert_eq!(settle(&fixture, &receipt).await["state"], "completed");
    let not_yet = fixture.recall("境内", None, END_OF_NOVEMBER).await.unwrap();
    assert_eq!(not_yet["state"]["status"], "unknown");
    let in_effect = fixture.recall("境内", None, DECEMBER_START).await.unwrap();
    assert_current(&in_effect, "80 元");
    assert_eq!(in_effect["version"], 1);
    assert_eq!(fixture.stub.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn retry_after_model_failure_keeps_the_original_source_and_receipt() {
    let fixture = setup().await;
    fixture.stub.unavailable.store(true, Ordering::SeqCst);
    let receipt = retain(&fixture, "retry-model", ORIGINAL).await;
    let failed = settle(&fixture, &receipt).await;
    assert_eq!(failed["state"], "failed", "{failed}");
    assert!(matches!(
        fixture.recall("境内", None, DECEMBER_START).await,
        Err(AppError::NotFound)
    ));
    assert_eq!(retain(&fixture, "retry-model", ORIGINAL).await, receipt);
    fixture.stub.unavailable.store(false, Ordering::SeqCst);
    fixture
        .stub
        .respond(vec![fact(ORIGINAL, "境内", "100 元", Some("2026-01-01"))]);
    fixture
        .service
        .job_action(
            &fixture.auth,
            "retry-extraction",
            id(&receipt, "job_id"),
            "retry",
        )
        .await
        .unwrap();
    assert_eq!(settle(&fixture, &receipt).await["state"], "completed");
    let restored = fixture.recall("境内", None, DECEMBER_START).await.unwrap();
    assert_current(&restored, "100 元");
    assert_eq!(restored["version"], 1);
    let evidence = restored["evidence"].as_array().unwrap();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0]["source_event_id"], receipt["source_event_id"]);
    assert_eq!(fixture.stub.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn write_authority_is_rechecked_before_pending_evidence_can_publish() {
    let fixture = setup().await;
    let store = fixture.service.engine.relational();
    let token = store.rotate_deployment_admin().await.unwrap();
    let admin = fixture
        .service
        .authorize_deployment_workspace(&token, fixture.auth.workspace_id)
        .await
        .unwrap();
    let auth = &fixture.auth;
    let receipt = fixture
        .service
        .retain_memory(
            auth,
            "accepted-before-revoke",
            CaptureInput {
                content: ORIGINAL.into(),
            },
        )
        .await
        .unwrap();
    fixture
        .stub
        .respond(vec![fact(ORIGINAL, "境内", "100 元", Some("2026-01-01"))]);
    fixture
        .service
        .engine
        .relational()
        .revoke_key(auth.id)
        .await
        .unwrap();
    assert!(worker::process_next(&fixture.service).await.unwrap());
    assert_eq!(
        fixture
            .service
            .job(&admin, id(&receipt, "job_id"))
            .await
            .unwrap()["state"],
        "failed"
    );
    assert_eq!(fixture.stub.calls.load(Ordering::SeqCst), 0);
    let retained = fixture
        .service
        .source_event(&admin, id(&receipt, "source_event_id"))
        .await
        .unwrap();
    assert_eq!(retained["content"], ORIGINAL);
}

#[path = "support/fixture_keys.rs"]
mod fixture_keys;
