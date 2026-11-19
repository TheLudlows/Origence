//! Redacted client wire fixtures and a local HTTP peer. No product client runs.
use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path as RoutePath, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use origence::client_hook::{Capture, Client, Profile};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Barrier, Mutex},
};
use uuid::Uuid;

const TOKEN: &str = "fixture-only-project-writer-token";
const OTHER_TOKEN: &str = "fixture-only-other-writer-token";

pub(super) struct HookFixture {
    _root: tempfile::TempDir,
    pub project: PathBuf,
    pub config: PathBuf,
}

impl HookFixture {
    pub fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let profile_dir = root.path().join("profiles");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&profile_dir).unwrap();
        private_permissions(&profile_dir, 0o700);
        let config = profile_dir.join("project.json");
        Self {
            _root: root,
            project,
            config,
        }
    }

    pub fn configure(&self, base: &str, key: Uuid, clients: &[&str]) {
        self.write_config(json!({
            "enabled": true, "clients": clients, "project": self.project,
            "server_url": base, "logical_key_id": key, "key_env": "ORIGENCE_TEST_HOOK_KEY"
        }));
    }

    fn write_config(&self, value: Value) {
        fs::write(&self.config, value.to_string()).unwrap();
        private_permissions(&self.config, 0o600);
    }

    pub fn profile(&self) -> Profile {
        Profile::load(&self.config).unwrap().unwrap()
    }

    fn queue_dir(&self) -> PathBuf {
        self.config.with_file_name("project.json.queue")
    }

    fn queue(&self) -> Value {
        serde_json::from_slice(&fs::read(self.queue_dir().join("queue-v1.json")).unwrap()).unwrap()
    }

    pub fn enqueue(&self, client: Client, token: &str, name: &str) -> Capture {
        self.capture(client, token, event(client, name, &self.project))
    }

    fn capture(&self, client: Client, token: &str, input: Value) -> Capture {
        self.profile()
            .enqueue(client, token, &self.project, input.to_string().as_bytes())
            .unwrap()
    }
}

fn private_permissions(path: &Path, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
}

fn event(client: Client, name: &str, cwd: &Path) -> Value {
    let raw = match client {
        Client::Codex => include_str!("fixtures/client-hooks/codex.json"),
        Client::ClaudeCode => include_str!("fixtures/client-hooks/claude-code.json"),
    };
    let values: Value = serde_json::from_str(raw).unwrap();
    let mut input = values[name].clone();
    assert!(input.is_object(), "{name}");
    input["cwd"] = json!(cwd);
    input
}

#[derive(Default)]
struct Requests {
    sessions: usize,
    creates: Vec<Value>,
    appends: Vec<(Uuid, Value)>,
    append_bodies: Vec<Bytes>,
    unavailable: bool,
    lose_append_receipt: bool,
    conflict_create: bool,
    gate: Option<Arc<Gate>>,
}

#[derive(Default)]
struct Gate {
    entered: tokio::sync::Notify,
    resume: tokio::sync::Notify,
}

