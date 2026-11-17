//! Loopback HTTP regression for private Run → selected review → native Skill export.
use crate::support::open_service;
use origence::{
    client::HostClient,
    client_skill::{SkillClient, export_skill},
    models::Models,
    storage::{RelationalStore, Scope},
};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn private_run_to_reviewed_skill_never_discloses_raw_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let service = open_service(dir.path(), Models::disabled()).await.unwrap();
    let store = service.engine.relational();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "foundation HTTP")
        .await
        .unwrap();
    let writer = store.issue_key(scope, "writer").await.unwrap();
    let reviewer = store.issue_key(scope, "reviewer").await.unwrap();
    let reader = store.issue_key(scope, "reader").await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = origence::api::router(service.clone());
    let host = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = |key: &str| HostClient::new(&base, key.to_string()).unwrap();
    let writer_client = client(&writer.token);
    let reviewer_client = client(&reviewer.token);
    let reader_client = client(&reader.token);
    let run = Uuid::new_v4();
    let run_path = format!("/v1/runs/{run}");
    let create = json!({"run_id":run,"summary":"private original task"});
    writer_client
        .call(Method::POST, "/v1/runs", Some(&create), None)
        .await
        .unwrap();
    let http_session = writer_client
        .call(Method::GET, "/v1/session", None, None)
        .await
        .unwrap();
    assert_eq!(http_session, session_from_stdio(&base, &writer.token).await);
    assert!(matches!(
        reviewer_client
            .call(Method::GET, &run_path, None, None)
            .await,
        Err(origence::error::AppError::NotFound)
    ));
    let context = json!({"event_id":Uuid::new_v4(),"context":{"query":"search guidance"}});
    let preparation = writer_client
        .call(
            Method::POST,
            &format!("{run_path}/context"),
            Some(&context),
            None,
        )
        .await
        .unwrap();
    assert_eq!(preparation["preparation"]["sequence"], 1);
    let prefix = writer_client
        .call(
            Method::GET,
            &format!("{run_path}/prefix?through_sequence=1"),
            None,
            None,
        )
        .await
        .unwrap();
    let candidate = Uuid::new_v4();
    let candidate_path = format!("/v1/skill-candidates/{candidate}");
    let body = json!({
        "expected_revision":0,
        "candidate":{
            "draft":{"name":"reviewed-runbook","description":"Check a service","body":"Check inputs and verify the result."},
            "sources":[{"kind":"run","run_id":run,"through_sequence":1,"digest":prefix["digest"]}],
            "private_notes":"private scratch notes"
        }
    });
    writer_client
        .call(Method::PUT, &candidate_path, Some(&body), None)
        .await
        .unwrap();
    let skill = Uuid::new_v4();
    let review = Uuid::new_v4();
    let review_input = json!({
        "revision":1, "reviewer_id":reviewer.key_id, "basis":"Redacted selected evidence",
        "target":{"tenant_id":scope.tenant_id,"workspace_id":scope.workspace_id,"skill_id":skill,"version":1}
    });
    writer_client
        .call(
            Method::POST,
            &format!("{candidate_path}/reviews/{review}"),
            Some(&review_input),
            None,
        )
        .await
        .unwrap();
    assert!(matches!(
        reviewer_client
            .call(Method::GET, &candidate_path, None, None)
            .await,
        Err(origence::error::AppError::NotFound)
    ));
    let review_path = format!("/v1/skill-reviews/{review}");
    let packet = reviewer_client
        .call(Method::GET, &review_path, None, None)
        .await
        .unwrap();
    assert_eq!(packet["packet"]["basis"], "Redacted selected evidence");
    assert!(!packet.to_string().contains("private scratch notes"));
    assert!(!packet.to_string().contains("private original task"));
    let digest = &packet["packet"]["digest"];
    let exact = json!({"digest":digest});
    assert!(matches!(
        reviewer_client
            .call(
                Method::POST,
                &format!("{review_path}/publish"),
                Some(&exact),
                None
            )
            .await,
        Err(origence::error::AppError::Conflict(_))
    ));
    reviewer_client
        .call(
            Method::POST,
            &format!("{review_path}/decision"),
            Some(&json!({"digest":digest,"approve":true})),
            None,
        )
        .await
        .unwrap();
    let published = reviewer_client
        .call(
            Method::POST,
            &format!("{review_path}/publish"),
            Some(&exact),
            None,
        )
        .await
        .unwrap();
    assert_eq!(published["manifest"]["skill_id"], skill.to_string());
    assert!(!published.to_string().contains("Redacted selected evidence"));

    let project = tempfile::tempdir().unwrap();
    let wrong = Scope {
        workspace_id: Uuid::new_v4(),
        ..scope
    };
    assert!(matches!(
        export_skill(
            &reader_client,
            SkillClient::Codex,
            project.path(),
            wrong,
            skill,
            1
        )
        .await,
        Err(origence::error::AppError::Forbidden)
    ));
    let installed = export_skill(
        &reader_client,
        SkillClient::Codex,
        project.path(),
        scope,
        skill,
        1,
    )
    .await
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(installed.join("SKILL.md")).unwrap(),
        published["files"]["SKILL.md"].as_str().unwrap()
    );
    let response = reqwest::Client::new()
        .get(format!("{base}/v1/skills/{skill}/versions/1/package"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.json::<Value>().await.unwrap()["error"]["code"],
        "UNAUTHENTICATED"
    );
    host.abort();
    let _ = host.await;
    service.engine.shutdown().await.unwrap();
}

// Actual CLI transport; no provider account or native model client is needed.
async fn session_from_stdio(base: &str, key: &str) -> Value {
    use std::{process::Stdio, time::Duration};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_origence"))
            .args(["--server-url", base, "mcp"])
            .env("OC_API_KEY", key)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
        let initial = json!({
            "jsonrpc":"2.0","id":1,"method":"initialize","params":{
                "protocolVersion":"2025-03-26","capabilities":{},
                "clientInfo":{"name":"origence-foundation-fixture","version":"1"}
            }
        });
        input
            .write_all(format!("{initial}\n").as_bytes())
            .await
            .unwrap();
        let info: Value =
            serde_json::from_str(&output.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(info["result"]["protocolVersion"], "2025-03-26");
        let initialized = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
        let params = json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"context_session","arguments":{}}});
        input
            .write_all(format!("{initialized}\n{params}\n").as_bytes())
            .await
            .unwrap();
        let response: Value =
            serde_json::from_str(&output.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(response["id"], 2);
        let session =
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        drop(input);
        assert!(child.wait().await.unwrap().success());
        session
    })
    .await
    .expect("stdio MCP test exceeded its time budget")
}
