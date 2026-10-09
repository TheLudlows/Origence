//! S0 service-level regression with real SQLite/LanceDB and loopback-only embeddings.
//! Fixed vectors prove candidate filtering, not semantic retrieval quality.
#![cfg(feature = "local-storage")]

use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use origence::{
    memory_identity::MemoryIdentity,
    models::Models,
    parsing,
    service::Service,
    storage::{
        DomainTx, IssuedKey, LedgerEntry, LedgerKey, RelationalStore, Scope, SourceVersion,
        Surface, VectorEntry, VectorQuery, VectorStore, hash, ledger_idempotency_key,
    },
};
use reqwest::{Client, Method};
use serde_json::{Value, json};
use std::{
    path::Path,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    task::JoinHandle,
};
use uuid::Uuid;

const QUERY: &str = "needle";
const DISTRACTORS: usize = 130;

#[derive(Clone, Default)]
struct EmbeddingStub {
    unavailable: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
}

async fn embedding(
    State(state): State<EmbeddingStub>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    state.calls.fetch_add(1, Ordering::SeqCst);
    assert_eq!(body["input"], QUERY);
    if state.unavailable.load(Ordering::SeqCst) {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    Ok(Json(json!({"data": [{"embedding": [1.0, 0.0, 0.0]}]})))
}

struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct Fixture {
    scope: Scope,
    identity: MemoryIdentity,
    admin: IssuedKey,
    reader: IssuedKey,
    asset: Uuid,
    old_chunk: Uuid,
    current_chunk: Uuid,
    current_source: Uuid,
    content: String,
}

/// Batch-seed real adapters; no mock vector store, raw SQL, or global env mutation.
async fn seed(service: &Service, scope: Scope, profile: &str, decoys: usize) -> Fixture {
    let store = service.engine.relational();
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "s0-vector")
        .await
        .unwrap();
    let admin = store.issue_key(scope, "admin").await.unwrap();
    let reader = store.issue_key(scope, "reader").await.unwrap();
    let auth = store.authenticate(&admin.token).await.unwrap();
    let mut fixture = Fixture {
        scope,
        identity: serde_json::from_value(json!({
            "subject": {"kind": "service", "stable_id": "target"},
            "predicate": "release.approval",
            "context": {"environment": "production"}
        }))
        .unwrap(),
        admin,
        reader,
        asset: Uuid::nil(),
        old_chunk: Uuid::nil(),
        current_chunk: Uuid::nil(),
        current_source: Uuid::nil(),
        content: String::new(),
    };
    let mut tx = store.begin(auth.clone()).await.unwrap();
    let mut entries = Vec::new();
    let mut ledger = Vec::new();
    for index in 0..=decoys {
        let is_target = index == decoys;
        let mut identity = fixture.identity.clone();
        if !is_target {
            identity.subject.stable_id = format!("distractor-{index}");
        }
        let (asset, _) = tx.identity_slot(&identity).await.unwrap();
        let last_version = if is_target { 2 } else { 1 };
        for version in 1..=last_version {
            // Neither target version contains QUERY: hybrid cannot pass via keyword.
            let content = if is_target {
                format!(
                    "Approval evidence version {version} for {}",
                    scope.workspace_id
                )
            } else {
                format!("{QUERY} competitor {index}")
            };
            let source_id = tx.create_event("memory", &content, None).await.unwrap();
            let source = SourceVersion { source_id, version };
            let chunk = Uuid::new_v4();
            tx.insert_version(
                asset,
                version,
                &content,
                &hash(&content),
                source_id,
                None,
                None,
            )
            .await
            .unwrap();
            tx.insert_chunk(
                chunk,
                asset,
                version,
                0,
                &content,
                &json!({"byte_start": 0, "byte_end": content.len()}),
                &parsing::lexical(&content),
            )
            .await
            .unwrap();
            tx.update_asset_version(asset, version, None).await.unwrap();
            let key = LedgerKey {
                scope,
                source,
                artifact_id: chunk,
                surface: Surface::Vector,
                generation: 1,
            };
            tx.register_pending(LedgerEntry {
                key,
                artifact_type: "chunk".into(),
                idempotency_key: ledger_idempotency_key(&key),
            })
            .await
            .unwrap();
            tx.index_ready(chunk, profile, 3, 1).await.unwrap();
            ledger.push(key);
            entries.push(VectorEntry {
                id: chunk,
                embedding: if is_target && version == 2 {
                    vec![0.0, 1.0, 0.0]
                } else {
                    vec![1.0, 0.0, 0.0]
                },
                profile: profile.into(),
                dimension: 3,
                generation: 1,
                source,
            });
            if is_target {
                fixture.asset = asset;
                if version == 1 {
                    fixture.old_chunk = chunk;
                } else {
                    fixture.current_chunk = chunk;
                    fixture.current_source = source_id;
                    fixture.content = content;
                }
            }
        }
    }
    tx.commit().await.unwrap();
    service
        .engine
        .vector()
        .upsert(scope, entries)
        .await
        .unwrap();
    let mut tx = store.begin(auth).await.unwrap();
    for key in ledger {
        tx.confirm_committed(key).await.unwrap();
    }
    tx.commit().await.unwrap();
    fixture
}