#[derive(Clone)]
struct HttpFixture {
    base: String,
    key: Uuid,
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
            log: Arc::new(Mutex::new(Requests::default())),
        }
    }

    fn serve(&self, listener: tokio::net::TcpListener) {
        let app = Router::new()
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

async fn session(State(peer): State<HttpFixture>, headers: HeaderMap) -> Reply {
    let mut log = peer.log.lock().unwrap();
    log.sessions += 1;
    if log.unavailable {
        return reply(StatusCode::SERVICE_UNAVAILABLE, json!({}));
    }
    if headers["authorization"].to_str().unwrap() != format!("Bearer {TOKEN}") {
        return reply(StatusCode::UNAUTHORIZED, json!({}));
    }
    reply(StatusCode::OK, json!({"key_id": peer.key}))
}

async fn create(State(peer): State<HttpFixture>, Json(run): Json<Value>) -> Reply {
    let mut log = peer.log.lock().unwrap();
    log.creates.push(run.clone());
    if log.conflict_create {
        return reply(StatusCode::CONFLICT, json!({}));
    }
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
    let (sequence, lose_receipt, gate) = {
        let mut log = peer.log.lock().unwrap();
        log.appends.push((run, report.clone()));
        log.append_bodies.push(body);
        (
            log.appends.len(),
            std::mem::take(&mut log.lose_append_receipt),
            log.gate.take(),
        )
    };
    if let Some(gate) = gate {
        gate.entered.notify_one();
        gate.resume.notified().await;
    }
    if lose_receipt {
        return reply(StatusCode::OK, json!({})); // Accepted, but no verifiable receipt.
    }
    reply(
        StatusCode::OK,
        json!({"run_id": run, "event_id": report["event_id"], "sequence": sequence}),
    )
}

#[tokio::test]
async fn client_hooks_opt_in_and_project_boundary() {
    let peer = HttpFixture::start().await;
    let fixture = HookFixture::new();
    assert!(Profile::load(&fixture.config).unwrap().is_none());

    // Exercise the public CLI without any user credential or valid hook stdin.
    fixture.write_config(json!({"server_url": peer.base}));
    let cli = || {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_origence"));
        command
            .current_dir(&fixture.project)
            .env_remove("ORIGENCE_TEST_HOOK_KEY")
            .args(["client-hook", "--profile"])
            .arg(&fixture.config);
        command
    };
    let capture = cli()
        .args(["capture", "--client", "codex"])
        .output()
        .unwrap();
    assert!(capture.status.success());
    assert!(capture.stderr.is_empty());
    assert!(cli().arg("flush").output().unwrap().status.success());
    assert!(!fixture.queue_dir().exists());

    fixture.configure(&peer.base, peer.key, &["codex"]);
    assert_eq!(
        fixture.enqueue(Client::ClaudeCode, TOKEN, "start"),
        Capture::Ignored
    );
    let nested = fixture.project.join("nested");
    let ordinary = fixture.project.join("src");
    fs::create_dir_all(nested.join(".git")).unwrap();
    fs::create_dir(&ordinary).unwrap();
    for cwd in [fixture.config.parent().unwrap(), nested.as_path()] {
        let input = event(Client::Codex, "start", cwd);
        assert_eq!(
            fixture.capture(Client::Codex, TOKEN, input),
            Capture::Ignored
        );
    }
    assert_eq!(
        fixture
            .profile()
            .enqueue(
                Client::Codex,
                TOKEN,
                &nested,
                event(Client::Codex, "start", &fixture.project)
                    .to_string()
                    .as_bytes(),
            )
            .unwrap(),
        Capture::Ignored
    );
    let mut unreliable = event(Client::Codex, "start", &fixture.project);
    unreliable.as_object_mut().unwrap().remove("turn_id");
    assert_eq!(
        fixture.capture(Client::Codex, TOKEN, unreliable),
        Capture::MissingIdentity
    );
    let mut unknown_tool = event(Client::Codex, "shell", &fixture.project);
    unknown_tool["tool_name"] = json!("FIXTURE_PRIVATE_UNKNOWN_TOOL");
    assert_eq!(
        fixture.capture(Client::Codex, TOKEN, unknown_tool),
        Capture::Ignored
    );
    assert!(
        fixture
            .profile()
            .enqueue(
                Client::Codex,
                TOKEN,
                &fixture.project,
                std::io::repeat(b'X').take(1024 * 1024 + 1),
            )
            .is_err()
    );
    assert!(!fixture.queue_dir().exists());

    use std::io::{Read, Write};
    use std::process::Stdio;
    let mut child = cli()
        .args(["capture", "--client", "codex"])
        .env("ORIGENCE_TEST_HOOK_KEY", TOKEN)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            event(Client::Codex, "start", &ordinary)
                .to_string()
                .as_bytes(),
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"{}\n");
    assert_eq!(fixture.queue()["pending"].as_array().unwrap().len(), 1);
    assert_eq!(peer.log.lock().unwrap().sessions, 0);

    let identity = cli()
        .args([
            "run-id",
            "--client",
            "codex",
            "--session-id",
            "codex-test-session",
            "--correlation-id",
            "codex-test-turn",
        ])
        .output()
        .unwrap();
    let identity: Value = serde_json::from_slice(&identity.stdout).unwrap();
    let mut sender = tokio::process::Command::from(cli());
    let sent = sender
        .arg("flush")
        .env("ORIGENCE_TEST_HOOK_KEY", TOKEN)
        .output()
        .await
        .unwrap();
    assert!(sent.status.success());
    let sent: Value = serde_json::from_slice(&sent.stdout).unwrap();
    assert_eq!(sent["sent"], 1);
    assert_eq!(peer.log.lock().unwrap().creates[0], identity);
}

