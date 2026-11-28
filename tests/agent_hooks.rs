//! Exercise the installed Node hook with synthetic client events and a local HTTP peer.
use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path as RoutePath, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

pub(super) const HOOK_ENTRY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/plugins/origence/hooks/entry.mjs"
);

const TOKEN: &str = "fixture-only-project-writer-token";
const OTHER_TOKEN: &str = "fixture-only-other-writer-token";

pub(super) struct HookFixture {
    _root: tempfile::TempDir,
    pub project: PathBuf,
    pub data: PathBuf,
}

impl HookFixture {
    pub fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let data = root.path().join("plugin-data");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&data).unwrap();
        Self {
            _root: root,
            project,
            data,
        }
    }

    pub fn event(&self, client: &str, name: &str) -> Value {
        let raw = match client {
            "codex" => include_str!("fixtures/client-hooks/codex.json"),
            "claude-code" => include_str!("fixtures/client-hooks/claude-code.json"),
            _ => panic!("unknown fixture client"),
        };
        let values: Value = serde_json::from_str(raw).unwrap();
        let mut input = values[name].clone();
        assert!(input.is_object(), "{name}");
        input["cwd"] = json!(self.project);
        input
    }

    pub async fn invoke(
        &self,
        client: &str,
        input: Value,
        base: &str,
        token: &str,
        workspace: Option<Uuid>,
        enabled: bool,
    ) {
        use std::process::Stdio;
        use tokio::io::AsyncWriteExt;
        let mut command = tokio::process::Command::new("node");
        command
            .current_dir(&self.project)
            .env("OC_API_KEY", token)
            .env_remove("OC_WORKSPACE_ID")
            .env("OC_SERVER_URL", base)
            .env_remove("OC_TURN_SYNC_PROJECT")
            .env("OC_PLUGIN_RUN_HOOK", if enabled { "1" } else { "0" })
            .env("PLUGIN_DATA", &self.data)
            .env("CLAUDE_PLUGIN_DATA", &self.data)
            .args(["--disable-warning=ExperimentalWarning", HOOK_ENTRY, client])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(workspace) = workspace {
            command.env("OC_WORKSPACE_ID", workspace.to_string());
        }
        let mut child = command.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .await
            .unwrap();
        let output = child.wait_with_output().await.unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"{}\n");
        assert!(!String::from_utf8(output.stderr).unwrap().contains(token));
    }

    pub fn pending(&self) -> Vec<Value> {
        let mut pending = Vec::new();
        for directory in fs::read_dir(&self.data).unwrap() {
            let file = directory.unwrap().path().join("queue-v1.json");
            if file.is_file() {
                let state: Value = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
                pending.extend(state["pending"].as_array().unwrap().iter().cloned());
            }
        }
        pending
    }
}

#[derive(Default)]
struct Requests {
    plugin_scopes: Vec<Uuid>,
    creates: Vec<Value>,
    appends: Vec<(Uuid, Value)>,
    append_bodies: Vec<Bytes>,
    lose_append_receipt: bool,
}

#[derive(Clone)]
struct HttpFixture {
    base: String,
    key: Uuid,
    workspace: Uuid,
    log: Arc<Mutex<Requests>>,
}

impl HttpFixture {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let fixture = Self::new(listener.local_addr().unwrap());
        fixture.serve(listener);
        fixture
    }

    fn new(address: std::net::SocketAddr) -> Self {
        Self {
            base: format!("http://{address}"),
            key: Uuid::new_v4(),
            workspace: Uuid::new_v4(),
            log: Arc::new(Mutex::new(Requests::default())),
        }
    }

    fn serve(&self, listener: tokio::net::TcpListener) {
        let app = Router::new()
            .route("/v1/whoami", get(whoami))
            .route("/v1/session", get(session))
            .route("/v1/runs", post(create))
            .route("/v1/runs/{run}/events", post(append))
            .with_state(self.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    }
}

type Reply = (StatusCode, Json<Value>);

fn reply(status: StatusCode, json: Value) -> Reply {
    (status, Json(json))
}

