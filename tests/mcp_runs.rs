//! Synthetic stdio clients, not Claude Code/Codex task acceptance.
use crate::support::{fixture, open_service};
use origence::{
    client::HostClient,
    models::Models,
    storage::{RelationalStore, Scope},
};
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use uuid::Uuid;
struct Peer {
    child: tokio::process::Child,
    input: tokio::process::ChildStdin,
    output: Lines<BufReader<tokio::process::ChildStdout>>,
    id: u64,
}
impl Peer {
    async fn start(base: &str, key: &str) -> Self {
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_origence"))
            .args(["--server-url", base, "mcp"])
            .env("OC_API_KEY", key)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut p = Self {
            input: child.stdin.take().unwrap(),
            output: BufReader::new(child.stdout.take().unwrap()).lines(),
            child,
            id: 0,
        };
        let reply = p.rpc("initialize",json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"synthetic-protocol-peer","version":"1"}})).await;
        assert!(reply.get("result").is_some(), "{reply}");
        p.input
            .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
            .await
            .unwrap();
        p
    }
    async fn rpc(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let msg = json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params});
        self.input
            .write_all(format!("{msg}\n").as_bytes())
            .await
            .unwrap();
        serde_json::from_str(
            &tokio::time::timeout(Duration::from_secs(20), self.output.next_line())
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
        )
        .unwrap()
    }
    async fn call(&mut self, name: &str, arguments: Value) -> Result<Value, Value> {
        let v = self
            .rpc("tools/call", json!({"name":name,"arguments":arguments}))
            .await;
        if v.get("error").is_some() {
            return Err(v["error"].clone());
        }
        assert_ne!(v["result"]["isError"], true, "{v}");
        Ok(serde_json::from_str(v["result"]["content"][0]["text"].as_str().unwrap()).unwrap())
    }
    async fn stop(mut self) {
        drop(self.input);
        assert!(
            tokio::time::timeout(Duration::from_secs(10), self.child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
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
    let mut a = Peer::start(&base, &owner.token).await;
    let mut b = Peer::start(&base, &owner.token).await;
    let catalog = a.rpc("tools/list", json!({})).await;
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
    for token in [&other.token, &admin.token] {
        let mut denied = Peer::start(&base, token).await;
        assert!(denied.call("context_run_get", read.clone()).await.is_err());
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
        denied.stop().await;
    }
    // Every declared client event retains its exact payload and report attribution.
    for event in [
        json!({"kind":"context_reported","references":[]}),
        json!({"kind":"tool_reported","tool":"synthetic tool","summary":"observed fixture call"}),
        json!({"kind":"result_reported","outcome":"unknown","summary":"no real task"}),
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
    let mut replacement = Peer::start(&base, rotated["token"].as_str().unwrap()).await;
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
    replacement.stop().await;
    a.stop().await;
    b.stop().await;
    host.abort();
    let _ = host.await;
    service.engine.shutdown().await.unwrap();
}