#[tokio::test]
async fn client_hooks_connection_refused_then_sender_restart() {
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let peer = HttpFixture::new(socket.local_addr().unwrap());
    let fixture = HookFixture::new();
    fixture.configure(&peer.base, peer.key, &["claude_code"]);
    assert_eq!(
        fixture.enqueue(Client::ClaudeCode, TOKEN, "stop"),
        Capture::Queued
    );
    let original = fixture.queue();
    assert_eq!(
        fixture.profile().flush(TOKEN.into()).await.unwrap().blocked,
        Some("host_unavailable")
    );
    assert_eq!(fixture.queue(), original);
    peer.serve(socket.listen(16).unwrap());
    assert_eq!(fixture.profile().flush(TOKEN.into()).await.unwrap().sent, 1);
    assert_eq!(
        peer.log.lock().unwrap().appends[0].1["event_id"],
        original["pending"][0]["event_id"]
    );
}

#[tokio::test]
async fn client_hooks_fixture_reports_preserve_observed_outcomes_and_privacy() {
    let peer = HttpFixture::start().await;
    let fixture = HookFixture::new();
    fixture.configure(&peer.base, peer.key, &["codex", "claude_code"]);
    // Stops and tool signals may arrive before a start. Omitted activity stays unknown.
    let cases = [
        (Client::Codex, "stop"),
        (Client::Codex, "shell"),
        (Client::Codex, "start"),
        (Client::Codex, "mcp_error"),
        (Client::Codex, "interrupt"),
        (Client::ClaudeCode, "tool_failure"),
        (Client::ClaudeCode, "start"),
        (Client::ClaudeCode, "tool_completed"),
        (Client::ClaudeCode, "tool_abort"),
        (Client::ClaudeCode, "subagent_start"),
        (Client::ClaudeCode, "subagent_stop"),
        (Client::ClaudeCode, "stop"),
        (Client::ClaudeCode, "api_error"),
    ];
    for (client, name) in cases {
        assert_eq!(fixture.enqueue(client, TOKEN, name), Capture::Queued);
        assert_eq!(fixture.enqueue(client, TOKEN, name), Capture::Duplicate);
    }
    let mut conflicting_tool = event(Client::ClaudeCode, "tool_failure", &fixture.project);
    conflicting_tool["hook_event_name"] = json!("PostToolUse");
    assert_eq!(
        fixture.capture(Client::ClaudeCode, TOKEN, conflicting_tool),
        Capture::Duplicate
    );
    let mut unbound = event(Client::ClaudeCode, "stop", &fixture.project);
    unbound.as_object_mut().unwrap().remove("prompt_id");
    unbound["turn_id"] = json!("550e8400-e29b-41d4-a716-446655440002");
    assert_eq!(
        fixture.capture(Client::ClaudeCode, TOKEN, unbound),
        Capture::MissingIdentity
    );
    let mut unidentified_agent = event(Client::ClaudeCode, "subagent_stop", &fixture.project);
    unidentified_agent
        .as_object_mut()
        .unwrap()
        .remove("agent_id");
    assert_eq!(
        fixture.capture(Client::ClaudeCode, TOKEN, unidentified_agent),
        Capture::MissingIdentity
    );
    let mut nested_tool = event(Client::ClaudeCode, "tool_completed", &fixture.project);
    nested_tool["agent_id"] = json!("fixture-claude-agent");
    assert_eq!(
        fixture.capture(Client::ClaudeCode, TOKEN, nested_tool),
        Capture::Ignored
    );
    assert_eq!(
        fixture.enqueue(Client::ClaudeCode, TOKEN, "session_end"),
        Capture::Ignored
    );
    let queue = fixture.queue().to_string();
    for secret in [
        "FIXTURE_PRIVATE",
        TOKEN,
        "fixture-claude-agent",
        "claude-test-session",
        "codex-test-turn",
        "550e8400",
        "project",
        "tool_response",
        "tool_input",
        "transcript_path",
    ] {
        assert!(!queue.contains(secret), "{secret}");
    }

    assert_eq!(
        fixture.profile().flush(TOKEN.into()).await.unwrap().sent,
        cases.len()
    );
    assert_eq!(
        fixture.enqueue(Client::Codex, TOKEN, "stop"),
        Capture::Duplicate
    );
    assert_eq!(
        fixture.enqueue(Client::ClaudeCode, TOKEN, "tool_failure"),
        Capture::Duplicate
    );
    let log = peer.log.lock().unwrap();
    assert_eq!(log.creates.len(), 4);
    assert_eq!(log.appends.len(), cases.len());
    let distinct: std::collections::HashSet<_> = log.appends.iter().map(|(run, _)| run).collect();
    assert_eq!(distinct.len(), 4);
    assert_eq!(log.appends[0].0, log.appends[2].0);
    assert_eq!(log.appends[5].0, log.appends[11].0);
    let reports: Vec<_> = log.appends.iter().map(|(_, v)| &v["event"]).collect();
    for index in [0, 4, 11, 12] {
        assert_eq!(reports[index]["outcome"], "unknown");
    }
    assert!(reports[0]["summary"].as_str().unwrap().contains("continue"));
    assert!(
        reports[1]["summary"]
            .as_str()
            .unwrap()
            .contains("status unknown")
    );
    assert!(
        reports[3]["summary"]
            .as_str()
            .unwrap()
            .contains("tool error")
    );
    assert!(
        reports[4]["summary"]
            .as_str()
            .unwrap()
            .contains("interruption")
    );
    assert!(
        reports[5]["summary"]
            .as_str()
            .unwrap()
            .contains("tool failure. Tool duration: 4187 ms.")
    );
    assert!(
        reports[7]["summary"]
            .as_str()
            .unwrap()
            .contains("completed successfully. Tool duration: 12 ms.")
    );
    assert!(
        reports[8]["summary"]
            .as_str()
            .unwrap()
            .ends_with("failure as an abort.")
    );
    assert_eq!(reports[9]["status"], "partial");
    assert_eq!(reports[10]["status"], "partial");
    assert!(
        reports[12]["summary"]
            .as_str()
            .unwrap()
            .contains("API error")
    );
    for (_, value) in &log.appends {
        let report = value.to_string();
        assert!(!report.contains("FIXTURE_PRIVATE"));
        assert!(!report.contains("wall time"));
        assert!(!report.contains("123 ms"));
        assert!(!report.contains("0 ms"));
        assert_eq!(value["reporter"], "client");
        assert!(value["observed_at_ms"].as_u64().unwrap() > 0);
    }
}

