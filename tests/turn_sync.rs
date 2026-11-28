//! Exercise installed hook commands and the real private archive API with synthetic text.
use crate::fixture_keys::FixtureKeys;
use axum::{
    Json,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use origence::{client::HostClient, error::AppError, models::Models, storage::Scope};
use reqwest::Method;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Output, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use super::agent_hooks::{HOOK_ENTRY, HookFixture};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Submission {
    key: String,
    body: Vec<u8>,
}

#[derive(Default)]
struct Wire {
    lost_receipt: AtomicBool,
    unavailable: AtomicBool,
    posts: Mutex<Vec<Submission>>,
}

async fn archive_wire(State(wire): State<Arc<Wire>>, request: Request, next: Next) -> Response {
    if wire.unavailable.load(Ordering::SeqCst) {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({}))).into_response();
    }
    let archive = request.method() == Method::POST && request.uri().path().ends_with("/turns");
    if !archive {
        return next.run(request).await;
    }
    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, 1024 * 1024).await.unwrap();
    wire.posts.lock().unwrap().push(Submission {
        key: parts.headers["idempotency-key"].to_str().unwrap().into(),
        body: bytes.to_vec(),
    });
    let response = next
        .run(Request::from_parts(parts, Body::from(bytes)))
        .await;
    if response.status().is_success() && wire.lost_receipt.swap(false, Ordering::SeqCst) {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({}))).into_response();
    }
    response
}

#[derive(Clone)]
struct Plugin<'a> {
    project: &'a Path,
    data: PathBuf,
    base: &'a str,
    token: &'a str,
    workspace: Uuid,
    client: &'static str,
}

impl Plugin<'_> {
    async fn invoke(&self, event: Value, enabled: bool) -> Output {
        self.invoke_from(self.project, event, enabled.then_some(self.project))
            .await
    }

    async fn invoke_from(&self, cwd: &Path, event: Value, project: Option<&Path>) -> Output {
        let mut command = tokio::process::Command::new("node");
        command
            .current_dir(cwd)
            .env("OC_API_KEY", self.token)
            .env("OC_WORKSPACE_ID", self.workspace.to_string())
            .env("OC_SERVER_URL", self.base)
            .env("OC_PLUGIN_RUN_HOOK", "0")
            .env("PLUGIN_DATA", &self.data)
            .env("CLAUDE_PLUGIN_DATA", &self.data)
            .env_remove("OC_TURN_SYNC_PROJECT")
            .args([
                "--disable-warning=ExperimentalWarning",
                HOOK_ENTRY,
                self.client,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(project) = project {
            command.env("OC_TURN_SYNC_PROJECT", project);
        }
        let mut child = command.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(event.to_string().as_bytes())
            .await
            .unwrap();
        let output = child.wait_with_output().await.unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"{}\n");
        output
    }

    fn event(&self, event: &str, turn: &str) -> Value {
        let mut value = json!({
            "session_id": "synthetic-native-session",
            "hook_event_name": event,
            "transcript_path": "IGNORED_TRANSCRIPT",
            "tool_input": {"command":"IGNORED_RAW_COMMAND"},
            "tool_response": "IGNORED_TOOL_OUTPUT"
        });
        value[if self.client == "codex" {
            "turn_id"
        } else {
            "prompt_id"
        }] = json!(turn);
        value
    }

    fn open(&self, turn: &str, question: &str) -> Value {
        let mut event = self.event("UserPromptSubmit", turn);
        event["prompt"] = json!(question);
        event
    }

    fn stop(&self, turn: &str, answer: &str) -> Value {
        let mut event = self.event("Stop", turn);
        event["last_assistant_message"] = json!(answer);
        event
    }

    fn saved(&self) -> String {
        let queue = fs::read_dir(&self.data)
            .unwrap()
            .map(|entry| entry.unwrap().path().join("turns-v1.json"))
            .find(|file| file.is_file())
            .unwrap();
        fs::read_to_string(queue).unwrap()
    }
}

