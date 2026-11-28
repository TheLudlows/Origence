#![cfg(feature = "host")]
use axum::{Json, Router, extract::State};
use origence::storage::sqlite_db as db;
use origence::{models::Models, storage::*, types::*};
use serde_json::{Value, json};
use std::{
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
};
use uuid::Uuid;
fn id(v: &Value, k: &str) -> Uuid {
    serde_json::from_value(v[k].clone()).unwrap()
}
fn command(dir: &std::path::Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_origence"));
    c.env("OC_DATA_DIR", dir)
        .env("OC_ENABLE_MODELS", "false")
        .env("RUST_LOG", "error")
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    c
}
async fn stop(child: &mut Child) {
    child.kill().await.unwrap();
    child.wait().await.unwrap();
}
async fn embed(State(slow): State<Arc<AtomicBool>>, Json(_): Json<Value>) -> Json<Value> {
    if slow.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    Json(json!({"data":[{"embedding":[1.0,0.5,0.25]}]}))
}
async fn extract(Json(v): Json<Value>) -> Json<Value> {
    let prompt = format!(
        "{}{}",
        v["messages"][0]["content"].as_str().unwrap_or(""),
        v["messages"][1]["content"].as_str().unwrap_or("")
    );
    let content = if prompt.contains("one explicit identity") {
        let input: Value =
            serde_json::from_str(v["messages"][1]["content"].as_str().unwrap()).unwrap();
        let source = input["source"].as_str().unwrap();
        if source == "CAPTURECONFLICT" {
            json!({"memories":[{"quote":"CAPTURE","byte_start":0,"byte_end":7},{"quote":"CONFLICT","byte_start":7,"byte_end":15}]}).to_string()
        } else {
            json!({"memories":[{"quote":source,"byte_start":0,"byte_end":source.len()}]})
                .to_string()
        }
    } else if prompt.contains("Summarize") {
        "Atlas summary evidence".to_string()
    } else if prompt.contains("GRAPH-CHAIN-FIXTURE") {
        json!({"memories":[],"entities":[
            {"name":"seedalpha","entity_type":"node","description":"seed"},
            {"name":"neighborbeta","entity_type":"node","description":"outgoing"},
            {"name":"secondgamma","entity_type":"node","description":"second hop"},
            {"name":"incomingdelta","entity_type":"node","description":"incoming"}
        ],"relations":[
            {"source":"seedalpha","predicate":"points_to","target":"neighborbeta"},
            {"source":"neighborbeta","predicate":"points_to","target":"secondgamma"},
            {"source":"incomingdelta","predicate":"points_to","target":"seedalpha"}
        ]})
        .to_string()
    } else if prompt.contains("EMPTY-EXTRACT") {
        json!({"memories":[],"entities":[],"relations":[]}).to_string()
    } else {
        json!({"memories":[
            {"fact_key":"model.claim","content":"Model proposal requires review","publish_if_authorized":true},
            {"fact_key":"model.second","content":"Second extracted fact","publish_if_authorized":true},
            {"fact_key":"model.claim","content":"A different claim under the same key","publish_if_authorized":true},
            {"fact_key":"model.claim","content":"Model proposal requires review","publish_if_authorized":true}
        ],"entities":[{"name":"Atlas","entity_type":"service","description":"payments"},{"name":"Team","entity_type":"team","description":"owner"}],"relations":[{"source":"Atlas","predicate":"owned_by","target":"Team"}]}).to_string()
    };
    Json(json!({"choices":[{"message":{"content":content}}]}))
}
async fn request(
    http: &reqwest::Client,
    base: &str,
    token: &str,
    method: reqwest::Method,
    path: &str,
    body: Value,
) -> Value {
    let r = http
        .request(method, format!("{base}{path}"))
        .bearer_auth(token)
        .header("Idempotency-Key", Uuid::new_v4().to_string())
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = r.status();
    let value = r.json::<Value>().await.unwrap();
    assert!(status.is_success(), "{path} {status}: {value}");
    value
}
async fn post(http: &reqwest::Client, base: &str, token: &str, path: &str, body: Value) -> Value {
    request(http, base, token, reqwest::Method::POST, path, body).await
}
async fn job(http: &reqwest::Client, base: &str, token: &str, job: Uuid, state: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            let v = request(
                http,
                base,
                token,
                reqwest::Method::GET,
                &format!("/v1/jobs/{job}"),
                Value::Null,
            )
            .await;
            if v["state"] == state {
                return v;
            }
            assert!(
                !["failed", "superseded", "cancelled"].contains(&v["state"].as_str().unwrap()),
                "unexpected job {v}"
            );
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    })
    .await
    .expect("job deadline")
}
async fn start(dir: &std::path::Path, model: &str) -> (Child, String) {
    let mut c = command(dir);
    crate::support::fixture(dir, Some(3)).bind_profile(Some(format!(
        "{}:stub:3:v1",
        origence::storage::hash(model)
    )));
    crate::support::command_env(&mut c, dir, Some(3));
    c.env("OC_ENABLE_MODELS", "true")
        .env("OC_MODEL_BASE_URL", model)
        .env("OC_MODEL_API_KEY", "stub")
        .env("OC_EMBEDDING_MODEL", "stub")
        .env("OC_EMBEDDING_DIMENSION", "3")
        .env("OC_EXTRACTION_MODEL", "stub")
        .args(["serve", "--bind", "127.0.0.1:0"])
        .stdout(Stdio::piped());
    let mut child = c.spawn().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
    let line = tokio::time::timeout(Duration::from_secs(30), output.next_line())
        .await
        .unwrap()
        .unwrap()
        .expect("host started");
    let v: Value = serde_json::from_str(&line).unwrap();
    let base = format!("http://{}", v["listening"].as_str().unwrap());
    let http = reqwest::Client::new();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if http
                .get(format!("{base}/health/ready"))
                .send()
                .await
                .is_ok_and(|r| r.status().is_success())
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .unwrap();
    (child, base)
}
fn pdf() -> Vec<u8> {
    super::pdf_fixture::text_pdf(&["Release approval evidence", "Second page provenance"])
}