struct Host {
    child: Child,
    base: String,
    http: Client,
}

impl Host {
    async fn start(dir: &Path, model_url: &str) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_origence"))
            .env("OC_DATA_DIR", dir)
            .env("OC_ENABLE_MODELS", "true")
            .env("OC_MODEL_BASE_URL", model_url)
            .env("OC_MODEL_API_KEY", "local-s0-test")
            .env("OC_EMBEDDING_MODEL", "s0-regression")
            .env("OC_EMBEDDING_DIMENSION", "3")
            .env("OC_EXTRACTION_MODEL", "")
            .env("NO_PROXY", "127.0.0.1,localhost")
            .env("RUST_LOG", "error")
            .args(["serve", "--bind", "127.0.0.1:0"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let line = tokio::time::timeout(Duration::from_secs(60), lines.next_line())
            .await
            .expect("host startup deadline")
            .unwrap()
            .expect("host readiness announcement");
        let announcement: Value = serde_json::from_str(&line).unwrap();
        let host = Self {
            child,
            base: format!("http://{}", announcement["listening"].as_str().unwrap()),
            http: Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(20))
                .build()
                .unwrap(),
        };
        host.call(
            Method::GET,
            "/health/ready",
            "",
            Value::Null,
            StatusCode::OK,
        )
        .await;
        host
    }

    async fn call(
        &self,
        method: Method,
        path: &str,
        token: &str,
        body: Value,
        expected: StatusCode,
    ) -> Value {
        let response = self
            .http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(token)
            .header("Idempotency-Key", Uuid::new_v4().to_string())
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let value: Value = response.json().await.unwrap();
        assert_eq!(status, expected, "{path}: {value}");
        value
    }

    async fn search(&self, fixture: &Fixture, mode: &str, filter: Option<MemoryIdentity>) -> Value {
        self.call(
            Method::POST,
            "/v1/search",
            &fixture.reader.token,
            json!({"query": QUERY, "mode": mode, "limit": 100,
                "allow_partial": false, "memory_identity": filter}),
            StatusCode::OK,
        )
        .await
    }

    async fn resolve(&self, fixture: &Fixture, mode: &str) -> Value {
        self.call(
            Method::POST,
            "/v1/resolve",
            &fixture.reader.token,
            json!({"query": QUERY, "mode": mode, "budget_tokens": 4000,
                "allow_partial": false, "memory_identity": fixture.identity}),
            StatusCode::OK,
        )
        .await
    }
}

fn assert_target(result: &Value, fixture: &Fixture, mode: &str) {
    assert_eq!(result["effective_mode"], mode);
    assert_eq!(result["warnings"], json!([]));
    assert_eq!(result["hits"].as_array().unwrap().len(), 1, "{result}");
    let hit = &result["hits"][0];
    assert_eq!(hit["asset_id"], json!(fixture.asset));
    assert_eq!(hit["chunk_id"], json!(fixture.current_chunk));
    assert_eq!(hit["source_event_id"], json!(fixture.current_source));
    assert_eq!(hit["version"], 2);
    assert_eq!(hit["content"], fixture.content);
    assert_eq!(hit["identity"], json!(fixture.identity));
    assert_eq!(result["graph"], json!({"entities": [], "relations": []}));
}

