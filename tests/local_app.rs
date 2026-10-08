#![cfg(feature = "local-storage")]
use axum::{Json, Router, extract::State};
use opencontext::{models::Models, service::Service, storage::*, types::*};
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
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
};
use uuid::Uuid;
fn id(v: &Value, k: &str) -> Uuid {
    serde_json::from_value(v[k].clone()).unwrap()
}
fn command(dir: &std::path::Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_opencontext"));
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
    } else if prompt.contains("EMPTY-EXTRACT") {
        json!({"memories":[],"entities":[],"relations":[]}).to_string()
    } else {
        json!({"memories":[
            {"fact_key":"model.claim","content":"Model proposal requires review","publish_if_authorized":true},
            {"fact_key":"model.second","content":"Second extracted fact","publish_if_authorized":true},
            {"fact_key":"model.claim","content":"Duplicate claim ignored","publish_if_authorized":true}
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
async fn rpc(
    input: &mut tokio::process::ChildStdin,
    output: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    value: Value,
) -> Value {
    input
        .write_all(format!("{value}\n").as_bytes())
        .await
        .unwrap();
    input.flush().await.unwrap();
    let line = tokio::time::timeout(Duration::from_secs(10), output.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    serde_json::from_str(&line).unwrap()
}
fn pdf() -> Vec<u8> {
    let stream = "BT /F1 12 Tf 72 720 Td (Release approval evidence) Tj ET";
    let objects=["<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{stream}\nendstream",stream.len())];
    let mut doc = "%PDF-1.4\n".to_string();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(doc.len());
        doc += &format!("{} 0 obj\n{object}\nendobj\n", i + 1);
    }
    let xref = doc.len();
    doc += "xref\n0 6\n0000000000 65535 f \n";
    for offset in offsets {
        doc += &format!("{offset:010} 00000 n \n");
    }
    doc += &format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n");
    doc.into_bytes()
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
    // A duplicate fact_key in one extraction drops instead of breaking the job.
    assert_eq!(extracted["result"]["memories"].as_array().unwrap().len(), 2);
    let identity = json!({"subject":{"kind":"service","stable_id":"billing"},"predicate":"release.approval","context":{"environment":"production"}});
    let first = post(
        &http,
        &base,
        token,
        "/v1/captures/identified",
        json!({"identity":identity,"content":"生产发布需要审批"}),
    )
    .await;
    let published = job(&http, &base, token, id(&first, "job_id"), "completed").await;
    assert_eq!(published["result"]["memories"][0]["identity"], identity);
    let second = post(
        &http,
        &base,
        token,
        "/v1/captures/identified",
        json!({"identity":identity,"content":"生产发布需要双人审批"}),
    )
    .await;
    assert_eq!(first["asset_id"], second["asset_id"]);
    job(&http, &base, token, id(&second, "job_id"), "completed").await;
    let view = request(
        &http,
        &base,
        token,
        reqwest::Method::GET,
        &format!("/v1/assets/{}", first["asset_id"].as_str().unwrap()),
        Value::Null,
    )
    .await;
    assert_eq!(view["version"], 2);
    assert_eq!(view["identity"], identity);

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
    assert_eq!(unchanged["version"], 2);
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
    // PDF parsing is a real child process, with upload bytes preserved under local blobs.
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
        json!({"workspace_id":bootstrap["workspace_id"],"role":"reviewer"}),
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
    // Remote MCP coexists with the exclusive host and reauthorizes each call.
    let key = post(
        &http,
        &base,
        token,
        "/admin/keys",
        json!({"workspace_id":bootstrap["workspace_id"],"role":"reader"}),
    )
    .await;
    let mut mcp = command(dir.path())
        .env("OC_API_KEY", key["token"].as_str().unwrap())
        .env("OC_SERVER_URL", &base)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = mcp.stdin.take().unwrap();
    let mut output = BufReader::new(mcp.stdout.take().unwrap()).lines();
    let hello=rpc(&mut input,&mut output,json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1"}}})).await;
    assert!(hello.get("result").is_some());
    input
        .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
        .await
        .unwrap();
    let tools = rpc(
        &mut input,
        &mut output,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    )
    .await;
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 3);

    let recalled=rpc(&mut input,&mut output,json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"context_search","arguments":{"query":"Atlas","mode":"hybrid"}}})).await;
    let recalled: Value =
        serde_json::from_str(recalled["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(recalled["graph"]["entities"].as_array().unwrap().len(), 2);
    let assembled=rpc(&mut input,&mut output,json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"context_resolve","arguments":{"query":"Atlas","mode":"hybrid","budget_tokens":3000}}})).await;
    let assembled: Value =
        serde_json::from_str(assembled["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(assembled["count"].as_u64().unwrap() <= 3000);
    assert!(
        assembled["sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s.get("entity_id").is_some() && !s["evidence"].as_array().unwrap().is_empty())
    );
    let get = json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"context_get","arguments":{"asset_id":published["asset_id"]}}});
    let result = rpc(&mut input, &mut output, get.clone()).await;
    assert!(
        result["result"]["content"][0]["text"]
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
    assert!(
        rpc(&mut input, &mut output, get)
            .await
            .get("error")
            .is_some()
    );
    stop(&mut mcp).await;
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
    let offline = command(dir.path())
        .env("OC_API_KEY", token)
        .args(["--offline", "search", "approval"])
        .output()
        .await
        .unwrap();
    assert!(offline.status.success());
    mock.abort();
}
fn post_route() -> axum::routing::MethodRouter<Arc<AtomicBool>> {
    axum::routing::post(embed)
}

