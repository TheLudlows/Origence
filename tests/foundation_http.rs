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

#[tokio::test]
async fn run_verification_is_external_claim_bound_to_prepared_v2_and_withdrawn_source() {
    use origence::{
        run::{RunEventInput, RunInput},
        worker,
    };
    let dir = tempfile::tempdir().unwrap();
    let service = open_service(dir.path(), Models::disabled()).await.unwrap();
    let store = service.engine.relational();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(
            scope.tenant_id,
            scope.workspace_id,
            "synthetic verification",
        )
        .await
        .unwrap();
    let owner = store.issue_key(scope, "admin").await.unwrap();
    let reviewer = store.issue_key(scope, "reviewer").await.unwrap();
    let a = service.auth(&owner.token).await.unwrap();
    let first = service
        .knowledge(
            &a,
            "v1",
            serde_json::from_value(
                json!({"title":"Atlas", "content":"Atlas version one", "format":"text"}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    while worker::process_next(&service).await.unwrap() {}
    let asset = first["asset_id"].clone();
    let second = service.knowledge(&a, "v2", serde_json::from_value(json!({"title":"Atlas", "content":"Atlas version two", "format":"text", "asset_id":asset, "expected_version":1})).unwrap()).await.unwrap();
    while worker::process_next(&service).await.unwrap() {}
    let run = Uuid::new_v4();
    service
        .create_run(
            &a,
            RunInput {
                run_id: run,
                summary: "synthetic task".into(),
            },
        )
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = origence::api::router(service.clone());
    let host = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = HostClient::new(&base, owner.token.clone()).unwrap();
    let path = format!("/v1/runs/{run}");
    let prepared = client
        .call(
            Method::POST,
            &format!("{path}/context"),
            Some(&json!({"event_id":Uuid::new_v4(),"context":{"query":"Atlas", "mode":"keyword"}})),
            None,
        )
        .await
        .unwrap();
    assert!(
        prepared["bundle"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["provenance"]["asset_version"] == 2 || i["asset_version"] == 2),
        "{prepared}"
    );
    service.append_run_event(&a, run, serde_json::from_value::<RunEventInput>(json!({"event_id":Uuid::new_v4(),"reporter":"user","observed_at_ms":1,"event":{"kind":"result_reported","outcome":"success","summary":"self-reported"}})).unwrap()).await.unwrap();
    let prefix = service.run_prefix(&a, run, 2).await.unwrap();
    let id = Uuid::new_v4();
    let input = json!({"verification_id":id,"through_sequence":2,"prefix_digest":prefix["digest"],"verifier_label":"synthetic external assessor","assessed_at_ms":2,"conclusion":"failed","basis_summary":"external check found missing output","basis_sequences":[1,2],"reported_skill_version":null});
    let route = format!("{path}/verifications");
    let receipt = client
        .call(Method::POST, &route, Some(&input), None)
        .await
        .unwrap();
    assert_eq!(receipt["evidence_origin"], "owner_transcribed_external");
    assert_eq!(receipt["verifier_authentication"], "unverified_claim");
    assert_eq!(receipt["platform_verification"], "not_performed");
    assert_eq!(receipt["assessment"]["reported_skill_version"], Value::Null);
    assert_eq!(
        receipt,
        client
            .call(Method::POST, &route, Some(&input), None)
            .await
            .unwrap()
    );
    let outsider = HostClient::new(&base, reviewer.token.clone()).unwrap();
    assert!(
        outsider
            .call(Method::GET, &route, None, None)
            .await
            .is_err()
    );
    assert!(
        outsider
            .call(Method::POST, &route, Some(&input), None)
            .await
            .is_err()
    );
    assert!(outsider.call(Method::GET, &path, None, None).await.is_err());
    let mut forged = input.clone();
    forged["platform_verification"] = json!("passed");
    assert!(
        client
            .call(Method::POST, &route, Some(&forged), None)
            .await
            .is_err()
    );
    let mut invalid = input.clone();
    invalid["basis_sequences"] = json!([3]);
    assert!(
        client
            .call(Method::POST, &route, Some(&invalid), None)
            .await
            .is_err()
    );
    let mut changed = input.clone();
    changed["conclusion"] = json!("passed");
    assert!(matches!(
        client
            .call(Method::POST, &route, Some(&changed), None)
            .await,
        Err(origence::error::AppError::Conflict(_))
    ));
    // Later Run events do not invalidate or reinterpret the exact assessed prefix.
    service.append_run_event(&a, run, serde_json::from_value(json!({"event_id":Uuid::new_v4(),"reporter":"test","observed_at_ms":3,"event":{"kind":"coverage","component":"model_use","status":"unknown","summary":"not observed"}})).unwrap()).await.unwrap();
    assert_eq!(
        receipt,
        client.call(Method::GET, &route, None, None).await.unwrap()["items"][0]
    );
    let withdraw = format!("{route}/{id}/withdraw");
    let reason = json!({"reason":"external evidence withdrawn"});
    let withdrawn = client
        .call(Method::POST, &withdraw, Some(&reason), None)
        .await
        .unwrap();
    assert_eq!(
        withdrawn,
        client
            .call(Method::POST, &withdraw, Some(&reason), None)
            .await
            .unwrap()
    );
    assert_eq!(
        withdrawn,
        client
            .call(Method::POST, &route, Some(&input), None)
            .await
            .unwrap()
    );
    // Source withdrawal blocks even historical/revoked assessment read and replay.
    let source: Uuid = serde_json::from_value(second["source_event_id"].clone()).unwrap();
    service
        .delete(&a, "withdraw-source", source, "event")
        .await
        .unwrap();
    assert!(client.call(Method::GET, &route, None, None).await.is_err());
    assert!(
        client
            .call(Method::POST, &route, Some(&input), None)
            .await
            .is_err()
    );
    assert!(
        client
            .call(Method::POST, &withdraw, Some(&reason), None)
            .await
            .is_err()
    );
    host.abort();
    service.engine.shutdown().await.unwrap();
}
