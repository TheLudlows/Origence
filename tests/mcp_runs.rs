//! Run and private archive contracts over HTTP MCP with synthetic credentials.
use crate::support::{fixture, open_service};
use origence::{
    client::HostClient,
    models::Models,
    storage::{RelationalStore, Scope},
};
use serde_json::{Value, json};
use uuid::Uuid;

struct Peer<'a> {
    endpoint: crate::mcp_http::Peer,
    key: &'a str,
}
impl<'a> Peer<'a> {
    fn new(base: &str, key: &'a str) -> Self {
        Self {
            endpoint: crate::mcp_http::Peer::at(format!("{base}/mcp")),
            key,
        }
    }

    async fn call(&self, name: &str, arguments: Value) -> Result<Value, Value> {
        self.endpoint.call(self.key, None, name, arguments).await
    }
}

#[tokio::test]
async fn mcp_runs_private_retry_rotation_and_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let service = open_service(dir.path(), Models::disabled()).await.unwrap();
    let store = service.engine.relational();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "synthetic MCP")
        .await
        .unwrap();
    let owner = store.issue_key(scope, "writer").await.unwrap();
    let other = store.issue_key(scope, "writer").await.unwrap();
    let admin = store.issue_key(scope, "admin").await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = origence::api::router(service.clone());
    let host = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let a = Peer::new(&base, &owner.token);
    let b = Peer::new(&base, &owner.token);
    let catalog: Value = a
        .endpoint
        .post(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .bearer_auth(&owner.token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    for name in [
        "context_run_create",
        "context_run_report",
        "context_run_prepare",
        "context_run_get",
    ] {
        assert!(
            catalog["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["name"] == name)
        );
    }
    let session = a.call("context_session", json!({})).await.unwrap();
    assert_eq!(session, b.call("context_session", json!({})).await.unwrap());
    let turn = Uuid::new_v4();
    let archive = json!({"turn":{"turn_id":turn,"question":"synthetic question","answer":"synthetic answer","used_evidence":[]}});
    let receipt = a
        .call("context_turn_archive", archive.clone())
        .await
        .unwrap();
    assert_eq!(
        receipt,
        b.call("context_turn_archive", archive).await.unwrap()
    );
    b.call("context_turn_get", json!({"turn_id":turn}))
        .await
        .unwrap();
    // MCP archive -> existing HTTP feedback/Improve; Run feedback is not a substitute.
    let client = HostClient::new(&base, owner.token.clone()).unwrap();
    let sid = session["session_id"].as_str().unwrap();
    let feedback_path = format!("/v1/sessions/{sid}/turns/{turn}/feedback");
    let feedback = json!({"feedback_id":Uuid::new_v4(),"target":{"type":"turn"},"rating":"helpful","comment":"synthetic explicit feedback"});
    let feedback_receipt = client
        .call(
            reqwest::Method::POST,
            &feedback_path,
            Some(&feedback),
            Some("mcp-feedback"),
        )
        .await
        .unwrap();
    assert_eq!(
        feedback_receipt,
        client
            .call(
                reqwest::Method::POST,
                &feedback_path,
                Some(&feedback),
                Some("mcp-feedback")
            )
            .await
            .unwrap()
    );
    let mut changed_feedback = feedback.clone();
    changed_feedback["rating"] = json!("unhelpful");
    assert!(matches!(
        client
            .call(
                reqwest::Method::POST,
                &feedback_path,
                Some(&changed_feedback),
                Some("mcp-feedback")
            )
            .await,
        Err(origence::error::AppError::Conflict(_))
    ));
    let feedback_view = client
        .call(reqwest::Method::GET, &feedback_path, None, None)
        .await
        .unwrap();
    assert_eq!(feedback_view["feedback"].as_array().unwrap().len(), 1);
    let improve_id = Uuid::new_v4();
    let improve_path = format!("/v1/sessions/{sid}/improve");
    let improve =
        json!({"improve_id":improve_id,"conditions":["synthetic fixture only"],"max_feedback":1});
    // Disabled models must fail explicitly, with no accepted improvement persisted.
    assert!(matches!(
        client
            .call(
                reqwest::Method::POST,
                &improve_path,
                Some(&improve),
                Some("mcp-improve")
            )
            .await,
        Err(origence::error::AppError::Unavailable(_))
    ));
    assert!(matches!(
        client
            .call(
                reqwest::Method::GET,
                &format!("/v1/improvements/{improve_id}"),
                None,
                None
            )
            .await,
        Err(origence::error::AppError::NotFound)
    ));
    for token in [&other.token, &admin.token] {
        let outsider = HostClient::new(&base, token.to_string()).unwrap();
        assert!(
            outsider
                .call(reqwest::Method::GET, &feedback_path, None, None)
                .await
                .is_err()
        );
        assert!(
            outsider
                .call(
                    reqwest::Method::POST,
                    &feedback_path,
                    Some(&feedback),
                    Some("outsider-feedback")
                )
                .await
                .is_err()
        );
        assert!(
            outsider
                .call(
                    reqwest::Method::POST,
                    &improve_path,
                    Some(&improve),
                    Some("outsider-improve")
                )
                .await
                .is_err()
        );
    }
    let run = Uuid::new_v4();
    let create = json!({"run_id":run,"summary":"synthetic task"});
    let first = a.call("context_run_create", create.clone()).await.unwrap();
    assert_eq!(
        first,
        b.call("context_run_create", create.clone()).await.unwrap()
    );
    assert!(
        b.call(
            "context_run_create",
            json!({"run_id":run,"summary":"changed"})
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("CONFLICT")
    );
    let separate = Uuid::new_v4();
    b.call(
        "context_run_create",
        json!({"run_id":separate,"summary":"independent task"}),
    )
    .await
    .unwrap();
    let report = json!({"run_id":run,"report":{"event_id":Uuid::new_v4(),"reporter":"test","observed_at_ms":0,"event":{"kind":"coverage","component":"external tools","status":"unknown","summary":"not observed"}}});
    let receipt = a.call("context_run_report", report.clone()).await.unwrap();
    assert_eq!(
        receipt,
        b.call("context_run_report", report.clone()).await.unwrap()
    );
    let mut changed = report.clone();
    changed["report"]["event"]["status"] = json!("partial");
    assert!(
        b.call("context_run_report", changed)
            .await
            .unwrap_err()
            .to_string()
            .contains("CONFLICT")
    );
    let mut forged = report;
    forged["report"]["event"]["kind"] = json!("context_prepared");
    assert!(a.call("context_run_report", forged).await.is_err());
    let read = json!({"run_id":run,"limit":100});
    let trace = b.call("context_run_get", read.clone()).await.unwrap();
    assert!(trace.to_string().contains("unknown"));
    assert_eq!(trace["events"].as_array().unwrap().len(), 1);
    assert!(
        b.call("context_run_get", json!({"run_id":separate}))
            .await
            .unwrap()["events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let workspace_admin = Peer::new(&base, &admin.token);
    let admin_trace = workspace_admin
        .call("context_run_get", read.clone())
        .await
        .unwrap();
    assert_eq!(admin_trace["is_owner"], false);
    assert_eq!(admin_trace["events"], trace["events"]);
    assert!(
        Peer::new(&base, &other.token)
            .call("context_run_get", read.clone())
            .await
            .is_err()
    );
    for token in [&other.token, &admin.token] {
        let denied = Peer::new(&base, token);
        assert!(
            denied
                .call("context_turn_get", json!({"turn_id":turn}))
                .await
                .is_err()
        );
        assert!(
            denied
                .call(
                    "context_run_prepare",
                    json!({"run_id":run,"event_id":Uuid::new_v4(),"context":{"query":"synthetic"}})
                )
                .await
                .is_err()
        );
        assert!(
            denied
                .call("context_run_create", create.clone())
                .await
                .is_err()
        );
        assert!(denied.call("context_run_report",json!({"run_id":run,"report":{"event_id":Uuid::new_v4(),"reporter":"test","observed_at_ms":0,"event":{"kind":"result_reported","outcome":"unknown","summary":"denied"}}})).await.is_err());
    }
    // Every declared client event retains its exact payload and report attribution.
    for event in [
        json!({"kind":"context_reported","references":[]}),
        json!({"kind":"tool_reported","tool":"synthetic tool","summary":"observed fixture call"}),
        json!({"kind":"result_reported","outcome":"unknown","summary":"no real task"}),
        json!({"kind":"metering_reported","duration_ms":125,"agent_tokens":32,"estimated_cost":{"currency":"USD","micros":990}}),
        json!({"kind":"codex_command_reported","status":"failed","exit_code":1,"duration_ms":0}),
        json!({"kind":"claude_interrupt_reported","observation":{"stage":"requested"}}),
        json!({"kind":"claude_interrupt_reported","observation":{"stage":"receipt","status":"acknowledged"}}),
        json!({"kind":"claude_interrupt_reported","observation":{"stage":"result","status":"interrupted"}}),
        json!({"kind":"feedback","outcome":"failure","summary":"synthetic feedback"}),
        json!({"kind":"skill_loaded_reported","skill_id":Uuid::new_v4(),"version":1,"digest":"a".repeat(64)}),
        json!({"kind":"artifact_reported","artifact_id":Uuid::new_v4(),"summary":"synthetic artifact"}),
    ] {
        let report =
            json!({"event_id":Uuid::new_v4(),"reporter":"test","observed_at_ms":0,"event":event});
        a.call("context_run_report", json!({"run_id":run,"report":report}))
            .await
            .unwrap();
        let trace = b.call("context_run_get", read.clone()).await.unwrap();
        let last = trace["events"].as_array().unwrap().last().unwrap();
        assert_eq!(last["report"], report);
        assert_eq!(last["evidence_origin"], "reported");
    }
    let client = HostClient::new(&base, owner.token.clone()).unwrap();
    let catalog = client
        .call(reqwest::Method::GET, "/v1/runs", None, None)
        .await
        .unwrap();
    let row = catalog["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["run_id"] == json!(run))
        .unwrap();
    assert_eq!(row["status"], "unknown");
    let forged = json!({"event_id":Uuid::new_v4(),"reporter":"client","observed_at_ms":0,
        "event":{"kind":"codex_command_reported","status":"failed","aggregated_output":"private"}});
    assert!(
        client
            .call(
                reqwest::Method::POST,
                &format!("/v1/runs/{run}/events"),
                Some(&forged),
                None
            )
            .await
            .is_err()
    );
    let forged = json!({"event_id":Uuid::new_v4(),"reporter":"client","observed_at_ms":0,
        "event":{"kind":"claude_interrupt_reported","observation":{"stage":"receipt","status":"acknowledged","still_queued":["private"]}}});
    assert!(
        client
            .call(
                reqwest::Method::POST,
                &format!("/v1/runs/{run}/events"),
                Some(&forged),
                None
            )
            .await
            .is_err()
    );
    let metering = client
        .call(reqwest::Method::GET, "/v1/runs/overview", None, None)
        .await
        .unwrap();
    assert_eq!(
        metering["metering"],
        json!({
            "duration_p95_ms":125,"duration_runs":1,"agent_tokens":"32","token_runs":1,
            "costs":[{"basis":"estimated","currency":"USD","micros":"990","runs":1}]
        })
    );
    client.call(reqwest::Method::POST, "/v1/memories", Some(&json!({"fact_key":"synthetic.policy","content":"synthetic policy requires review"})), Some("synthetic-mcp-seed")).await.unwrap();
    while origence::worker::process_next(&service).await.unwrap() {}
    let prepare = json!({"run_id":run,"event_id":Uuid::new_v4(),"context":{"query":"synthetic"}});
    let prepared = a
        .call("context_run_prepare", prepare.clone())
        .await
        .unwrap();
    assert_eq!(
        prepared,
        b.call("context_run_prepare", prepare.clone())
            .await
            .unwrap()
    );
    let mut changed = prepare.clone();
    changed["context"]["query"] = json!("different");
    assert!(a.call("context_run_prepare", changed).await.is_err());
    fixture(dir.path(), None).unavailable(true);
    assert!(
        a.call(
            "context_run_prepare",
            json!({"run_id":run,"event_id":Uuid::new_v4(),"context":{"query":"failure"}})
        )
        .await
        .is_err()
    );
    let trace = b.call("context_run_get", read.clone()).await.unwrap();
    assert!(
        trace.to_string().contains("context_preparation_failed"),
        "{trace}"
    );
    let admin_client = HostClient::new(&base, admin.token).unwrap();
    let rotated = admin_client
        .call(
            reqwest::Method::POST,
            &format!("/admin/keys/{}/rotate", owner.key_id),
            None,
            None,
        )
        .await
        .unwrap();
    assert!(a.call("context_run_get", read.clone()).await.is_err());
    let replacement = Peer::new(&base, rotated["token"].as_str().unwrap());
    assert_eq!(
        session,
        replacement
            .call("context_session", json!({}))
            .await
            .unwrap()
    );
    assert_eq!(
        trace,
        replacement
            .call("context_run_get", read.clone())
            .await
            .unwrap()
    );
    assert!(
        replacement
            .call("context_turn_get", json!({"turn_id":turn}))
            .await
            .is_err()
    );
    fixture(dir.path(), None).unavailable(false);
    replacement
        .call("context_turn_get", json!({"turn_id":turn}))
        .await
        .unwrap();
    let asset = prepared["bundle"]["items"][0]["provenance"]["asset_id"]
        .as_str()
        .unwrap();
    admin_client
        .call(
            reqwest::Method::DELETE,
            &format!("/v1/assets/{asset}"),
            None,
            Some("synthetic-withdraw"),
        )
        .await
        .unwrap();
    assert!(
        replacement
            .call("context_run_get", read.clone())
            .await
            .is_err()
    );
    assert!(
        replacement
            .call("context_run_prepare", prepare)
            .await
            .is_err()
    );
    admin_client
        .call(
            reqwest::Method::DELETE,
            &format!("/admin/keys/{}", rotated["key_id"].as_str().unwrap()),
            None,
            None,
        )
        .await
        .unwrap();
    assert!(replacement.call("context_run_get", read).await.is_err());
    assert!(
        replacement
            .call("context_run_create", create)
            .await
            .is_err()
    );
    assert!(
        replacement
            .call("context_turn_get", json!({"turn_id":turn}))
            .await
            .is_err()
    );

    host.abort();
    let _ = host.await;
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn run_using_native_archive_keeps_only_the_cited_turn_proof() {
    use origence::{error::AppError, evidence::EvidenceInput, storage::sqlite_db};

    let dir = tempfile::tempdir().unwrap();
    let service = open_service(dir.path(), Models::disabled()).await.unwrap();
    let store = service.engine.relational();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "synthetic Run proof")
        .await
        .unwrap();
    let admin_key = store.issue_key(scope, "admin").await.unwrap();
    let admin = service.auth(&admin_key.token).await.unwrap();
    let key = store.issue_key(scope, "writer").await.unwrap();
    let owner = service.auth(&key.token).await.unwrap();
    let session = service.private_session(&owner).await.unwrap();
    let session = serde_json::from_value(session["session_id"].clone()).unwrap();
    let turn = Uuid::new_v4();
    let question = "runarchiveproofneedle";
    let answer = "Private answer for only this synthetic Run";
    service
        .archive_turn(
            &owner,
            "native-run-turn",
            session,
            serde_json::from_value(json!({
                "turn_id":turn,"question":question,"answer":answer,"used_evidence":[]
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let rows: i64 = sqlite_db::query_scalar(
        "SELECT count(*) FROM oc_session_turns WHERE tenant_id=? AND workspace_id=? AND turn_id=?",
    )
    .bind(scope.tenant_id)
    .bind(scope.workspace_id)
    .bind(turn)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(rows, 0, "ordinary private saves must avoid the proof table");

    let run = Uuid::new_v4();
    service
        .create_run(
            &owner,
            serde_json::from_value(json!({"run_id":run,"summary":"Synthetic archive replay"}))
                .unwrap(),
        )
        .await
        .unwrap();
    let event = Uuid::new_v4();
    let input: EvidenceInput = serde_json::from_value(json!({
        "query":question,"routes":[{"route":"session","limit":1}]
    }))
    .unwrap();
    let prepared = service
        .prepare_run_context(&owner, run, event, input.clone())
        .await
        .unwrap();
    assert_eq!(
        prepared["bundle"]["items"][0]["provenance"]["turn_id"],
        json!(turn)
    );
    assert!(
        prepared["bundle"]["rendered_context"]
            .as_str()
            .unwrap()
            .contains(answer)
    );
    assert_eq!(
        service
            .prepare_run_context(&owner, run, event, input.clone())
            .await
            .unwrap(),
        prepared
    );
    let persisted: String = sqlite_db::query_scalar(
        "SELECT payload FROM oc_run_events WHERE tenant_id=? AND workspace_id=? AND run_id=? AND event_id=?",
    )
    .bind(scope.tenant_id)
    .bind(scope.workspace_id)
    .bind(run)
    .bind(event)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert!(!persisted.contains(question) && !persisted.contains(answer));
    let source: Uuid = serde_json::from_value(
        prepared["bundle"]["items"][0]["provenance"]["source_event_id"].clone(),
    )
    .unwrap();
    let proof: String = sqlite_db::query_scalar(
        "SELECT content FROM oc_events WHERE tenant_id=? AND workspace_id=? AND id=?",
    )
    .bind(scope.tenant_id)
    .bind(scope.workspace_id)
    .bind(source)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert!(proof.is_empty());
    let owner_trace = service.run_trace(&owner, run, 0, 1).await.unwrap();
    let admin_trace = service.run_trace(&admin, run, 0, 1).await.unwrap();
    assert_eq!(admin_trace["is_owner"], false);
    assert_eq!(admin_trace["events"], owner_trace["events"]);
    assert_eq!(
        admin_trace["events"][0]["report"]["citations"][0]["turn_id"],
        json!(turn)
    );
    assert!(!admin_trace.to_string().contains(answer));
    assert!(
        admin_trace["events"][0]["report"]
            .get("replay_bundle")
            .is_none()
    );
    assert!(matches!(
        service.source_event(&admin, source).await,
        Err(AppError::NotFound)
    ));
    service.run_prefix(&owner, run, 1).await.unwrap();

    // Pause one preparer during shared retrieval. The winner commits before
    // a second private turn is archived, so the late candidate differs.
    service
        .memory(
            &owner,
            "native-run-race-memory",
            serde_json::from_value(json!({
                "fact_key":"synthetic.run_race",
                "content":format!("{question} synthetic shared evidence")
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    while origence::worker::process_next(&service).await.unwrap() {}
    let mixed_event = Uuid::new_v4();
    let mixed: EvidenceInput = serde_json::from_value(json!({
        "query": question,
        "routes":[{"route":"memory"},{"route":"session","limit":2}]
    }))
    .unwrap();
    let gate = fixture(dir.path(), None).pause_next_content_read();
    let delayed = tokio::spawn({
        let service = service.clone();
        let owner = owner.clone();
        let mixed = mixed.clone();
        async move {
            service
                .prepare_run_context(&owner, run, mixed_event, mixed)
                .await
        }
    });
    let timeout = std::time::Duration::from_secs(10);
    tokio::time::timeout(timeout, gate.entered.notified())
        .await
        .expect("delayed Run should reach its shared content read");
    let winner = service
        .prepare_run_context(&owner, run, mixed_event, mixed)
        .await
        .unwrap();
    assert_eq!(winner["bundle"]["items"].as_array().unwrap().len(), 2);
    let uncited_turn = Uuid::new_v4();
    let uncited_answer = "Private synthetic turn excluded from the winning Run";
    service
        .archive_turn(
            &owner,
            "later-native-turn",
            session,
            serde_json::from_value(json!({
                "turn_id":uncited_turn,"question":question,"answer":uncited_answer,"used_evidence":[]
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    gate.resume.notify_one();
    let resumed = tokio::time::timeout(timeout, delayed)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(resumed, winner);
    let uncited_proofs: i64 = sqlite_db::query_scalar(
        "SELECT count(*) FROM oc_session_turns WHERE tenant_id=? AND workspace_id=? AND turn_id=?",
    )
    .bind(scope.tenant_id)
    .bind(scope.workspace_id)
    .bind(uncited_turn)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(uncited_proofs, 0, "the losing bundle must not add a proof");

    service
        .withdraw_session_archive(&owner, session, turn)
        .await
        .unwrap();
    let trace = service.run_trace(&owner, run, 0, 10).await.unwrap();
    assert_eq!(
        trace["events"][0]["report"]["private_sources_unavailable"],
        1
    );
    assert_eq!(trace["events"][0]["references"], json!([]));
    assert_eq!(
        service.run_trace(&admin, run, 0, 10).await.unwrap()["events"],
        trace["events"]
    );
    assert!(matches!(
        service.prepare_run_context(&owner, run, event, input).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        service.run_prefix(&owner, run, 1).await,
        Err(AppError::NotFound)
    ));
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn mcp_runs_reuse_explicit_client_hook_identity_and_enforce_credentials() {
    use crate::client_hooks::HookFixture;
    use origence::client_hook::{Capture, Client};
    use reqwest::Method;

    let dir = tempfile::tempdir().unwrap();
    let service = open_service(dir.path(), Models::disabled()).await.unwrap();
    let store = service.engine.relational();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "hook fixture")
        .await
        .unwrap();
    let writer = store.issue_key(scope, "writer").await.unwrap();
    let reader = store.issue_key(scope, "reader").await.unwrap();
    let admin = store.issue_key(scope, "admin").await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = origence::api::router(service.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let fixture = HookFixture::new();
    fixture.configure(&base, writer.key_id, &["codex", "claude_code"]);

    let mcp = Peer::new(&base, &writer.token);
    assert_eq!(
        fixture.enqueue(Client::Codex, &writer.token, "start"),
        Capture::Queued
    );
    assert_eq!(
        fixture.enqueue(Client::ClaudeCode, &writer.token, "start"),
        Capture::Queued
    );
    let codex = fixture
        .profile()
        .run_identity(Client::Codex, "codex-test-session", "codex-test-turn")
        .unwrap();
    let claude = fixture
        .profile()
        .run_identity(
            Client::ClaudeCode,
            "claude-test-session",
            "550e8400-e29b-41d4-a716-446655440000",
        )
        .unwrap();
    assert_ne!(codex.run_id, claude.run_id);
    // The user/caller explicitly passes matching ID and summary. Either side can create first.
    assert_eq!(
        mcp.call("context_run_create", json!(codex)).await.unwrap()["run_id"],
        codex.run_id.to_string()
    );
    let outcome = fixture.profile().flush(writer.token.clone()).await.unwrap();
    assert_eq!(outcome.sent, 2);
    assert_eq!(
        mcp.call("context_run_create", json!(claude)).await.unwrap()["run_id"],
        claude.run_id.to_string()
    );
    assert_eq!(mcp.call("context_run_report", json!({
        "run_id": claude.run_id,
        "report": {
            "event_id": Uuid::new_v4(), "observed_at_ms": 1, "reporter": "user",
            "event": {"kind":"coverage","component":"synthetic_mcp","status":"partial","summary":"explicitly linked fixture"}
        }
    })).await.unwrap()["sequence"], 2);
    let trace = mcp
        .call("context_run_get", json!({"run_id": claude.run_id}))
        .await
        .unwrap();
    assert_eq!(trace["events"].as_array().unwrap().len(), 2);
    assert_eq!(trace["unreported_activity"], "unknown");

    let owner = HostClient::new(&base, writer.token.clone()).unwrap();
    let admin_client = HostClient::new(&base, admin.token).unwrap();
    let directory = owner
        .call(Method::GET, "/v1/runs", None, None)
        .await
        .unwrap();
    assert_eq!(directory["items"].as_array().unwrap().len(), 2);
    let admin_trace = admin_client.run_trace(claude.run_id, 0, 50).await.unwrap();
    assert_eq!(admin_trace["is_owner"], false);
    assert_eq!(admin_trace["events"].as_array().unwrap().len(), 2);

    fixture.configure(&base, reader.key_id, &["claude_code"]);
    assert_eq!(
        fixture.enqueue(Client::ClaudeCode, &reader.token, "start"),
        Capture::Queued
    );
    let rejected = fixture.profile().flush(reader.token).await.unwrap();
    assert_eq!(rejected.blocked, Some("authorization_denied"));

    fixture.configure(&base, writer.key_id, &["codex", "claude_code"]);
    assert_eq!(
        fixture.enqueue(Client::Codex, &writer.token, "stop"),
        Capture::Queued
    );
    let replacement = admin_client
        .call(
            Method::POST,
            &format!("/admin/keys/{}/rotate", writer.key_id),
            None,
            None,
        )
        .await
        .unwrap()["token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        fixture.profile().flush(writer.token).await.unwrap().blocked,
        Some("authorization_denied")
    );
    let replacement_client = HostClient::new(&base, replacement.clone()).unwrap();
    assert!(
        replacement_client
            .run_trace(codex.run_id, 0, 10)
            .await
            .is_ok()
    );
    assert_eq!(
        fixture
            .profile()
            .flush(replacement.clone())
            .await
            .unwrap()
            .held,
        2
    );
    assert_eq!(
        fixture.enqueue(Client::Codex, &replacement, "stop"),
        Capture::Duplicate
    );
    assert_eq!(
        fixture.enqueue(Client::Codex, &replacement, "shell"),
        Capture::Queued
    );
    let outcome = fixture.profile().flush(replacement).await.unwrap();
    assert_eq!(outcome.sent, 1);
    assert_eq!(outcome.held, 2);
    let trace = replacement_client
        .run_trace(codex.run_id, 0, 10)
        .await
        .unwrap();
    assert_eq!(trace["events"].as_array().unwrap().len(), 2);
    assert_eq!(trace["events"][1]["report"]["event"]["tool"], "Bash");

    server.abort();
    service.engine.shutdown().await.unwrap();
}