#[tokio::test]
async fn local_transactions_versions_retraction_and_idempotency() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
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
    assert!(opencontext::worker::process_next(&s).await.unwrap());
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
    assert!(opencontext::worker::process_next(&s).await.unwrap());
    let zh = s
        .search(
            &a,
            SearchInput {
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
    assert!(opencontext::worker::process_next(&s).await.unwrap());
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
    assert!(opencontext::worker::process_next(&s).await.unwrap());
    assert_eq!(
        s.get(&a, id(&first, "asset_id"), None).await.unwrap()["version"],
        2
    );
    let restored = s
        .restore(
            &a,
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
    assert!(opencontext::worker::process_next(&s).await.unwrap());
    assert_eq!(
        s.job(&a, id(&restored, "job_id")).await.unwrap()["result"]["version"],
        3
    );
    let deleted = s
        .delete(&a, "retract", id(&first, "source_event_id"), "event")
        .await
        .unwrap();
    assert!(s.get(&a, id(&first, "asset_id"), None).await.is_err());
    assert!(opencontext::worker::process_next(&s).await.unwrap());
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
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
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
    assert!(opencontext::worker::process_next(&s).await.unwrap());
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
    assert!(opencontext::worker::process_next(&s).await.unwrap());
    assert!(opencontext::worker::process_next(&s).await.unwrap());
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
async fn saved_publication_recovers_after_external_graph_write() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
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
    let entity_id = opencontext::graph::entity_id("Atlas");
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
    let mut tx = s.engine.relational().begin(auth).await.unwrap();
    tx.check_permission(Permission::Write).await.unwrap();
    tx.save_publication(claim.job_id,&json!({"generation":claim.generation,"profile":null,"memories":[{"asset":claim.asset,"fact_key":null,"expected_version":null,"version":1,"chunks":[{"id":chunk_id,"chunk":{"content":"Atlas evidence","locator":{"start":0,"end":14}},"embedding":null,"summary":""}]}],"graph":{"entities":[entity],"relations":[]}})).await.unwrap();
    tx.register_pending(LedgerEntry {
        key: ledger_key,
        artifact_type: "entity".into(),
        idempotency_key: ledger_idempotency_key(&ledger_key),
    })
    .await
    .unwrap();
    tx.commit().await.unwrap();
    s.engine
        .graph()
        .upsert_entities(scope, source, vec![entity])
        .await
        .unwrap();
    assert!(s.get(&a, id(&accepted, "asset_id"), None).await.is_err());
    // Persisted boundary: graph has the object, SQLite has only pending intent, no version.
    s.engine.shutdown().await.unwrap();
    drop(s);
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    assert_eq!(
        s.engine.graph().list_ids(scope).await.unwrap()["entities"],
        json!([])
    );
    assert!(opencontext::worker::process_next(&s).await.unwrap());
    assert!(!opencontext::worker::process_next(&s).await.unwrap());
    assert_eq!(
        s.get(&a, id(&accepted, "asset_id"), None).await.unwrap()["version"],
        1
    );
    assert_eq!(
        s.engine.graph().list_ids(scope).await.unwrap()["entities"],
        json!([entity_id])
    );
    let counts:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM oc_versions),(SELECT count(*) FROM oc_artifact_ledger WHERE state='committed')").fetch_one(s.engine.relational().pool()).await.unwrap();
    assert_eq!(counts, (1, 1));

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
        idempotency_key: ledger_idempotency_key(&vk),
    })
    .await
    .unwrap();
    tx.commit().await.unwrap();
    s.engine
        .vector()
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
    assert_eq!(s.engine.vector().search(q.clone()).await.unwrap().len(), 1);
    s.delete(&a, "delete", source.source_id, "event")
        .await
        .unwrap();
    // No worker is running: stale native records exist but cannot become public evidence.
    assert_eq!(
        s.engine.graph().list_ids(scope).await.unwrap()["entities"],
        json!([entity_id])
    );
    assert_eq!(s.engine.vector().search(q.clone()).await.unwrap().len(), 1);
    let mut tx = s.engine.relational().begin_read(auth).await.unwrap();
    assert!(tx.graph_evidence(entity_id).await.unwrap().is_empty());
    assert!(
        tx.vector_hit(chunk_id, source, "test", 1)
            .await
            .unwrap()
            .is_none()
    );
    assert!(tx.vector_candidates("test", 2, 1).await.unwrap().is_empty());
    tx.commit().await.unwrap();
    assert!(opencontext::worker::process_next(&s).await.unwrap());
    assert_eq!(
        s.engine.graph().list_ids(scope).await.unwrap()["entities"],
        json!([])
    );
    assert!(s.engine.vector().search(q).await.unwrap().is_empty());
    s.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn graceful_host_shutdown_stops_worker_and_closes_store() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let view = s.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let host = tokio::spawn(opencontext::host::serve(s, listener, async {
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