fn uuid(value: &Value, field: &str) -> Uuid {
    serde_json::from_value(value[field].clone()).unwrap()
}

#[tokio::test]
async fn turn_sync_pairs_both_clients_and_replays_lost_receipts_without_leaking_scope() {
    let db = tempfile::tempdir().unwrap();
    let service = super::support::open_service(db.path(), Models::disabled())
        .await
        .unwrap();
    let store = service.engine.relational();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "synthetic turn sync")
        .await
        .unwrap();
    let owner = store.seed_workspace_key(scope).await.unwrap();
    let other = store
        .create_key_workspace(
            Scope {
                workspace_id: Uuid::new_v4(),
                ..scope
            },
            "Other workspace",
        )
        .await
        .unwrap();
    let adjacent = Scope {
        workspace_id: Uuid::new_v4(),
        ..scope
    };
    store
        .create_workspace(adjacent.tenant_id, adjacent.workspace_id, "other workspace")
        .await
        .unwrap();

    let wire = Arc::new(Wire::default());
    let app = origence::api::router(service.clone())
        .layer(middleware::from_fn_with_state(wire.clone(), archive_wire));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let fixture = HookFixture::new();
    let codex = Plugin {
        project: &fixture.project,
        data: fixture.data.join("codex-data"),
        base: &base,
        token: &owner.token,
        workspace: scope.workspace_id,
        client: "codex",
    };
    let claude = Plugin {
        data: fixture.data.join("claude-data"),
        client: "claude-code",
        ..codex
    };
    codex
        .invoke(codex.open("turn-1", "disabled prompt"), false)
        .await;
    let paused = codex
        .invoke_from(
            codex.project,
            codex.open("turn-1", "paused prompt"),
            Some(Path::new("")),
        )
        .await;
    assert!(paused.stderr.is_empty());
    codex
        .invoke_from(
            fixture.data.as_path(),
            codex.open("turn-1", "prompt outside the opted-in project"),
            Some(codex.project),
        )
        .await;
    assert!(!codex.data.exists());

    let question = format!(
        "中文测试 <private>PRIVATE_QUESTION</private> {}",
        owner.token
    );
    assert!(
        codex
            .invoke(codex.open("turn-1", &question), true)
            .await
            .stderr
            .is_empty()
    );
    // Stop can arrive before the transcript is written. Pair the documented hook fields.
    let missing = codex.invoke(codex.event("Stop", "turn-1"), true).await;
    assert!(String::from_utf8_lossy(&missing.stderr).contains("completed answer was unavailable"));
    assert!(wire.posts.lock().unwrap().is_empty());
    let stop = codex.stop(
        "turn-1",
        "final answer\nAuthorization: Bearer PRIVATE_REPLY",
    );
    wire.unavailable.store(true, Ordering::SeqCst);
    assert!(
        String::from_utf8_lossy(&codex.invoke(stop.clone(), true).await.stderr)
            .contains("delivery deferred")
    );
    assert!(wire.posts.lock().unwrap().is_empty());
    let waiting = codex.saved();
    assert!(waiting.contains("final answer"));
    for secret in [
        "PRIVATE_QUESTION",
        "PRIVATE_REPLY",
        &owner.token,
        "IGNORED_",
        "synthetic-native-session",
    ] {
        assert!(!waiting.contains(secret));
    }

    wire.unavailable.store(false, Ordering::SeqCst);
    let different_workspace = Plugin {
        workspace: adjacent.workspace_id,
        ..codex.clone()
    };
    let different_key = Plugin {
        token: &other.token,
        ..codex.clone()
    };
    different_workspace.invoke(stop.clone(), true).await;
    different_key.invoke(stop.clone(), true).await;
    assert!(wire.posts.lock().unwrap().is_empty());
    assert_eq!(codex.saved(), waiting);
    wire.lost_receipt.store(true, Ordering::SeqCst);
    assert!(
        String::from_utf8_lossy(&codex.invoke(stop.clone(), true).await.stderr)
            .contains("delivery deferred")
    );
    assert_eq!(codex.saved(), waiting);
    let (a, b) = tokio::join!(codex.invoke(stop.clone(), true), codex.invoke(stop, true));
    assert!(a.stderr.is_empty() && b.stderr.is_empty());
    let first = {
        let posts = wire.posts.lock().unwrap();
        assert_eq!(posts.len(), 2);
        assert_eq!(posts[0], posts[1]);
        serde_json::from_slice::<Value>(&posts[1].body).unwrap()
    };
    assert_eq!(first["question"], "中文测试 [redacted] [redacted]");
    assert_eq!(first["answer"], "final answer\n[redacted]");
    assert_eq!(first["used_evidence"], json!([]));
    assert_eq!(first["evidence_use"], "unknown");
    assert!(!codex.saved().contains("final answer"));
    let continuation = codex
        .invoke(codex.stop("turn-1", "changed after Stop"), true)
        .await;
    assert!(String::from_utf8_lossy(&continuation.stderr).contains("first answer kept"));
    assert_eq!(wire.posts.lock().unwrap().len(), 2);

    assert!(
        claude
            .invoke(claude.open("turn-1", "Claude prompt"), true)
            .await
            .stderr
            .is_empty()
    );
    assert!(
        claude
            .invoke(claude.stop("turn-1", "Claude answer"), true)
            .await
            .stderr
            .is_empty()
    );
    let second: Value = serde_json::from_slice(&wire.posts.lock().unwrap()[2].body).unwrap();
    assert_ne!(first["turn_id"], second["turn_id"]);
    claude
        .invoke(claude.open("turn-2", "interrupted question"), true)
        .await;
    claude
        .invoke(claude.event("StopFailure", "turn-2"), true)
        .await;
    claude
        .invoke(claude.stop("turn-2", "late failed reply"), true)
        .await;
    assert_eq!(wire.posts.lock().unwrap().len(), 3);

    let authorized = HostClient::new(&base, owner.token.clone())
        .unwrap()
        .with_workspace(scope.workspace_id);
    let unauthorized = HostClient::new(&base, other.token.clone())
        .unwrap()
        .with_workspace(scope.workspace_id);
    let private = authorized
        .call(Method::GET, "/v1/session", None, None)
        .await
        .unwrap();
    let session = uuid(&private, "session_id");
    let path = |turn: &Value| format!("/v1/sessions/{session}/turns/{}", uuid(turn, "turn_id"));
    let first_view = authorized
        .call(Method::GET, &path(&first), None, None)
        .await
        .unwrap();
    let second_view = authorized
        .call(Method::GET, &path(&second), None, None)
        .await
        .unwrap();
    assert_eq!(first_view["evidence_use"], "unknown");
    assert_eq!(second_view["answer"], "Claude answer");
    let bundle = authorized
        .call(
            Method::POST,
            "/v1/context",
            Some(&json!({
                "query": "Claude prompt", "mode": "keyword", "budget": 4096,
                "routes": [{"route": "session", "limit": 4, "max_bytes": 4096}]
            })),
            None,
        )
        .await
        .unwrap();
    assert_eq!(bundle["items"][0]["provenance"]["evidence_use"], "unknown");
    assert!(matches!(
        unauthorized
            .call(Method::GET, &path(&first), None, None)
            .await,
        Err(AppError::NotFound | AppError::Forbidden)
    ));

    let owner_auth = service.auth(&owner.token).await.unwrap();
    assert!(matches!(
        service
            .delete(
                &owner_auth,
                "withdraw-synthetic-turn",
                uuid(&first_view, "source_event_id"),
                "event"
            )
            .await,
        Err(AppError::Forbidden)
    ));
    let still_saved = authorized
        .call(Method::GET, &path(&first), None, None)
        .await
        .unwrap();
    assert_eq!(still_saved, first_view);
    assert!(
        authorized
            .call(Method::GET, &path(&second), None, None)
            .await
            .is_ok()
    );
    server.abort();
    service.engine.shutdown().await.unwrap();
}