async fn whoami(State(peer): State<HttpFixture>, headers: HeaderMap) -> Reply {
    let role = if headers["authorization"].to_str().unwrap() == format!("Bearer {TOKEN}") {
        "writer"
    } else {
        "reader"
    };
    reply(
        StatusCode::OK,
        json!({"id": peer.key, "tenant_id": peer.key, "workspace_id": peer.workspace, "role": role}),
    )
}

async fn session(State(peer): State<HttpFixture>, headers: HeaderMap) -> Reply {
    if headers["authorization"].to_str().unwrap() != format!("Bearer {TOKEN}") {
        return reply(StatusCode::UNAUTHORIZED, json!({}));
    }
    reply(
        StatusCode::OK,
        json!({"key_id": peer.key, "workspace_id": peer.workspace}),
    )
}

async fn create(
    State(peer): State<HttpFixture>,
    headers: HeaderMap,
    Json(run): Json<Value>,
) -> Reply {
    let mut log = peer.log.lock().unwrap();
    if let Some(scope) = headers.get("x-origence-workspace") {
        log.plugin_scopes
            .push(scope.to_str().unwrap().parse().unwrap());
    }
    log.creates.push(run.clone());
    reply(
        StatusCode::OK,
        json!({"run_id": run["run_id"], "session_id": Uuid::new_v4()}),
    )
}

async fn append(
    State(peer): State<HttpFixture>,
    RoutePath(run): RoutePath<Uuid>,
    body: Bytes,
) -> Reply {
    let report: Value = serde_json::from_slice(&body).unwrap();
    let (sequence, lose_receipt) = {
        let mut log = peer.log.lock().unwrap();
        log.appends.push((run, report.clone()));
        log.append_bodies.push(body);
        (
            log.appends.len(),
            std::mem::take(&mut log.lose_append_receipt),
        )
    };
    if lose_receipt {
        return reply(StatusCode::OK, json!({})); // Accepted, but no verifiable receipt.
    }
    reply(
        StatusCode::OK,
        json!({"run_id": run, "event_id": report["event_id"], "sequence": sequence}),
    )
}

#[tokio::test]
async fn installed_hook_pins_workspace_and_retries_without_exposing_event_payload() {
    let peer = HttpFixture::start().await;
    let fixture = HookFixture::new();
    let codex = fixture.event("codex", "start");
    let selected = Some(peer.workspace);
    fixture
        .invoke(
            "codex",
            codex.clone(),
            &peer.base,
            OTHER_TOKEN,
            selected,
            true,
        )
        .await;
    fixture
        .invoke("codex", codex.clone(), &peer.base, TOKEN, selected, false)
        .await;
    fixture
        .invoke(
            "codex",
            codex.clone(),
            &peer.base,
            TOKEN,
            Some(Uuid::new_v4()),
            true,
        )
        .await;
    let mut outside = codex.clone();
    outside["cwd"] = json!(fixture.data);
    fixture
        .invoke("codex", outside, &peer.base, TOKEN, selected, true)
        .await;
    assert_eq!(fs::read_dir(&fixture.data).unwrap().count(), 0);
    assert!(peer.log.lock().unwrap().creates.is_empty());

    peer.log.lock().unwrap().lose_append_receipt = true;
    fixture
        .invoke("codex", codex.clone(), &peer.base, TOKEN, selected, true)
        .await;
    assert_eq!(fixture.pending().len(), 1);
    fixture
        .invoke("codex", codex, &peer.base, TOKEN, selected, true)
        .await;
    assert!(fixture.pending().is_empty());
    let mut claude = fixture.event("claude-code", "start");
    claude["prompt"] = json!("excluded-prompt-contents");
    fixture
        .invoke("claude-code", claude, &peer.base, TOKEN, None, true)
        .await;

    let log = peer.log.lock().unwrap();
    assert_eq!(log.plugin_scopes, [peer.workspace; 3]);
    assert_eq!(log.appends.len(), 3);
    assert_eq!(log.append_bodies[0], log.append_bodies[1]);
    assert_ne!(log.appends[1].0, log.appends[2].0);
    assert!(!String::from_utf8_lossy(&log.append_bodies[2]).contains("excluded-prompt-contents"));
}