#[tokio::test]
async fn local_host_api_cli_mcp_models_and_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let out = command(dir.path())
        .args(["--offline", "workspace-create", "local tests"])
        .output()
        .await
        .unwrap();
    assert!(out.status.success());
    let bootstrap: Value = serde_json::from_slice(&out.stdout).unwrap();
    let token = bootstrap["token"].as_str().unwrap();
    let other = command(dir.path())
        .args(["--offline", "workspace-create", "other scope"])
        .output()
        .await
        .unwrap();
    assert!(other.status.success());
    let other: Value = serde_json::from_slice(&other.stdout).unwrap();
    let slow = Arc::new(AtomicBool::new(false));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let model = format!("http://{}", listener.local_addr().unwrap());
    let mock = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/embeddings", post_route())
                .route("/chat/completions", axum::routing::post(extract))
                .with_state(slow.clone()),
        )
        .into_future(),
    );
    let (mut host, base) = start(dir.path(), &model).await;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();
    let input = json!({"fact_key":"policy","content":"Release approval required","publish_if_authorized":true});
    let published = post(&http, &base, token, "/v1/memories", input.clone()).await;
    let complete = job(&http, &base, token, id(&published, "job_id"), "completed").await;
    assert_eq!(
        complete["result"]["index_capabilities"],
        json!(["keyword", "vector"])
    );
    for mode in ["keyword", "vector", "hybrid"] {
        let v = post(
            &http,
            &base,
            token,
            "/v1/search",
            json!({"query":"approval","mode":mode}),
        )
        .await;
        assert_eq!(v["hits"][0]["asset_id"], published["asset_id"]);
        assert_eq!(v["effective_mode"], mode);
    }
    // Another workspace cannot see an exact native-vector match or guess an object ID.
    let isolated = post(
        &http,
        &base,
        other["token"].as_str().unwrap(),
        "/v1/search",
        json!({"query":"approval","mode":"hybrid"}),
    )
    .await;
    assert_eq!(isolated["hits"], json!([]));
    assert_eq!(
        http.get(format!(
            "{base}/v1/assets/{}",
            published["asset_id"].as_str().unwrap()
        ))
        .bearer_auth(other["token"].as_str().unwrap())
        .send()
        .await
        .unwrap()
        .status(),
        404
    );
    let cli = command(dir.path())
        .env("OC_API_KEY", token)
        .env("OC_SERVER_URL", &base)
        .args(["search", "approval"])
        .output()
        .await
        .unwrap();
    assert!(cli.status.success());
    let cli: Value = serde_json::from_slice(&cli.stdout).unwrap();
    assert_eq!(cli["hits"][0]["asset_id"], published["asset_id"]);
    let blocked = command(dir.path())
        .args(["--offline", "workspace-create", "blocked"])
        .output()
        .await
        .unwrap();
    assert!(!blocked.status.success());
    // Extraction publishes under the capture creator's own write authority.
    let capture = post(
        &http,
        &base,
        token,
        "/v1/captures",
        json!({"content":"Proposed policy"}),
    )
    .await;
    let extracted = job(&http, &base, token, id(&capture, "job_id"), "completed").await;
    assert_eq!(extracted["outcome"], "published");
    assert_eq!(extracted["result"]["readiness"], "ready");
    assert_eq!(
        extracted["result"]["index_capabilities"],
        json!(["keyword", "vector"])
    );
    let claim = &extracted["result"]["memories"][0];
    assert_eq!(claim["fact_key"], "model.claim");
    assert_eq!(claim["version"], 1);
    let recall = post(
        &http,
        &base,
        token,
        "/v1/search",
        json!({"query":"proposal","mode":"keyword"}),
    )
    .await;
    assert_eq!(recall["hits"][0]["asset_id"], claim["asset_id"]);
    let second = &extracted["result"]["memories"][1];
    assert_eq!(second["fact_key"], "model.second");
    assert_eq!(second["version"], 1);
    // Distinct facts sharing a key are retained. Exact repeats within the
    // same extraction collapse to one candidate.
    assert_eq!(extracted["result"]["memories"].as_array().unwrap().len(), 3);
    let alternative = &extracted["result"]["memories"][2];
    assert!(alternative["fact_key"].is_null());
    assert_ne!(alternative["asset_id"], claim["asset_id"]);
    let repeated = post(
        &http,
        &base,
        token,
        "/v1/captures",
        json!({"content":"More evidence"}),
    )
    .await;
    let repeated = job(&http, &base, token, id(&repeated, "job_id"), "completed").await;
    let repeated_first = &repeated["result"]["memories"][0];
    assert!(repeated_first["fact_key"].is_null());
    assert_ne!(repeated_first["asset_id"], claim["asset_id"]);
    let standalone = request(
        &http,
        &base,
        token,
        reqwest::Method::GET,
        &format!(
            "/v1/assets/{}?version=1",
            repeated_first["asset_id"].as_str().unwrap()
        ),
        Value::Null,
    )
    .await;
    assert_eq!(standalone["version"], 1);
    assert_eq!(standalone["normalization_status"], "unnormalized");
    let original_claim = request(
        &http,
        &base,
        token,
        reqwest::Method::GET,
        &format!("/v1/assets/{}", claim["asset_id"].as_str().unwrap()),
        Value::Null,
    )
    .await;
    assert_eq!(original_claim["version"], 1);
    assert_eq!(
        original_claim["normalization_status"],
        "legacy_unidentified"
    );
    let identity = json!({"subject":{"kind":"service","stable_id":"billing"},"predicate":"release.approval","context":{"environment":"production"}});
    let first_key = Uuid::new_v4().to_string();
    let first_body = json!({"identity":identity,"content":"生产发布需要审批","expected_version":0});
    let first = http
        .post(format!("{base}/v1/captures/identified"))
        .bearer_auth(token)
        .header("Idempotency-Key", &first_key)
        .json(&first_body)
        .send()
        .await
        .unwrap();
    assert!(first.status().is_success());
    let first: Value = first.json().await.unwrap();
    let identity_job = job(&http, &base, token, id(&first, "job_id"), "completed").await;
    assert_eq!(identity_job["result"]["memories"][0]["identity"], identity);
    let second = post(
        &http,
        &base,
        token,
        "/v1/captures/identified",
        json!({"identity":identity,"content":"生产发布需要双人审批","expected_version":1}),
    )
    .await;
    assert_ne!(first["asset_id"], second["asset_id"]);
    let appended = job(&http, &base, token, id(&second, "job_id"), "completed").await;
    assert_eq!(
        appended["result"]["memories"][0]["asset_id"],
        second["asset_id"]
    );
    assert!(appended["result"]["memories"][0]["identity"].is_null());
    assert_eq!(appended["result"]["memories"][0]["version"], 1);
    let view = request(
        &http,
        &base,
        token,
        reqwest::Method::GET,
        &format!("/v1/assets/{}", first["asset_id"].as_str().unwrap()),
        Value::Null,
    )
    .await;
    assert_eq!(view["version"], 1);
    assert_eq!(view["identity"], identity);
    let replay: Value = http
        .post(format!("{base}/v1/captures/identified"))
        .bearer_auth(token)
        .header("Idempotency-Key", &first_key)
        .json(&first_body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(replay, first);
    for path in ["/v1/memories/identified", "/v1/captures/identified"] {
        for (expected, status) in [(0, 409), (2, 409), (-1, 422)] {
            let rejected = http
                .post(format!("{base}{path}"))
                .bearer_auth(token)
                .header("Idempotency-Key", Uuid::new_v4().to_string())
                .json(&json!({"identity":identity,"content":"stale","expected_version":expected}))
                .send()
                .await
                .unwrap();
            assert_eq!(rejected.status(), status);
        }
    }

    let ambiguous = post(
        &http,
        &base,
        token,
        "/v1/captures/identified",
        json!({"identity":identity,"content":"CAPTURECONFLICT"}),
    )
    .await;
    let failed = job(&http, &base, token, id(&ambiguous, "job_id"), "failed").await;
    assert_eq!(failed["error_code"], "INVALID_ARGUMENT");
    let unchanged = request(
        &http,
        &base,
        token,
        reqwest::Method::GET,
        &format!("/v1/assets/{}", first["asset_id"].as_str().unwrap()),
        Value::Null,
    )
    .await;
    assert_eq!(unchanged["version"], 1);
    assert_eq!(unchanged["normalization_status"], "explicit_identity");
    let source_path = format!(
        "/v1/events/{}",
        ambiguous["source_event_id"].as_str().unwrap()
    );
    let source = request(
        &http,
        &base,
        token,
        reqwest::Method::GET,
        &source_path,
        Value::Null,
    )
    .await;
    assert_eq!(source["content"], "CAPTURECONFLICT");
    assert_eq!(
        http.get(format!("{base}{source_path}"))
            .bearer_auth(other["token"].as_str().unwrap())
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    let reader = post(
        &http,
        &base,
        token,
        "/admin/keys",
        json!({"workspace_id":bootstrap["workspace_id"],"role":"reader"}),
    )
    .await;
    assert_eq!(
        http.get(format!("{base}{source_path}"))
            .bearer_auth(reader["token"].as_str().unwrap())
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let lookup = http
        .post(format!("{base}/v1/memories/lookup"))
        .bearer_auth(reader["token"].as_str().unwrap())
        .json(&identity)
        .send()
        .await
        .unwrap();
    assert_eq!(lookup.status(), 200);
    let lookup: Value = lookup.json().await.unwrap();
    assert_eq!(lookup["asset_id"], first["asset_id"]);
    assert_eq!(lookup["version"], 1);
    // Both evidence records contain these terms, so both qualify for keyword AND.
    let shared_query = "生产发布";
    let recalled = post(
        &http,
        &base,
        reader["token"].as_str().unwrap(),
        "/v1/search",
        json!({"query":shared_query,"mode":"keyword"}),
    )
    .await;
    let hit = recalled["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|hit| hit["asset_id"] == first["asset_id"])
        .unwrap();
    assert_eq!(hit["identity"], identity);
    assert_eq!(hit["normalization_status"], "explicit_identity");
    let appended_hit = recalled["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|hit| hit["asset_id"] == second["asset_id"])
        .unwrap();
    assert!(appended_hit["identity"].is_null());
    assert_eq!(appended_hit["normalization_status"], "unnormalized");
    assert_eq!(appended_hit["locator"]["suggested_identity"], identity);
    assert_eq!(appended_hit["content"], "生产发布需要双人审批");
    assert_eq!(appended_hit["source_event_id"], second["source_event_id"]);
    let context = post(
        &http,
        &base,
        reader["token"].as_str().unwrap(),
        "/v1/resolve",
        json!({"query":shared_query,"budget_tokens":3000}),
    )
    .await;
    let cited = context["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["asset_id"] == first["asset_id"])
        .unwrap();
    assert_eq!(cited["identity"], identity);
    assert_eq!(cited["normalization_status"], "explicit_identity");
    assert!(
        context["rendered_context"]
            .as_str()
            .unwrap()
            .contains("explicit_identity")
    );
    assert_eq!(
        http.post(format!("{base}/v1/memories/lookup"))
            .bearer_auth(other["token"].as_str().unwrap())
            .json(&identity)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    request(
        &http,
        &base,
        token,
        reqwest::Method::DELETE,
        &source_path,
        Value::Null,
    )
    .await;
    assert_eq!(
        http.get(format!("{base}{source_path}"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );

    let mut other_identity = identity.clone();
    other_identity["context"]["environment"] = json!("staging");
    let other = post(
        &http,
        &base,
        token,
        "/v1/captures/identified",
        json!({"identity":other_identity,"content":"测试发布免审批"}),
    )
    .await;
    assert_ne!(other["asset_id"], first["asset_id"]);
    job(&http, &base, token, id(&other, "job_id"), "completed").await;

    for (filter, asset) in [
        (&identity, &first["asset_id"]),
        (&other_identity, &other["asset_id"]),
    ] {
        let filtered = post(
            &http,
            &base,
            reader["token"].as_str().unwrap(),
            "/v1/search",
            json!({"query":"发布","limit":1,"memory_identity":filter}),
        )
        .await;
        assert_eq!(filtered["hits"].as_array().unwrap().len(), 1);
        assert_eq!(&filtered["hits"][0]["asset_id"], asset);
        assert_eq!(&filtered["hits"][0]["identity"], filter);
    }
    let filtered_context = post(
        &http,
        &base,
        reader["token"].as_str().unwrap(),
        "/v1/resolve",
        json!({"query":"发布","mode":"hybrid","memory_identity":identity,"budget_tokens":3000}),
    )
    .await;
    assert!(!filtered_context["sources"].as_array().unwrap().is_empty());
    for source in filtered_context["sources"].as_array().unwrap() {
        assert_eq!(source["identity"], identity);
    }
    assert_eq!(filtered_context["graph"]["entities"], json!([]));
    assert_eq!(filtered_context["graph"]["relations"], json!([]));

    // Zero extracted memories still completes with an empty published list.
    let empty = post(
        &http,
        &base,
        token,
        "/v1/captures",
        json!({"content":"EMPTY-EXTRACT"}),
    )
    .await;
    let none = job(&http, &base, token, id(&empty, "job_id"), "completed").await;
    assert_eq!(none["outcome"], "published");
    assert_eq!(none["result"]["memories"], json!([]));
    // The removed candidate-review surface no longer resolves.
    for (method, path) in [
        (reqwest::Method::GET, "/v1/candidates"),
        (
            reqwest::Method::GET,
            "/v1/candidates/00000000-0000-0000-0000-000000000000",
        ),
        (
            reqwest::Method::POST,
            "/v1/candidates/00000000-0000-0000-0000-000000000000/review",
        ),
    ] {
        let r = http
            .request(method, format!("{base}{path}"))
            .bearer_auth(token)
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), reqwest::StatusCode::NOT_FOUND);
    }
    // Two sources own the same graph objects; deleting one must preserve the other.
    let mut docs = Vec::new();
    for n in 0..2 {
        let doc=post(&http,&base,token,"/v1/knowledge",json!({"title":format!("Atlas {n}"),"content":"Atlas is owned by Team","format":"markdown"})).await;
        job(&http, &base, token, id(&doc, "job_id"), "completed").await;
        docs.push(doc);
    }
    let graph = post(
        &http,
        &base,
        token,
        "/v1/search",
        json!({"query":"Atlas","mode":"hybrid"}),
    )
    .await;
    assert_eq!(graph["graph"]["entities"].as_array().unwrap().len(), 2);
    assert_eq!(graph["graph"]["relations"].as_array().unwrap().len(), 1);
    // Toggle summary and graph branches independently on the same published artifacts.
    // Fixed fixture vectors prove branch wiring, not semantic retrieval quality.
    for summaries in [false, true] {
        for graph_enabled in [false, true] {
            let components = json!({"summaries":summaries,"graph":graph_enabled});
            let selected = post(
                &http,
                &base,
                token,
                "/v1/search",
                json!({"query":"Atlas","mode":"hybrid","components":components}),
            )
            .await;
            assert_eq!(selected["active_components"], components);
            assert_eq!(
                selected["graph"]["entities"].as_array().unwrap().is_empty(),
                !graph_enabled
            );
            let context = post(&http, &base, token, "/v1/resolve",
                json!({"query":"Atlas","mode":"hybrid","components":components,"budget_tokens":3000})).await;
            assert_eq!(context["active_components"], components);
            if !graph_enabled {
                assert!(
                    context["sources"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|s| s.get("entity_id").is_none() && s.get("relation_id").is_none())
                );
            }
        }
    }
    let mut summary_scores = Vec::new();
    for summaries in [false, true] {
        let selected = post(&http, &base, token, "/v1/search",
            json!({"query":"summary","mode":"hybrid","limit":100,"components":{"summaries":summaries,"graph":false}})).await;
        let hit = selected["hits"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["asset_id"] == docs[0]["asset_id"])
            .unwrap();
        summary_scores.push(hit["score"].as_f64().unwrap());
    }
    assert!(summary_scores[1] > summary_scores[0]);
    assert!(
        graph["graph"]["entities"][0]["evidence"]
            .as_array()
            .unwrap()
            .len()
            >= 2
    );
    let deleted = request(
        &http,
        &base,
        token,
        reqwest::Method::DELETE,
        &format!("/v1/assets/{}", docs[0]["asset_id"].as_str().unwrap()),
        Value::Null,
    )
    .await;
    job(
        &http,
        &base,
        token,
        id(&deleted, "cleanup_job_id"),
        "completed",
    )
    .await;
    let graph = post(
        &http,
        &base,
        token,
        "/v1/search",
        json!({"query":"Atlas","mode":"hybrid"}),
    )
    .await;
    assert_eq!(graph["graph"]["entities"].as_array().unwrap().len(), 2);
    for e in graph["graph"]["entities"].as_array().unwrap() {
        assert!(
            e["evidence"]
                .as_array()
                .unwrap()
                .iter()
                .all(|e| e["asset_id"] != docs[0]["asset_id"])
        );
    }
    let context = post(
        &http,
        &base,
        token,
        "/v1/resolve",
        json!({"query":"Atlas","mode":"hybrid","budget_tokens":3000}),
    )
    .await;
    assert!(context["count"].as_u64().unwrap() <= 3000);
    assert!(!context["sources"].as_array().unwrap().is_empty());
    // The library parser runs inside the host; preserve raw uploads and page provenance.
    let bytes = pdf();
    let response = http
        .post(format!("{base}/v1/files?name=evidence.pdf&format=pdf"))
        .bearer_auth(token)
        .header("Idempotency-Key", "pdf-upload")
        .body(bytes.clone())
        .send()
        .await
        .unwrap();
    let status = response.status();
    let file = response.json::<Value>().await.unwrap();
    assert!(status.is_success(), "{file}");
    let document = post(
        &http,
        &base,
        token,
        "/v1/knowledge",
        json!({"title":"PDF evidence","file_id":file["file_id"],"format":"pdf"}),
    )
    .await;
    job(&http, &base, token, id(&document, "job_id"), "completed").await;
    let fetched = http
        .get(format!(
            "{base}/v1/files/{}",
            file["file_id"].as_str().unwrap()
        ))
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(fetched.as_ref(), bytes.as_slice());
    for (query, page) in [("approval", 1), ("provenance", 2)] {
        let results = post(
            &http,
            &base,
            token,
            "/v1/search",
            json!({"query":query,"mode":"keyword"}),
        )
        .await;
        let hit = results["hits"]
            .as_array()
            .unwrap()
            .iter()
            .find(|hit| hit["asset_id"] == document["asset_id"])
            .expect("PDF evidence must be searchable");
        assert_eq!(hit["locator"]["page"], page);
        assert_eq!(hit["locator"]["parser"], "pdf-oxide-0.3.78-v1");
        assert_eq!(hit["locator"]["byte_start"], 0);
    }
    let invalid_file = http
        .post(format!("{base}/v1/files?name=invalid.pdf&format=pdf"))
        .bearer_auth(token)
        .header("Idempotency-Key", "invalid-pdf-upload")
        .body("%PDF-1.4\ntruncated")
        .send()
        .await
        .unwrap();
    assert!(invalid_file.status().is_success());
    let invalid_file: Value = invalid_file.json().await.unwrap();
    let invalid_document = post(
        &http,
        &base,
        token,
        "/v1/knowledge",
        json!({"title":"Invalid PDF","file_id":invalid_file["file_id"],"format":"pdf"}),
    )
    .await;
    let failed = job(
        &http,
        &base,
        token,
        id(&invalid_document, "job_id"),
        "failed",
    )
    .await;
    assert_eq!(failed["error_code"], "INVALID_ARGUMENT");
    assert_eq!(
        http.get(format!(
            "{base}/v1/assets/{}",
            invalid_document["asset_id"].as_str().unwrap()
        ))
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .status(),
        404
    );
    // Cancel while model IO is in flight, then retry with a new generation.
    slow.store(true, Ordering::SeqCst);
    let pending = post(
        &http,
        &base,
        token,
        "/v1/memories",
        json!({"fact_key":"cancel","content":"Cancelled evidence","publish_if_authorized":true}),
    )
    .await;
    job(&http, &base, token, id(&pending, "job_id"), "processing").await;
    post(
        &http,
        &base,
        token,
        &format!("/v1/jobs/{}/cancel", pending["job_id"].as_str().unwrap()),
        json!({}),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(2300)).await;
    assert_eq!(
        http.get(format!(
            "{base}/v1/assets/{}",
            pending["asset_id"].as_str().unwrap()
        ))
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .status(),
        404
    );
    slow.store(false, Ordering::SeqCst);
    post(
        &http,
        &base,
        token,
        &format!("/v1/jobs/{}/retry", pending["job_id"].as_str().unwrap()),
        json!({}),
    )
    .await;
    let retried = job(&http, &base, token, id(&pending, "job_id"), "completed").await;
    assert_eq!(retried["generation"], 3);

    // Revoke the creator during model IO: the final write must use fresh permission.
    slow.store(true, Ordering::SeqCst);
    let actor = post(
        &http,
        &base,
        token,
        "/admin/keys",
        json!({"workspace_id":bootstrap["workspace_id"],"role":"writer"}),
    )
    .await;
    let revoked=post(&http,&base,actor["token"].as_str().unwrap(),"/v1/memories",json!({"fact_key":"revoked","content":"Never publish revoked work","publish_if_authorized":true})).await;
    job(&http, &base, token, id(&revoked, "job_id"), "processing").await;
    request(
        &http,
        &base,
        token,
        reqwest::Method::DELETE,
        &format!("/admin/keys/{}", actor["key_id"].as_str().unwrap()),
        Value::Null,
    )
    .await;
    let failed = job(&http, &base, token, id(&revoked, "job_id"), "failed").await;
    assert_eq!(failed["error_code"], "FORBIDDEN");
    assert_eq!(
        http.get(format!(
            "{base}/v1/assets/{}",
            revoked["asset_id"].as_str().unwrap()
        ))
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .status(),
        404
    );
    // Delete in flight: the persisted cancellation and tombstone block late results.
    let doomed = post(
        &http,
        &base,
        token,
        "/v1/memories",
        json!({"fact_key":"doomed","content":"Deleted work evidence","publish_if_authorized":true}),
    )
    .await;
    job(&http, &base, token, id(&doomed, "job_id"), "processing").await;
    let cleanup = request(
        &http,
        &base,
        token,
        reqwest::Method::DELETE,
        &format!("/v1/assets/{}", doomed["asset_id"].as_str().unwrap()),
        Value::Null,
    )
    .await;
    job(
        &http,
        &base,
        token,
        id(&cleanup, "cleanup_job_id"),
        "completed",
    )
    .await;
    let hidden = post(
        &http,
        &base,
        token,
        "/v1/search",
        json!({"query":"Deleted","mode":"hybrid"}),
    )
    .await;
    assert!(
        hidden["hits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|h| h["asset_id"] != doomed["asset_id"])
    );
    // Hard-kill after claim; exclusive startup immediately recovers the durable job.
    slow.store(true, Ordering::SeqCst);
    let crashed = post(
        &http,
        &base,
        token,
        "/v1/memories",
        json!({"fact_key":"crash","content":"Restart evidence","publish_if_authorized":true}),
    )
    .await;
    job(&http, &base, token, id(&crashed, "job_id"), "processing").await;
    stop(&mut host).await;
    slow.store(false, Ordering::SeqCst);
    let (mut host, base) = start(dir.path(), &model).await;
    let recovered = job(&http, &base, token, id(&crashed, "job_id"), "completed").await;
    assert_eq!(recovered["result"]["version"], 1);
    // The running host exposes tools and reauthorizes each request.
    let key = post(
        &http,
        &base,
        token,
        "/admin/keys",
        json!({"workspace_id":bootstrap["workspace_id"],"role":"reader"}),
    )
    .await;
    let reader = key["token"].as_str().unwrap();
    let peer = crate::mcp_http::Peer::at(format!("{base}/mcp"));
    let tools: Value = peer
        .post(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .bearer_auth(reader)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let tool_names: std::collections::BTreeSet<_> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        tool_names,
        std::collections::BTreeSet::from([
            "context_run_create",
            "context_run_prepare",
            "context_run_report",
            "context_run_get",
            "context_bundle",
            "context_get",
            "context_memory_recall",
            "context_memory_retain",
            "context_resolve",
            "context_search",
            "context_session",
            "context_turn_archive",
            "context_turn_get",
        ])
    );

    let recalled = peer
        .call(
            reader,
            None,
            "context_search",
            json!({"query":"Atlas","mode":"hybrid"}),
        )
        .await
        .unwrap();
    assert_eq!(recalled["graph"]["entities"].as_array().unwrap().len(), 2);
    let assembled = peer
        .call(
            reader,
            None,
            "context_resolve",
            json!({"query":"Atlas","mode":"hybrid","budget_tokens":12000}),
        )
        .await
        .unwrap();
    assert!(assembled["count"].as_u64().unwrap() <= 12000);
    assert!(
        assembled["sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s.get("entity_id").is_some() && !s["evidence"].as_array().unwrap().is_empty())
    );
    let context_request = json!({"query":"Atlas", "mode":"keyword", "budget":32000,
        "routes":[{"route":"memory","limit":20,"max_bytes":32000},{"route":"knowledge","limit":20,"max_bytes":32000}]});
    let direct_bundle: Value = http
        .post(format!("{base}/v1/context"))
        .bearer_auth(key["token"].as_str().unwrap())
        .json(&context_request)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let via_mcp = peer
        .call(reader, None, "context_bundle", context_request.clone())
        .await
        .unwrap();
    assert_eq!(
        crate::support::without_retrieval_observations(&direct_bundle),
        crate::support::without_retrieval_observations(&via_mcp)
    );
    assert!(!via_mcp["items"].as_array().unwrap().is_empty());
    let get = json!({"asset_id":published["asset_id"]});
    let result = peer
        .call(reader, None, "context_get", get.clone())
        .await
        .unwrap();
    assert!(
        result["content"]
            .as_str()
            .unwrap()
            .contains("Release approval required")
    );
    request(
        &http,
        &base,
        token,
        reqwest::Method::DELETE,
        &format!("/admin/keys/{}", key["key_id"].as_str().unwrap()),
        Value::Null,
    )
    .await;
    assert_eq!(
        peer.call(reader, None, "context_get", get).await,
        Err(json!({"http_status":401}))
    );
    assert_eq!(
        peer.call(reader, None, "context_bundle", context_request)
            .await,
        Err(json!({"http_status":401}))
    );
    stop(&mut host).await;
    // No silent offline fallback when host is gone.
    let unavailable = command(dir.path())
        .env("OC_API_KEY", token)
        .env("OC_SERVER_URL", &base)
        .args(["search", "approval"])
        .output()
        .await
        .unwrap();
    assert!(!unavailable.status.success());
    let mut offline_command = command(dir.path());
    crate::support::command_env(&mut offline_command, dir.path(), Some(3));
    let offline = offline_command
        .env("OC_ENABLE_MODELS", "true")
        .env("OC_MODEL_BASE_URL", &model)
        .env("OC_MODEL_API_KEY", "stub")
        .env("OC_EMBEDDING_MODEL", "stub")
        .env("OC_EMBEDDING_DIMENSION", "3")
        .env("OC_EXTRACTION_MODEL", "stub")
        .env("OC_API_KEY", token)
        .args(["--offline", "search", "approval"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .unwrap();
    assert!(
        offline.status.success(),
        "offline search after host termination: status={:?}, code={:?}, stdout={}, stderr={}",
        offline.status,
        offline.status.code(),
        String::from_utf8_lossy(&offline.stdout),
        String::from_utf8_lossy(&offline.stderr)
    );
    mock.abort();
}
fn post_route() -> axum::routing::MethodRouter<Arc<AtomicBool>> {
    axum::routing::post(embed)
}

#[tokio::test]
async fn identity_filter_survives_top_100_distractors_and_stale_versions() {
    let dir = tempfile::tempdir().unwrap();
    let service = crate::support::open_service(dir.path(), Models::disabled())
        .await
        .unwrap();
    let store = service.engine.relational();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "candidate-filter")
        .await
        .unwrap();
    let admin = store.issue_key(scope, "admin").await.unwrap();
    let reader = store.issue_key(scope, "reader").await.unwrap();
    let reader_auth = service.auth(&reader.token).await.unwrap();
    let identity: origence::memory_identity::MemoryIdentity = serde_json::from_value(json!({
        "subject": {"kind": "service", "stable_id": "target"},
        "predicate": "release.approval",
        "context": {"environment": "production"}
    }))
    .unwrap();

    // 130 stronger keyword matches would previously fill the global top-100.
    let mut tx = store
        .begin(store.authenticate(&admin.token).await.unwrap())
        .await
        .unwrap();
    let mut full_text = Vec::new();
    for i in 0..130 {
        let (asset, _) = tx.slot(&format!("decoy-{i}")).await.unwrap();
        let source = tx.create_event("memory", "approval", None).await.unwrap();
        tx.insert_version(asset, 1, "approval", source, None, None)
            .await
            .unwrap();
        let chunk = Uuid::new_v4();
        tx.insert_chunk(chunk, asset, 1, 0, "approval", &json!({}))
            .await
            .unwrap();
        tx.update_asset_version(asset, 1, None).await.unwrap();
        full_text.push(FullTextEntry {
            id: chunk,
            source: SourceVersion {
                source_id: source,
                version: 1,
            },
            generation: 1,
            search_terms: "approval".into(),
            payload: origence::storage::ChunkPayload {
                content: "approval".into(),
                locator: json!({}),
                summary: None,
            },
        });
    }
    let (target, _) = tx.identity_slot(&identity).await.unwrap();
    let mut current_source = Uuid::nil();
    for version in [1, 2] {
        let source = tx
            .create_event("memory", "approval evidence", None)
            .await
            .unwrap();
        tx.insert_version(target, version, "approval evidence", source, None, None)
            .await
            .unwrap();
        let chunk = Uuid::new_v4();
        tx.insert_chunk(
            chunk,
            target,
            version,
            0,
            if version == 1 {
                "approval old"
            } else {
                "approval current"
            },
            &json!({}),
        )
        .await
        .unwrap();
        tx.update_asset_version(target, version, None)
            .await
            .unwrap();
        current_source = source;
        full_text.push(FullTextEntry {
            id: chunk,
            source: SourceVersion {
                source_id: source,
                version,
            },
            generation: 1,
            search_terms: "approval evidence supplemental".into(),
            payload: origence::storage::ChunkPayload {
                content: if version == 1 {
                    "approval old"
                } else {
                    "approval current"
                }
                .into(),
                locator: json!({}),
                summary: None,
            },
        });
    }
    // This direct authority fixture must publish the same native surface as
    // the worker; authority chunk terms alone are no longer a keyword index.
    let ledger_key = |entry: &FullTextEntry| LedgerKey {
        scope,
        source: entry.source,
        artifact_id: entry.id,
        surface: Surface::FullText,
        generation: entry.generation,
    };
    for entry in &full_text {
        let key = ledger_key(entry);
        tx.register_pending(LedgerEntry {
            key,
            artifact_type: "chunk".into(),
        })
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
    crate::support::projection(&service)
        .upsert_full_text(scope, full_text.clone())
        .await
        .unwrap();
    let mut tx = store
        .begin(store.authenticate(&admin.token).await.unwrap())
        .await
        .unwrap();
    for entry in &full_text {
        tx.confirm_committed(ledger_key(entry)).await.unwrap();
    }
    tx.commit().await.unwrap();
    // Establish strict BM25 strength, without imposing any choice among the
    // 130 tied decoys at the native cutoff. The old target version stays native.
    let strength = crate::support::projection(&service)
        .search_full_text(FullTextQuery {
            scope,
            artifact_ids: Some(vec![full_text[0].id, full_text.last().unwrap().id]),
            generation: 1,
            search_terms: "approval".into(),
            limit: 2,
        })
        .await
        .unwrap();
    assert_eq!(strength.len(), 2);
    assert_eq!(strength[0].id, full_text[0].id);
    assert!(strength[0].score > strength[1].score);

    let search = |filter| SearchInput {
        components: Default::default(),
        memory_identity: filter,
        query: "approval".into(),
        mode: "keyword".into(),
        limit: 100,
        allow_partial: false,
    };
    let plain = service.search(&reader_auth, search(None)).await.unwrap();
    assert_eq!(plain["hits"].as_array().unwrap().len(), 100);
    assert!(
        plain["hits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|hit| hit["asset_id"] != json!(target))
    );
    let filtered = service
        .search(&reader_auth, search(Some(identity.clone())))
        .await
        .unwrap();
    assert_eq!(filtered["hits"].as_array().unwrap().len(), 1);
    assert_eq!(filtered["hits"][0]["asset_id"], json!(target));
    assert_eq!(filtered["hits"][0]["version"], 2);
    assert_eq!(filtered["hits"][0]["content"], "approval current");
    assert_eq!(filtered["hits"][0]["identity"], json!(identity));

    let context = service
        .resolve(
            &reader_auth,
            ResolveInput {
                components: Default::default(),
                query: "approval".into(),
                memory_identity: Some(identity.clone()),
                mode: "keyword".into(),
                budget_tokens: 3000,
                allow_partial: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(context["sources"].as_array().unwrap().len(), 1);
    assert_eq!(context["sources"][0]["asset_id"], json!(target));
    assert!(
        context["rendered_context"]
            .as_str()
            .unwrap()
            .contains("approval current")
    );

    let mut unknown = identity.clone();
    unknown.subject.stable_id = "unknown".into();
    assert_eq!(
        service
            .search(&reader_auth, search(Some(unknown)))
            .await
            .unwrap()["hits"],
        json!([])
    );

    let other_scope = Scope {
        tenant_id: scope.tenant_id,
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(other_scope.tenant_id, other_scope.workspace_id, "other")
        .await
        .unwrap();
    let other_key = store.issue_key(other_scope, "reader").await.unwrap();
    let other_auth = service.auth(&other_key.token).await.unwrap();
    assert_eq!(
        service
            .search(&other_auth, search(Some(identity.clone())))
            .await
            .unwrap()["hits"],
        json!([])
    );

    let mut tx = store
        .begin(store.authenticate(&admin.token).await.unwrap())
        .await
        .unwrap();
    tx.retract_event(current_source).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        service
            .search(&reader_auth, search(Some(identity)))
            .await
            .unwrap()["hits"],
        json!([])
    );
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn local_transactions_versions_retraction_and_idempotency() {
    let dir = tempfile::tempdir().unwrap();
    let s = crate::support::open_service(dir.path(), Models::disabled())
        .await
        .unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    let store = s.engine.relational();
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "local")
        .await
        .unwrap();
    let key = store.issue_key(scope, "admin").await.unwrap();
    let a = s.auth(&key.token).await.unwrap();

    let writer_key = store.issue_key(scope, "writer").await.unwrap();
    let writer = s.auth(&writer_key.token).await.unwrap();
    let proposal = s
        .memory(
            &writer,
            "writer",
            MemoryInput {
                fact_key: "writer".into(),
                content: "Writer proposal".into(),
                publish_if_authorized: true,
            },
        )
        .await
        .unwrap();
    assert!(proposal["job_id"].is_string());
    assert_eq!(proposal["state"], "accepted");
    let reader_key = store.issue_key(scope, "reader").await.unwrap();
    let reader = s.auth(&reader_key.token).await.unwrap();
    // Job state can expose unpublished input, so it stays behind write permission.
    assert!(s.job(&reader, id(&proposal, "job_id")).await.is_err());
    assert!(origence::worker::process_next(&s).await.unwrap());
    assert_eq!(
        s.get(&writer, id(&proposal, "asset_id"), None)
            .await
            .unwrap()["version"],
        1
    );
    let chinese = s
        .knowledge(
            &a,
            "zh",
            KnowledgeInput {
                title: "policy".into(),
                content: Some("生产发布必须经过审批".into()),
                file_id: None,
                format: "text".into(),
                asset_id: None,
                expected_version: None,
            },
        )
        .await
        .unwrap();
    assert!(origence::worker::process_next(&s).await.unwrap());
    let zh = s
        .search(
            &a,
            SearchInput {
                components: Default::default(),
                memory_identity: None,
                query: "发布审批".into(),
                limit: 10,
                mode: "keyword".into(),
                allow_partial: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(zh["hits"][0]["asset_id"], chinese["asset_id"]);
    let input = MemoryInput {
        fact_key: "preference".into(),
        content: "Release approval required".into(),
        publish_if_authorized: true,
    };
    let first = s.memory(&a, "once", input.clone()).await.unwrap();
    assert_eq!(first, s.memory(&a, "once", input.clone()).await.unwrap());
    assert_eq!(first["conflict"], false);
    assert!(first["source_event_id"].is_string());
    let mut different = input.clone();
    different.content = "different".into();
    assert!(s.memory(&a, "once", different).await.is_err());
    assert!(origence::worker::process_next(&s).await.unwrap());
    assert_eq!(
        s.get(&a, id(&first, "asset_id"), None).await.unwrap()["version"],
        1
    );
    let update = s
        .memory(
            &a,
            "update",
            MemoryInput {
                content: "Release review required".into(),
                ..input
            },
        )
        .await
        .unwrap();
    assert!(update["job_id"].is_string());
    assert_eq!(update["conflict"], true);
    assert!(origence::worker::process_next(&s).await.unwrap());
    assert_eq!(
        s.get(&a, id(&first, "asset_id"), None).await.unwrap()["version"],
        2
    );
    let restore_key = s.issue_key(&a, "writer").await.unwrap();
    let restore_writer = s
        .auth(restore_key["token"].as_str().unwrap())
        .await
        .unwrap();
    let restored = s
        .restore(
            &restore_writer,
            "restore",
            id(&first, "asset_id"),
            RestoreInput {
                target_version: 1,
                expected_version: 2,
                reason: "rollback".into(),
            },
        )
        .await
        .unwrap();
    assert!(origence::worker::process_next(&s).await.unwrap());
    assert_eq!(
        s.job(&a, id(&restored, "job_id")).await.unwrap()["result"]["version"],
        3
    );
    let deleted = s
        .delete(&a, "retract", id(&first, "source_event_id"), "event")
        .await
        .unwrap();
    assert!(s.get(&a, id(&first, "asset_id"), None).await.is_err());
    assert!(origence::worker::process_next(&s).await.unwrap());
    assert_eq!(
        s.job(&a, id(&deleted, "cleanup_job_id")).await.unwrap()["state"],
        "completed"
    );
    // A still-valid historical version remains available, while retracted source versions do not.
    assert!(s.get(&a, id(&first, "asset_id"), Some(2)).await.is_ok());
    assert!(s.get(&a, id(&first, "asset_id"), Some(1)).await.is_err());
    let result = s
        .search(
            &a,
            SearchInput {
                components: Default::default(),
                memory_identity: None,
                query: "Release".into(),
                limit: 10,
                mode: "keyword".into(),
                allow_partial: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(result["hits"], json!([]));
    s.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn racing_expected_versions_supersede_the_loser() {
    let dir = tempfile::tempdir().unwrap();
    let s = crate::support::open_service(dir.path(), Models::disabled())
        .await
        .unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    let store = s.engine.relational();
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "local")
        .await
        .unwrap();
    let key = store.issue_key(scope, "writer").await.unwrap();
    let a = s.auth(&key.token).await.unwrap();
    let input = MemoryInput {
        fact_key: "racer".into(),
        content: "Race base".into(),
        publish_if_authorized: true,
    };
    let base = s.memory(&a, "base", input.clone()).await.unwrap();
    assert!(origence::worker::process_next(&s).await.unwrap());
    assert_eq!(
        s.get(&a, id(&base, "asset_id"), None).await.unwrap()["version"],
        1
    );
    // Both updates accept against the same current version; the first commit
    // passes the recheck, the loser settles superseded without a new version.
    let left = s
        .memory(
            &a,
            "left",
            MemoryInput {
                content: "Race left".into(),
                ..input.clone()
            },
        )
        .await
        .unwrap();
    let right = s
        .memory(
            &a,
            "right",
            MemoryInput {
                content: "Race right".into(),
                ..input.clone()
            },
        )
        .await
        .unwrap();
    assert_eq!(left["conflict"], true);
    assert_eq!(right["conflict"], true);
    assert!(origence::worker::process_next(&s).await.unwrap());
    assert!(origence::worker::process_next(&s).await.unwrap());
    let left_state = s.job(&a, id(&left, "job_id")).await.unwrap()["state"].clone();
    let right_state = s.job(&a, id(&right, "job_id")).await.unwrap()["state"].clone();
    // Queue order between same-instant jobs is not guaranteed; assert the
    // outcome pair, not which one won.
    let states = [left_state, right_state];
    assert!(states.contains(&json!("completed")));
    assert!(states.contains(&json!("superseded")));
    assert_eq!(
        s.get(&a, id(&base, "asset_id"), None).await.unwrap()["version"],
        2
    );
    s.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn malformed_durable_jobs_settle_and_leave_the_worker_available() {
    let dir = tempfile::tempdir().unwrap();
    let s = crate::support::open_service(dir.path(), Models::disabled())
        .await
        .unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    let store = s.engine.relational();
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "malformed jobs")
        .await
        .unwrap();
    let key = store.issue_key(scope, "admin").await.unwrap();
    let a = s.auth(&key.token).await.unwrap();
    let cases = [
        (
            "unknown operation",
            "unknown_operation",
            json!({"schema_version":1,"expected_version":null,"embedding_profile":null}),
        ),
        (
            "unsupported schema",
            "publish",
            json!({"schema_version":2,"expected_version":null,"embedding_profile":null}),
        ),
        (
            "missing schema",
            "publish",
            json!({"expected_version":null,"embedding_profile":null}),
        ),
        ("non-object payload", "publish", Value::Null),
        (
            "missing ingest format",
            "ingest",
            json!({"schema_version":1,"expected_version":null,"embedding_profile":null}),
        ),
        (
            "malformed ingest format",
            "ingest",
            json!({"schema_version":1,"format":7,"expected_version":null,"embedding_profile":null}),
        ),
        (
            "malformed restore version",
            "restore",
            json!({"schema_version":1,"target_version":"invalid","expected_version":null,"embedding_profile":null}),
        ),
        (
            "present null identity",
            "extract",
            json!({"schema_version":1,"identity":null,"expected_version":null,"embedding_profile":null}),
        ),
    ];
    for (case, operation, payload) in cases {
        let malformed = s
            .memory(
                &a,
                &format!("malformed {case}"),
                MemoryInput {
                    fact_key: format!("malformed {case}"),
                    content: "Malformed job source stays available".into(),
                    publish_if_authorized: true,
                },
            )
            .await
            .unwrap();
        let malformed_id = id(&malformed, "job_id");
        // Bypass admission validation only in this durable-queue fixture. Keep
        // real authorization, source and asset bindings, and force queue order.
        db::query("UPDATE oc_jobs SET operation=?,payload=?,created_at=0 WHERE id=?")
            .bind(operation)
            .bind(payload.to_string())
            .bind(malformed_id)
            .execute(store.pool())
            .await
            .unwrap();
        let valid = s
            .memory(
                &a,
                &format!("valid after {case}"),
                MemoryInput {
                    fact_key: format!("valid after {case}"),
                    content: "The following valid job publishes".into(),
                    publish_if_authorized: true,
                },
            )
            .await
            .unwrap();
        let valid_id = id(&valid, "job_id");

        assert!(origence::worker::process_next(&s).await.unwrap(), "{case}");
        let failed = s.job(&a, malformed_id).await.unwrap();
        assert_eq!(failed["state"], "failed", "{case}: {failed}");
        assert_eq!(failed["error_code"], "INVALID_ARGUMENT", "{case}");
        assert_eq!(s.job(&a, valid_id).await.unwrap()["state"], "pending");
        assert!(s.get(&a, id(&malformed, "asset_id"), None).await.is_err());
        assert_eq!(
            s.source_event(&a, id(&malformed, "source_event_id"))
                .await
                .unwrap()["content"],
            "Malformed job source stays available"
        );

        assert!(origence::worker::process_next(&s).await.unwrap(), "{case}");
        assert_eq!(s.job(&a, valid_id).await.unwrap()["state"], "completed");
        assert_eq!(
            s.get(&a, id(&valid, "asset_id"), None).await.unwrap()["version"],
            1
        );
        assert!(!origence::worker::process_next(&s).await.unwrap(), "{case}");
    }
    s.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn saved_publication_checks_generation_before_decoding() {
    let dir = tempfile::tempdir().unwrap();
    let s = crate::support::open_service(dir.path(), Models::disabled())
        .await
        .unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    let store = s.engine.relational();
    store
        .create_workspace(
            scope.tenant_id,
            scope.workspace_id,
            "publication generation",
        )
        .await
        .unwrap();
    let key = store.issue_key(scope, "admin").await.unwrap();
    let a = s.auth(&key.token).await.unwrap();
    for (case, publication, state) in [
        (
            "stale generation",
            json!({"generation":0,"memories":"malformed"}),
            "completed",
        ),
        (
            "missing generation",
            json!({"memories":"malformed"}),
            "completed",
        ),
        (
            "wrong generation type",
            json!({"generation":"1","memories":"malformed"}),
            "completed",
        ),
        (
            "current generation",
            json!({"generation":1,"memories":"malformed"}),
            "failed",
        ),
    ] {
        let accepted = s
            .memory(
                &a,
                case,
                MemoryInput {
                    fact_key: case.into(),
                    content: "Fresh source evidence".into(),
                    publish_if_authorized: true,
                },
            )
            .await
            .unwrap();
        let job_id = id(&accepted, "job_id");
        // An obsolete saved plan must be ignored before its body is decoded.
        // The same malformed body is an input error when the generation matches.
        db::query(
            "UPDATE oc_jobs SET payload=json_set(payload,'$.publication',json(?)) WHERE id=?",
        )
        .bind(publication.to_string())
        .bind(job_id)
        .execute(store.pool())
        .await
        .unwrap();
        assert!(origence::worker::process_next(&s).await.unwrap(), "{case}");
        let settled = s.job(&a, job_id).await.unwrap();
        assert_eq!(settled["state"], state, "{case}: {settled}");
        if state == "completed" {
            assert_eq!(
                s.get(&a, id(&accepted, "asset_id"), None).await.unwrap()["version"],
                1
            );
        } else {
            assert_eq!(settled["error_code"], "INVALID_ARGUMENT");
            assert!(s.get(&a, id(&accepted, "asset_id"), None).await.is_err());
        }
        assert!(!origence::worker::process_next(&s).await.unwrap(), "{case}");
    }
    s.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn saved_publication_recovers_after_external_anyseek_graph_write() {
    let dir = tempfile::tempdir().unwrap();
    let s = crate::support::open_service(dir.path(), Models::disabled())
        .await
        .unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    s.engine
        .relational()
        .create_workspace(scope.tenant_id, scope.workspace_id, "replay")
        .await
        .unwrap();
    let key = s
        .engine
        .relational()
        .issue_key(scope, "admin")
        .await
        .unwrap();
    let a = s.auth(&key.token).await.unwrap();
    let accepted = s
        .knowledge(
            &a,
            "ingest",
            KnowledgeInput {
                title: "Atlas".into(),
                content: Some("Atlas evidence".into()),
                file_id: None,
                format: "text".into(),
                asset_id: None,
                expected_version: None,
            },
        )
        .await
        .unwrap();
    let claim = s.engine.queue().claim_next().await.unwrap().unwrap();
    let source = SourceVersion {
        source_id: claim.source.unwrap(),
        version: 1,
    };
    let entity = Entity {
        name: "Atlas".into(),
        entity_type: "service".into(),
        description: "evidence".into(),
    };
    let entity_id = origence::graph::entity_id("Atlas");
    let chunk_id = Uuid::new_v4();
    let ledger_key = LedgerKey {
        scope,
        source,
        artifact_id: entity_id,
        surface: Surface::Graph,
        generation: claim.generation,
    };
    let auth = s
        .engine
        .relational()
        .authenticate(&key.token)
        .await
        .unwrap();
    let epoch = origence::storage::anyseek::operation_epoch().unwrap();
    let native = origence::storage::anyseek::AnySeekFullTextStore::new(
        crate::support::fixture(dir.path(), None).config(),
    )
    .unwrap();
    let mut tx = s.engine.relational().begin(auth).await.unwrap();
    tx.check_permission(Permission::Write).await.unwrap();
    tx.save_publication(claim.job_id,&json!({"generation":claim.generation,"full_text_operation_epoch":epoch,"profile":null,"memories":[{"asset":claim.asset,"fact_key":null,"expected_version":null,"version":1,"chunks":[{"id":chunk_id,"chunk":{"content":"Atlas evidence","locator":{"start":0,"end":14}},"embedding":null,"summary":"","search_terms":"atlas evidence"}]}],"graph":{"entities":[entity],"relations":[]}})).await.unwrap();
    tx.register_pending(LedgerEntry {
        key: ledger_key,
        artifact_type: "entity".into(),
    })
    .await
    .unwrap();
    let chunk_pending = LedgerKey {
        artifact_id: chunk_id,
        surface: Surface::FullText,
        ..ledger_key
    };
    tx.register_pending(LedgerEntry {
        key: chunk_pending,
        artifact_type: "chunk".into(),
    })
    .await
    .unwrap();
    tx.commit().await.unwrap();
    native
        .upsert_with_graph(
            scope,
            vec![FullTextEntry {
                id: chunk_id,
                source,
                generation: claim.generation,
                search_terms: origence::parsing::lexical("Atlas evidence"),
                payload: origence::storage::ChunkPayload {
                    content: "Atlas evidence".into(),
                    locator: json!({"start":0,"end":14}),
                    summary: None,
                },
            }],
            vec![],
            Some(origence::storage::anyseek::GraphPublication {
                source,
                generation: claim.generation,
                graph: GraphExtraction {
                    entities: vec![entity],
                    relations: vec![],
                },
            }),
            epoch,
        )
        .await
        .unwrap();
    assert_eq!(
        native
            .graph_seed_candidates(scope, "atlas")
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(s.get(&a, id(&accepted, "asset_id"), None).await.is_err());
    // Crash boundary: AnySeek owns the frozen group; SQLite has pending intent only.
    s.engine.shutdown().await.unwrap();
    drop(s);
    let s = crate::support::open_service(dir.path(), Models::disabled())
        .await
        .unwrap();
    assert!(s.get(&a, id(&accepted, "asset_id"), None).await.is_err());
    assert!(origence::worker::process_next(&s).await.unwrap());
    assert!(!origence::worker::process_next(&s).await.unwrap());
    assert_eq!(
        s.get(&a, id(&accepted, "asset_id"), None).await.unwrap()["version"],
        1
    );
    assert_eq!(
        native
            .graph_seed_candidates(scope, "atlas")
            .await
            .unwrap()
            .len(),
        1
    );
    let versions: i64 = db::query_scalar("SELECT count(*) FROM oc_versions")
        .fetch_one(s.engine.relational().pool())
        .await
        .unwrap();
    assert_eq!(versions, 1);
    let publications: Vec<(String, Uuid, Uuid, i32, i64, String)> = db::query_as(
        "SELECT surface,artifact_id,source_id,version,generation,state FROM oc_artifact_ledger ORDER BY surface",
    )
    .fetch_all(s.engine.relational().pool())
    .await
    .unwrap();
    assert_eq!(
        publications,
        vec![
            (
                "full_text".into(),
                chunk_id,
                source.source_id,
                source.version,
                claim.generation,
                "committed".into(),
            ),
            (
                "graph".into(),
                entity_id,
                source.source_id,
                source.version,
                claim.generation,
                "committed".into(),
            ),
        ]
    );

    // Put a real vector behind the same published chunk to check visibility before cleanup.
    let vk = LedgerKey {
        artifact_id: chunk_id,
        surface: Surface::Vector,
        ..ledger_key
    };
    let auth = s
        .engine
        .relational()
        .authenticate(&key.token)
        .await
        .unwrap();
    let mut tx = s.engine.relational().begin(auth.clone()).await.unwrap();
    tx.register_pending(LedgerEntry {
        key: vk,
        artifact_type: "chunk".into(),
    })
    .await
    .unwrap();
    tx.commit().await.unwrap();
    crate::support::projection(&s)
        .upsert(
            scope,
            vec![VectorEntry {
                id: chunk_id,
                embedding: vec![1.0, 0.0],
                profile: "test".into(),
                dimension: 2,
                generation: 1,
                source,
            }],
        )
        .await
        .unwrap();
    let mut tx = s.engine.relational().begin(auth.clone()).await.unwrap();
    tx.index_ready(chunk_id, "test", 2, 1).await.unwrap();
    tx.confirm_committed(vk).await.unwrap();
    tx.commit().await.unwrap();
    let q = VectorQuery {
        scope,
        artifact_ids: None,
        profile: "test".into(),
        dimension: 2,
        generation: 1,
        embedding: vec![1.0, 0.0],
        limit: 10,
    };
    assert_eq!(
        crate::support::projection(&s)
            .search(q.clone())
            .await
            .unwrap()
            .len(),
        1
    );
    s.delete(&a, "delete", source.source_id, "event")
        .await
        .unwrap();
    // No worker is running: stale native records exist but cannot become public evidence.
    assert_eq!(
        native
            .graph_seed_candidates(scope, "atlas")
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        crate::support::projection(&s)
            .search(q.clone())
            .await
            .unwrap()
            .len(),
        1
    );
    let mut tx = s.engine.relational().begin_read(auth).await.unwrap();
    assert!(tx.graph_evidence(entity_id).await.unwrap().is_empty());
    assert!(
        tx.vector_hit(chunk_id, source, "test", 1)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        tx.vector_candidates("test", 2, 1, None)
            .await
            .unwrap()
            .is_empty()
    );
    tx.commit().await.unwrap();
    assert!(origence::worker::process_next(&s).await.unwrap());
    assert!(
        native
            .graph_seed_candidates(scope, "atlas")
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        crate::support::projection(&s)
            .search(q)
            .await
            .unwrap()
            .is_empty()
    );
    s.engine.shutdown().await.unwrap();
    assert!(!dir.path().join("graph.db").exists());
}

#[tokio::test]
async fn graceful_host_shutdown_stops_worker_and_closes_store() {
    let dir = tempfile::tempdir().unwrap();
    let s = crate::support::open_service(dir.path(), Models::disabled())
        .await
        .unwrap();
    let view = s.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let host = tokio::spawn(origence::host::serve(s, listener, async {
        let _ = rx.await;
    }));
    tokio::time::timeout(Duration::from_secs(10), async {
        while !view.ready().await {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(10), host)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(!view.ready().await);
    assert!(!view.worker_running.load(Ordering::Acquire));
}

#[tokio::test]
async fn offline_cli_search_crosses_authority_batch_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let service = crate::support::open_service(dir.path(), Models::disabled())
        .await
        .unwrap();
    let store = service.engine.relational();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "offline-batch")
        .await
        .unwrap();
    let admin = store.issue_key(scope, "admin").await.unwrap();
    let reader = store.issue_key(scope, "reader").await.unwrap();
    // Initialize the workspace through the public publication path before
    // injecting the large authority fixture; this also pins the analysis binding.
    let auth = service.auth(&admin.token).await.unwrap();
    service
        .memory(
            &auth,
            "offline-binding",
            serde_json::from_value(
                json!({"fact_key":"offline.binding", "content":"initialize workspace collection"}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    while origence::worker::process_next(&service).await.unwrap() {}
    let mut tx = store
        .begin(store.authenticate(&admin.token).await.unwrap())
        .await
        .unwrap();
    let mut full_text = Vec::new();
    let mut expected = std::collections::HashMap::new();
    // Distinct assets exercise both the 128-ID and 32-version authority batches.
    // Use the same durable native publication protocol as the worker, without
    // invoking a model or depending on the test executable's CLI behavior.
    for index in 0..130 {
        let (asset, _) = tx.slot(&format!("offline-batch-{index}")).await.unwrap();
        let source = tx.create_event("memory", "approval", None).await.unwrap();
        tx.insert_version(asset, 1, "approval", source, None, None)
            .await
            .unwrap();
        let chunk = Uuid::new_v4();
        tx.insert_chunk(chunk, asset, 1, 0, "approval", &json!({}))
            .await
            .unwrap();
        tx.update_asset_version(asset, 1, None).await.unwrap();
        full_text.push(FullTextEntry {
            id: chunk,
            source: SourceVersion {
                source_id: source,
                version: 1,
            },
            generation: 1,
            search_terms: "approval".into(),
            payload: origence::storage::ChunkPayload {
                content: "approval".into(),
                locator: json!({}),
                summary: None,
            },
        });
        expected.insert(chunk, (asset, source));
    }
    let ledger_key = |entry: &FullTextEntry| LedgerKey {
        scope,
        source: entry.source,
        artifact_id: entry.id,
        surface: Surface::FullText,
        generation: entry.generation,
    };
    for entry in &full_text {
        let key = ledger_key(entry);
        tx.register_pending(LedgerEntry {
            key,
            artifact_type: "chunk".into(),
        })
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
    crate::support::projection(&service)
        .upsert_full_text(scope, full_text.clone())
        .await
        .unwrap();
    let mut tx = store
        .begin(store.authenticate(&admin.token).await.unwrap())
        .await
        .unwrap();
    for entry in &full_text {
        // Match the worker's final publication transaction: startup cleanup
        // retains native records only while their scoped chunk owner is live.
        tx.register_owner(entry.source, "chunk", entry.id, Some(entry.id))
            .await
            .unwrap();
        tx.confirm_committed(ledger_key(entry)).await.unwrap();
    }
    tx.commit().await.unwrap();
    assert_eq!(expected.len(), 130);
    service.engine.shutdown().await.unwrap();
    drop(service);

    // A Rust test thread has a different stack budget. Run the actual binary
    // after releasing every store handle so Windows exercises its default main
    // thread stack through startup, SQL batches, native FTS and final authority.
    let mut offline_command = command(dir.path());
    crate::support::command_env(&mut offline_command, dir.path(), None);
    let output = offline_command
        .env("OC_API_KEY", &reader.token)
        .args(["--offline", "search", "approval", "--limit", "100"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "offline batch search: status={:?}, code={:?}, stdout={}, stderr={}",
        output.status,
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["effective_mode"], "keyword");
    assert_eq!(response["warnings"], json!([]));
    let hits: Vec<SearchHit> = serde_json::from_value(response["hits"].clone()).unwrap();
    assert_eq!(hits.len(), 100);
    let mut seen = std::collections::HashSet::new();
    for hit in hits {
        let (asset, source) = expected.get(&hit.chunk_id).unwrap();
        assert!(seen.insert(hit.chunk_id));
        assert_eq!(hit.asset_id, *asset);
        assert_eq!(hit.source_event_id, *source);
        assert_eq!(hit.version, 1);
        assert_eq!(hit.content, "approval");
        assert!(hit.score.is_finite());
    }
}

/// The public hybrid result preserves directed edges but expands both incident
/// directions exactly once. This protocol-peer test is not backend acceptance.
#[tokio::test]
async fn graph_one_hop_direction_disabled_path_and_withdrawal() {
    use origence::graph::{entity_id, relation_id};
    let dir = tempfile::tempdir().unwrap();
    let out = command(dir.path())
        .args(["--offline", "workspace-create", "graph-chain"])
        .output()
        .await
        .unwrap();
    assert!(out.status.success());
    let bootstrap: Value = serde_json::from_slice(&out.stdout).unwrap();
    let token = bootstrap["token"].as_str().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let model = format!("http://{}", listener.local_addr().unwrap());
    let mock = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/embeddings", post_route())
                .route("/chat/completions", axum::routing::post(extract))
                .with_state(Arc::new(AtomicBool::new(false))),
        )
        .into_future(),
    );
    let (mut host, mut base) = start(dir.path(), &model).await;
    let http = reqwest::Client::new();
    let doc = post(
        &http,
        &base,
        token,
        "/v1/knowledge",
        json!({
            "title":"chain", "content":"GRAPH-CHAIN-FIXTURE seedalpha evidence", "format":"markdown"
        }),
    )
    .await;
    job(&http, &base, token, id(&doc, "job_id"), "completed").await;
    let query = json!({"query":"seedalpha","mode":"hybrid","limit":100,"components":{"summaries":false,"graph":true}});
    let shared = post(&http,&base,token,"/v1/knowledge",json!({"title":"shared chain","content":"GRAPH-CHAIN-FIXTURE seedalpha shared evidence","format":"markdown"})).await;
    job(&http, &base, token, id(&shared, "job_id"), "completed").await;
    let shared_graph = post(&http, &base, token, "/v1/search", query.clone()).await;
    assert_eq!(
        shared_graph["graph"]["relations"].as_array().unwrap().len(),
        2
    );
    assert!(
        shared_graph["graph"]["relations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|edge| edge["evidence"].as_array().unwrap().len() >= 2)
    );
    let removed = request(
        &http,
        &base,
        token,
        reqwest::Method::DELETE,
        &format!("/v1/assets/{}", shared["asset_id"].as_str().unwrap()),
        Value::Null,
    )
    .await;
    job(
        &http,
        &base,
        token,
        id(&removed, "cleanup_job_id"),
        "completed",
    )
    .await;
    assert!(
        !dir.path().join("graph.db").exists(),
        "fresh deployment must not create graph.db"
    );
    stop(&mut host).await;
    let legacy_bytes = b"fixture-owned invalid legacy graph database: must remain untouched";
    std::fs::write(dir.path().join("graph.db"), legacy_bytes).unwrap();
    (host, base) = start(dir.path(), &model).await;
    assert_eq!(
        std::fs::read(dir.path().join("graph.db")).unwrap(),
        legacy_bytes
    );
    let remote = crate::support::fixture(dir.path(), Some(3));
    remote.clear_queries();
    let result = post(&http, &base, token, "/v1/search", query.clone()).await;
    let entities = result["graph"]["entities"].as_array().unwrap();
    let mut actual = entities.iter().map(|v| id(v, "id")).collect::<Vec<_>>();
    actual.sort();
    let mut expected = ["seedalpha", "neighborbeta", "incomingdelta"]
        .map(entity_id)
        .to_vec();
    expected.sort();
    assert_eq!(
        actual, expected,
        "secondgamma must not be reached through neighborbeta"
    );
    let edges = result["graph"]["relations"].as_array().unwrap();
    assert_eq!(edges.len(), 2);
    for (from, to) in [
        ("seedalpha", "neighborbeta"),
        ("incomingdelta", "seedalpha"),
    ] {
        let expected_id = relation_id(entity_id(from), "points_to", entity_id(to));
        let edge = edges
            .iter()
            .find(|e| id(e, "id") == expected_id)
            .expect("incident edge");
        assert_eq!(id(edge, "source_id"), entity_id(from));
        assert_eq!(id(edge, "target_id"), entity_id(to));
        assert!(!edge["evidence"].as_array().unwrap().is_empty());
    }
    let wire = remote.queries();
    let adjacency = wire
        .iter()
        .flat_map(|q| q["filter"].as_array().unwrap())
        .filter(|f| matches!(f["StringSet"]["field"].as_str(), Some("endpoint_ids")))
        .collect::<Vec<_>>();
    assert_eq!(
        adjacency.len(),
        1,
        "one exact incident-endpoint query, no expanded frontier"
    );
    for filter in adjacency {
        assert_eq!(
            filter["StringSet"]["filter"],
            json!([entity_id("seedalpha")])
        );
    }
    remote.clear_queries();
    let mut disabled_query = query.clone();
    disabled_query["components"]["graph"] = json!(false);
    let disabled = post(&http, &base, token, "/v1/search", disabled_query).await;
    assert_eq!(disabled["graph"], json!({"entities":[],"relations":[]}));
    let hit_ids = |value: &Value| {
        let mut ids = value["hits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| id(v, "chunk_id"))
            .collect::<Vec<_>>();
        ids.sort();
        ids
    };
    assert_eq!(
        hit_ids(&result),
        hit_ids(&disabled),
        "graph off must retain native candidate paths"
    );
    assert!(
        remote.queries().iter().all(
            |q| q["filter"].as_array().unwrap().iter().all(|f| !matches!(
                f.get("StringSet").or_else(|| f.get("String")).unwrap()["field"].as_str(),
                Some("graph_keys" | "graph_id" | "endpoint_ids")
            ))
        ),
        "graph-disabled must not query the graph projection"
    );
    let projection = remote.snapshot_records();
    assert!(
        projection.iter().all(|r| r["document"]["kind"] == "chunk"),
        "graph metadata must not create standalone BM25 documents"
    );
    let old_projection = projection
        .into_iter()
        .filter(|r| !r["document"]["graph"].is_null())
        .collect::<Vec<_>>();
    assert_eq!(
        old_projection.len(),
        1,
        "one existing chunk anchors the source graph"
    );
    let updated = post(&http,&base,token,"/v1/knowledge",json!({
        "title":"chain replacement", "content":"EMPTY-EXTRACT seedalpha replacement", "format":"markdown",
        "asset_id":doc["asset_id"], "expected_version":1
    })).await;
    job(&http, &base, token, id(&updated, "job_id"), "completed").await;
    remote.restore_stale_records(&old_projection);
    let current = post(&http, &base, token, "/v1/search", query.clone()).await;
    assert_eq!(
        current["graph"],
        json!({"entities":[],"relations":[]}),
        "stale graph projection cannot revive the old version"
    );
    assert!(
        current["hits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|hit| hit["version"] == 2)
    );
    let deleted = request(
        &http,
        &base,
        token,
        reqwest::Method::DELETE,
        &format!("/v1/assets/{}", doc["asset_id"].as_str().unwrap()),
        Value::Null,
    )
    .await;
    // Authorization hides the source immediately, independent of remote cleanup.
    let withdrawn = post(&http, &base, token, "/v1/search", query.clone()).await;
    assert_eq!(withdrawn["graph"], json!({"entities":[],"relations":[]}));
    job(
        &http,
        &base,
        token,
        id(&deleted, "cleanup_job_id"),
        "completed",
    )
    .await;
    let cleaned = post(&http, &base, token, "/v1/search", query).await;
    assert_eq!(cleaned["graph"], json!({"entities":[],"relations":[]}));
    let fresh = post(&http,&base,token,"/v1/knowledge",json!({
        "title":"source withdrawal", "content":"GRAPH-CHAIN-FIXTURE seedalpha source", "format":"markdown"
    })).await;
    job(&http, &base, token, id(&fresh, "job_id"), "completed").await;
    let live = post(
        &http,
        &base,
        token,
        "/v1/search",
        json!({"query":"seedalpha","mode":"hybrid"}),
    )
    .await;
    assert_eq!(live["graph"]["relations"].as_array().unwrap().len(), 2);
    let source = live["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["asset_id"] == fresh["asset_id"])
        .unwrap()["source_event_id"]
        .as_str()
        .unwrap();
    let revoked = request(
        &http,
        &base,
        token,
        reqwest::Method::DELETE,
        &format!("/v1/events/{source}"),
        Value::Null,
    )
    .await;
    let hidden = post(
        &http,
        &base,
        token,
        "/v1/search",
        json!({"query":"seedalpha","mode":"hybrid"}),
    )
    .await;
    assert_eq!(
        hidden["graph"],
        json!({"entities":[],"relations":[]}),
        "source retraction hides graph immediately"
    );
    job(
        &http,
        &base,
        token,
        id(&revoked, "cleanup_job_id"),
        "completed",
    )
    .await;
    stop(&mut host).await;
    assert_eq!(
        std::fs::read(dir.path().join("graph.db")).unwrap(),
        legacy_bytes
    );
    mock.abort();
}