#[tokio::test]
async fn client_hooks_retry_immutable_payloads_and_hold_other_bindings() {
    let peer = HttpFixture::start().await;
    let alternate = HttpFixture::start().await;
    let fixture = HookFixture::new();
    fixture.configure(&peer.base, peer.key, &["codex", "claude_code"]);
    let codex = fixture
        .profile()
        .run_identity(Client::Codex, "codex-test-session", "codex-test-turn")
        .unwrap();
    let other_project = HookFixture::new();
    other_project.configure(&peer.base, peer.key, &["codex"]);
    assert_ne!(
        other_project
            .profile()
            .run_identity(Client::Codex, "codex-test-session", "codex-test-turn")
            .unwrap(),
        codex
    );

    assert_eq!(
        fixture.enqueue(Client::Codex, TOKEN, "shell"),
        Capture::Queued
    );
    assert_eq!(
        fixture.enqueue(Client::ClaudeCode, TOKEN, "tool_failure"),
        Capture::Queued
    );
    let original = fixture.queue();
    assert_eq!(
        fixture
            .profile()
            .flush(OTHER_TOKEN.into())
            .await
            .unwrap()
            .held,
        2
    );
    fixture.configure(&alternate.base, peer.key, &["codex", "claude_code"]);
    assert_eq!(fixture.profile().flush(TOKEN.into()).await.unwrap().held, 2);
    assert_eq!(alternate.log.lock().unwrap().sessions, 0);
    fixture.configure(&peer.base, alternate.key, &["codex", "claude_code"]);
    assert_eq!(fixture.profile().flush(TOKEN.into()).await.unwrap().held, 2);
    assert_eq!(peer.log.lock().unwrap().sessions, 0);
    // New observations with a misconfigured logical key are checked server-side.
    assert_eq!(
        fixture.enqueue(Client::Codex, TOKEN, "start"),
        Capture::Queued
    );
    assert_eq!(
        fixture.profile().flush(TOKEN.into()).await.unwrap().blocked,
        Some("logical_key_mismatch")
    );
    assert!(peer.log.lock().unwrap().creates.is_empty());

    fixture.configure(&peer.base, peer.key, &["codex"]);
    peer.log.lock().unwrap().unavailable = true;
    let report = fixture.profile().flush(TOKEN.into()).await.unwrap();
    assert_eq!(report.blocked, Some("host_unavailable"));
    assert_eq!(report.held, 2);
    assert_eq!(fixture.queue()["pending"][0], original["pending"][0]);
    {
        let mut log = peer.log.lock().unwrap();
        log.unavailable = false;
        log.lose_append_receipt = true;
    }
    assert_eq!(
        fixture.profile().flush(TOKEN.into()).await.unwrap().blocked,
        Some("host_unavailable")
    );
    assert_eq!(fixture.queue()["pending"][0], original["pending"][0]);
    assert_eq!(fixture.profile().flush(TOKEN.into()).await.unwrap().sent, 1);
    {
        let log = peer.log.lock().unwrap();
        assert_eq!(log.appends[0], log.appends[1]);
        assert_eq!(log.append_bodies[0], log.append_bodies[1]);
        assert_eq!(log.creates[0], log.creates[1]);
        assert_eq!(log.creates[0], json!(codex));
    }
    assert_eq!(fixture.queue()["pending"].as_array().unwrap().len(), 2);

    fixture.configure(&peer.base, peer.key, &["codex", "claude_code"]);
    peer.log.lock().unwrap().conflict_create = true;
    assert_eq!(
        fixture
            .profile()
            .flush(TOKEN.into())
            .await
            .unwrap()
            .discarded,
        1
    );
    assert_eq!(peer.log.lock().unwrap().appends.len(), 2);
    assert_eq!(fixture.queue()["pending"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn client_hooks_bound_queue_concurrent_capture_and_separate_sender_lock() {
    let peer = HttpFixture::start().await;
    let fixture = HookFixture::new();
    fixture.configure(&peer.base, peer.key, &["codex"]);
    let profile = Arc::new(fixture.profile());
    let data = event(Client::Codex, "start", &fixture.project).to_string();
    let barrier = Barrier::new(8);
    let capture_start = || profile.enqueue(Client::Codex, TOKEN, &fixture.project, data.as_bytes());
    let results = std::thread::scope(|scope| {
        let threads: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    capture_start()
                })
            })
            .collect();
        threads
            .into_iter()
            .map(|t| t.join().unwrap())
            .collect::<Vec<_>>()
    });
    let mut queued = 0;
    for result in results {
        match result {
            Ok(Capture::Queued) => queued += 1,
            Ok(Capture::Duplicate) => {}
            Err(error) if error.to_string() == "hook queue busy" => {
                // Every competing capture has finished. The same event must now deduplicate.
                assert_eq!(capture_start().unwrap(), Capture::Duplicate);
            }
            other => panic!("unexpected concurrent capture outcome: {other:?}"),
        }
    }
    assert_eq!(queued, 1);

    let before = fixture.queue();
    assert_eq!(before["pending"].as_array().unwrap().len(), 1);
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(fixture.queue_dir().join("queue.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let stop = event(Client::Codex, "stop", &fixture.project).to_string();
    let error = profile
        .enqueue(Client::Codex, TOKEN, &fixture.project, stop.as_bytes())
        .unwrap_err();
    assert_eq!(error.to_string(), "hook queue busy");
    assert_eq!(fixture.queue(), before);
    drop(lock);

    let gate = Arc::new(Gate::default());
    peer.log.lock().unwrap().gate = Some(gate.clone());
    let profile = fixture.profile();
    let mut sender = tokio::spawn(profile.flush(TOKEN.into()));
    tokio::select! {
        () = gate.entered.notified() => {}
        outcome = &mut sender => panic!("sender finished before the fixture append: {outcome:?}"),
    }
    assert_eq!(fixture.profile().flush(TOKEN.into()).await.unwrap().sent, 0);
    assert_eq!(
        fixture.enqueue(Client::Codex, TOKEN, "stop"),
        Capture::Queued
    );
    gate.resume.notify_one();
    assert_eq!(sender.await.unwrap().unwrap().sent, 1);

    let profile = fixture.profile();
    let mut input = event(Client::Codex, "shell", &fixture.project);
    for n in 0..255 {
        input["tool_use_id"] = json!(format!("bounded-tool-{n}"));
        assert_eq!(
            profile
                .enqueue(
                    Client::Codex,
                    TOKEN,
                    &fixture.project,
                    input.to_string().as_bytes()
                )
                .unwrap(),
            Capture::Queued
        );
    }
    input["tool_use_id"] = json!("exceeds-queue");
    assert_eq!(fixture.capture(Client::Codex, TOKEN, input), Capture::Full);
    assert_eq!(fixture.queue()["pending"].as_array().unwrap().len(), 256);
    assert_eq!(
        fixture.enqueue(Client::Codex, TOKEN, "stop"),
        Capture::Duplicate
    );
    assert_eq!(
        fixture.profile().flush(TOKEN.into()).await.unwrap().sent,
        16
    );
    assert_eq!(fixture.queue()["pending"].as_array().unwrap().len(), 240);
}
