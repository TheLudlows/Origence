//! Actual standalone CLI against loopback HTTP and a fresh SQLite authority.
mod support;
use origence::{
    client::HostClient,
    models::Models,
    storage::{RelationalStore, Scope},
};
use reqwest::Method;
use serde_json::{Value, json};
use uuid::Uuid;

async fn cli(base: &str, key: &str, args: &[&str], success: bool) -> Value {
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_origence-skill"))
        .arg("--server-url")
        .arg(base)
        .args(args)
        .env("OC_API_KEY", key)
        .output()
        .await
        .unwrap();
    assert_eq!(
        output.status.success(),
        success,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    if success {
        serde_json::from_slice(&output.stdout).unwrap()
    } else {
        Value::Null
    }
}

#[tokio::test]
async fn cli_selected_run_edit_review_publish_and_withdraw() {
    let dir = tempfile::tempdir().unwrap();
    let service = support::open_service(dir.path(), Models::disabled())
        .await
        .unwrap();
    let store = service.engine.relational();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "synthetic skill CLI")
        .await
        .unwrap();
    let writer = store.issue_key(scope, "writer").await.unwrap();
    let reviewer = store.issue_key(scope, "writer").await.unwrap();
    let other = store.issue_key(scope, "admin").await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = origence::api::router(service.clone());
    let host = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let writer_client = HostClient::new(&base, writer.token.clone()).unwrap();
    let run = Uuid::new_v4().to_string();
    writer_client
        .call(
            Method::POST,
            "/v1/runs",
            Some(&json!({"run_id":run,"summary":"PRIVATE synthetic task"})),
            None,
        )
        .await
        .unwrap();
    let source = cli(&base, &writer.token, &["run-source", &run, "0"], true).await;
    cli(&base, &other.token, &["run-source", &run, "0"], false).await;
    writer_client.call(Method::POST, &format!("/v1/runs/{run}/events"), Some(&json!({
        "event_id":Uuid::new_v4(), "reporter":"test", "observed_at_ms":0,
        "event":{"kind":"result_reported","outcome":"unknown","summary":"PRIVATE later report"}
    })), None).await.unwrap();
    assert_eq!(
        source,
        cli(&base, &writer.token, &["run-source", &run, "0"], true).await
    );
    let candidate = Uuid::new_v4().to_string();
    let skill = Uuid::new_v4().to_string();
    let selection_path = dir.path().join("selection.json");
    let input = selection_path.to_str().unwrap();
    let mut selection = json!({"source":source,"name":"synthetic-check","description":"Verify outputs","steps":"Check outputs independently.","selected_basis":"Redacted synthetic observation","disclosure_confirmed":false,"private_notes":"PRIVATE support"});
    tokio::fs::write(&selection_path, selection.to_string())
        .await
        .unwrap();
    cli(
        &base,
        &writer.token,
        &[
            "save",
            &candidate,
            "--expected-revision",
            "0",
            "--input",
            input,
        ],
        false,
    )
    .await;
    selection["disclosure_confirmed"] = json!(true);
    tokio::fs::write(&selection_path, selection.to_string())
        .await
        .unwrap();
    for _ in 0..2 {
        assert_eq!(
            cli(
                &base,
                &writer.token,
                &[
                    "save",
                    &candidate,
                    "--expected-revision",
                    "0",
                    "--input",
                    input
                ],
                true
            )
            .await["revision"],
            1
        );
    }
    cli(&base, &other.token, &["candidate", &candidate], false).await;
    cli(&base, &reviewer.token, &["package", &skill, "1"], false).await;
    let tenant = scope.tenant_id.to_string();
    let workspace = scope.workspace_id.to_string();
    let reviewer_id = reviewer.key_id.to_string();
    let review = Uuid::new_v4().to_string();
    let submit = |review: &str, revision: &str| {
        vec![
            "submit".to_string(),
            candidate.clone(),
            review.into(),
            "--revision".into(),
            revision.into(),
            "--reviewer-id".into(),
            reviewer_id.clone(),
            "--tenant-id".into(),
            tenant.clone(),
            "--workspace-id".into(),
            workspace.clone(),
            "--skill-id".into(),
            skill.clone(),
            "--version".into(),
            "1".into(),
        ]
    };
    let args = submit(&review, "1");
    let packet = cli(
        &base,
        &writer.token,
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
        true,
    )
    .await;
    assert!(!packet.to_string().contains("PRIVATE"));
    assert!(!packet.to_string().contains(&run));
    let digest = packet["digest"].as_str().unwrap();
    cli(&base, &reviewer.token, &["publish", &review, digest], false).await;
    cli(
        &base,
        &other.token,
        &["decide", &review, digest, "--approve"],
        false,
    )
    .await;
    cli(&base, &reviewer.token, &["review", &review], true).await;
    cli(
        &base,
        &reviewer.token,
        &["decide", &review, digest, "--approve"],
        true,
    )
    .await;
    selection["selected_basis"] = json!("Changed redacted observation");
    tokio::fs::write(&selection_path, selection.to_string())
        .await
        .unwrap();
    assert_eq!(
        cli(
            &base,
            &writer.token,
            &[
                "save",
                &candidate,
                "--expected-revision",
                "1",
                "--input",
                input
            ],
            true
        )
        .await["revision"],
        2
    );
    cli(&base, &reviewer.token, &["publish", &review, digest], false).await;
    let review2 = Uuid::new_v4().to_string();
    let args = submit(&review2, "2");
    let packet = cli(
        &base,
        &writer.token,
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
        true,
    )
    .await;
    let digest = packet["digest"].as_str().unwrap();
    cli(
        &base,
        &reviewer.token,
        &["decide", &review2, digest, "--approve"],
        true,
    )
    .await;
    let package = cli(&base, &reviewer.token, &["publish", &review2, digest], true).await;
    assert!(!package.to_string().contains("observation"));
    cli(&base, &writer.token, &["package", &skill, "1"], true).await;
    cli(&base, &reviewer.token, &["withdraw", &skill, "1"], true).await;
    cli(&base, &writer.token, &["package", &skill, "1"], false).await;
    host.abort();
    service.engine.shutdown().await.unwrap();
}