async fn exercise(mode: &str) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let model_url = format!("http://{}/v1", listener.local_addr().unwrap());
    let state = EmbeddingStub::default();
    let router = Router::new()
        .route("/v1/embeddings", post(embedding))
        .with_state(state.clone());
    let _model_server = AbortOnDrop(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    let dir = tempfile::tempdir().unwrap();
    let profile = format!("{}:s0-regression:3:v1", hash(&model_url));
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    let (primary, other_workspace, other_tenant) = {
        let service = Service::open(dir.path(), Models::disabled()).await.unwrap();
        let primary = seed(&service, scope, &profile, DISTRACTORS).await;
        let other_workspace = seed(
            &service,
            Scope {
                tenant_id: scope.tenant_id,
                workspace_id: Uuid::new_v4(),
            },
            &profile,
            0,
        )
        .await;
        let other_tenant = seed(
            &service,
            Scope {
                tenant_id: Uuid::new_v4(),
                workspace_id: Uuid::new_v4(),
            },
            &profile,
            0,
        )
        .await;
        let store = service.engine.relational();
        let mut tx = store
            .begin(store.authenticate(&primary.reader.token).await.unwrap())
            .await
            .unwrap();
        let ids = tx.vector_candidates(&profile, 3, 1, None).await.unwrap();
        assert_eq!(ids.len(), DISTRACTORS + 1);
        assert!(!ids.contains(&primary.old_chunk));
        tx.commit().await.unwrap();
        let native = service
            .engine
            .vector()
            .search(VectorQuery {
                scope: primary.scope,
                artifact_ids: Some(ids),
                profile: profile.clone(),
                dimension: 3,
                generation: 1,
                embedding: vec![1.0, 0.0, 0.0],
                limit: 100,
            })
            .await
            .unwrap();
        assert_eq!(native.len(), 100);
        assert!(native.iter().all(|hit| hit.id != primary.current_chunk));
        // The old vector is physically present but must never become current evidence.
        let stale = service
            .engine
            .vector()
            .search(VectorQuery {
                scope,
                artifact_ids: Some(vec![primary.old_chunk]),
                profile: profile.clone(),
                dimension: 3,
                generation: 1,
                embedding: vec![1.0, 0.0, 0.0],
                limit: 1,
            })
            .await
            .unwrap();
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].source.version, 1);
        service.engine.shutdown().await.unwrap();
        (primary, other_workspace, other_tenant)
    };
    // Reopen through the actual executable: configuration and HTTP wiring are exercised.
    let mut host = Host::start(dir.path(), &model_url).await;
    let plain = host.search(&primary, mode, None).await;
    assert_eq!(plain["effective_mode"], mode);
    assert_eq!(plain["hits"].as_array().unwrap().len(), 100);
    assert!(
        plain["hits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|hit| { hit["asset_id"] != json!(primary.asset) })
    );
    let keyword = host
        .search(&primary, "keyword", Some(primary.identity.clone()))
        .await;
    assert_eq!(
        keyword["hits"],
        json!([]),
        "hybrid must not pass via keyword"
    );
    for fixture in [&primary, &other_workspace, &other_tenant] {
        let result = host
            .search(fixture, mode, Some(fixture.identity.clone()))
            .await;
        assert_target(&result, fixture, mode);
        let context = host.resolve(fixture, mode).await;
        assert_eq!(context["effective_mode"], mode);
        assert_eq!(context["sources"].as_array().unwrap().len(), 1);
        assert_eq!(context["sources"][0]["asset_id"], json!(fixture.asset));
        assert_eq!(context["sources"][0]["version"], 2);
        assert!(
            context["rendered_context"]
                .as_str()
                .unwrap()
                .contains(&fixture.content)
        );
    }
    assert!(state.calls.load(Ordering::SeqCst) >= 7);
    let mut missing = primary.identity.clone();
    missing.subject.stable_id = "missing".into();
    assert_eq!(
        host.search(&primary, mode, Some(missing)).await["hits"],
        json!([])
    );
    let mut different_context = primary.identity.clone();
    different_context
        .context
        .insert("environment".into(), "staging".into());
    assert_eq!(
        host.search(&primary, mode, Some(different_context)).await["hits"],
        json!([])
    );

    state.unavailable.store(true, Ordering::SeqCst);
    host.call(
        Method::POST,
        "/v1/search",
        &primary.reader.token,
        json!({"query": QUERY, "mode": mode, "memory_identity": primary.identity}),
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
    let partial = host
        .call(
            Method::POST,
            "/v1/search",
            &primary.reader.token,
            json!({"query": QUERY, "mode": mode, "allow_partial": true,
            "memory_identity": primary.identity}),
            StatusCode::OK,
        )
        .await;
    assert_eq!(partial["effective_mode"], "keyword");
    assert_eq!(partial["hits"], json!([]));
    assert!(!partial["warnings"].as_array().unwrap().is_empty());
    state.unavailable.store(false, Ordering::SeqCst);

    host.call(
        Method::DELETE,
        &format!("/v1/events/{}", primary.current_source),
        &primary.admin.token,
        Value::Null,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        host.search(&primary, mode, Some(primary.identity.clone()))
            .await["hits"],
        json!([])
    );
    assert_eq!(host.resolve(&primary, mode).await["sources"], json!([]));
    let old = host
        .call(
            Method::GET,
            &format!("/v1/assets/{}?version=1", primary.asset),
            &primary.reader.token,
            Value::Null,
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        old["version"], 1,
        "a valid old version must not be used as fallback"
    );
    host.call(
        Method::DELETE,
        &format!("/v1/assets/{}", other_workspace.asset),
        &other_workspace.admin.token,
        Value::Null,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        host.search(
            &other_workspace,
            mode,
            Some(other_workspace.identity.clone())
        )
        .await["hits"],
        json!([])
    );
    assert_eq!(
        host.resolve(&other_workspace, mode).await["sources"],
        json!([])
    );
    assert_target(
        &host
            .search(&other_tenant, mode, Some(other_tenant.identity.clone()))
            .await,
        &other_tenant,
        mode,
    );
    host.child.kill().await.unwrap();
    host.child.wait().await.unwrap();
}

#[tokio::test]
async fn s0_vector_identity_prefilter_real_lancedb() {
    tokio::time::timeout(Duration::from_secs(180), exercise("vector"))
        .await
        .expect("vector regression deadline");
}

#[tokio::test]
async fn s0_hybrid_identity_prefilter_real_lancedb() {
    tokio::time::timeout(Duration::from_secs(180), exercise("hybrid"))
        .await
        .expect("hybrid regression deadline");
}
