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
    let reviewer = store.issue_key(scope, "writer").await.unwrap();
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
    assert_eq!(
        http_session,
        session_from_mcp(&base, &writer.token, None).await
    );
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
    let mut reader_appointment = review_input.clone();
    reader_appointment["reviewer_id"] = json!(reader.key_id);
    assert!(matches!(
        writer_client
            .call(
                Method::POST,
                &format!("{candidate_path}/reviews/{}", Uuid::new_v4()),
                Some(&reader_appointment),
                None,
            )
            .await,
        Err(origence::error::AppError::Forbidden)
    ));
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
    assert!(matches!(
        reader_client
            .call(
                Method::POST,
                &format!("{review_path}/publish"),
                Some(&exact),
                None,
            )
            .await,
        Err(origence::error::AppError::Forbidden)
    ));
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

pub(crate) async fn session_from_mcp(base: &str, key: &str, workspace: Option<Uuid>) -> Value {
    crate::mcp_http::Peer::at(format!("{base}/mcp"))
        .call(key, workspace, "context_session", json!({}))
        .await
        .unwrap()
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
    let reviewer = store.issue_key(scope, "writer").await.unwrap();
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
    let readback = client.call(Method::GET, &route, None, None).await.unwrap();
    assert_eq!(readback["items"][0]["assessment"]["conclusion"], "failed");
    assert_eq!(
        readback["items"][0]["platform_verification"],
        "not_performed"
    );
    assert_eq!(
        readback["items"][0]["verifier_authentication"],
        "unverified_claim"
    );
    let trace = client.call(Method::GET, &path, None, None).await.unwrap();
    assert_eq!(trace["events"][0]["evidence_origin"], "server_prepared");
    assert_eq!(trace["events"][1]["evidence_origin"], "reported");
    assert_eq!(trace["events"][1]["report"]["event"]["outcome"], "success");
    let withdraw = format!("{route}/{id}/withdraw");
    assert!(matches!(
        outsider
            .call(
                Method::POST,
                &withdraw,
                Some(&json!({"reason":"forged approval"})),
                None
            )
            .await,
        Err(origence::error::AppError::NotFound | origence::error::AppError::Forbidden)
    ));

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
    let link_input = json!({
        "verification_id":Uuid::new_v4(), "through_sequence":2,
        "prefix_digest":prefix["digest"], "external_ref":"finance/assessment-123"
    });
    let linked = service
        .link_run_verification(&a, run, serde_json::from_value(link_input.clone()).unwrap())
        .await
        .unwrap();
    assert_eq!(linked["evidence_origin"], "owner_linked_external");
    assert_eq!(linked["assessment"], link_input);
    assert_eq!(linked["platform_verification"], "not_performed");
    assert_eq!(
        linked,
        client
            .call(Method::POST, &route, Some(&link_input), None)
            .await
            .unwrap()
    );
    assert!(
        outsider
            .call(Method::POST, &route, Some(&link_input), None)
            .await
            .is_err()
    );
    let mut changed_link = link_input.clone();
    changed_link["external_ref"] = json!("finance/another-assessment");
    assert!(matches!(
        client
            .call(Method::POST, &route, Some(&changed_link), None)
            .await,
        Err(origence::error::AppError::Conflict(_))
    ));
    assert!(
        client.call(Method::GET, &route, None, None).await.unwrap()["items"]
            .as_array()
            .unwrap()
            .contains(&linked)
    );
    let overview = || client.call(Method::GET, "/v1/runs/overview", None, None);
    assert_eq!(overview().await.unwrap()["created_runs"], 1);
    // Source withdrawal blocks even historical/revoked assessment read and replay.
    let source: Uuid = serde_json::from_value(second["source_event_id"].clone()).unwrap();
    service
        .delete(&a, "withdraw-source", source, "event")
        .await
        .unwrap();
    assert!(matches!(
        client.call(Method::GET, &route, None, None).await,
        Err(origence::error::AppError::NotFound | origence::error::AppError::Forbidden)
    ));
    assert!(matches!(
        client.call(Method::GET, &path, None, None).await,
        Err(origence::error::AppError::NotFound | origence::error::AppError::Forbidden)
    ));
    assert_eq!(overview().await.unwrap()["created_runs"], 1);
    let catalog = client
        .call(Method::GET, "/v1/runs", None, None)
        .await
        .unwrap();
    assert_eq!(catalog["items"][0]["run_id"], run.to_string());
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

#[tokio::test]
async fn deployment_admin_provisions_peer_workspaces_and_manages_scoped_keys() {
    let dir = tempfile::tempdir().unwrap();
    let service = open_service(dir.path(), Models::disabled()).await.unwrap();
    let store = service.engine.relational();
    let member = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(member.tenant_id, member.workspace_id, "Members")
        .await
        .unwrap();
    let member_admin = store.issue_key(member, "admin").await.unwrap();
    let admin = store.rotate_deployment_admin().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let serve =
        tokio::spawn(axum::serve(listener, origence::api::router(service.clone())).into_future());
    let http = reqwest::Client::new();
    let path = "/admin/deployment/workspaces";
    let workspace = Uuid::new_v4();
    let destination = format!("{path}/{workspace}");
    let call = |method, path: &str, token: &str| {
        http.request(method, format!("{base}{path}"))
            .bearer_auth(token)
    };
    let create = |token: &str, name: &str| {
        call(Method::PUT, &destination, token)
            .json(&json!({"name":name}))
            .send()
    };

    assert_eq!(
        create(&member_admin.token, "Independent")
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            Method::PUT,
            &format!("/v1/workspaces/owned/{workspace}"),
            &member_admin.token
        )
        .json(&json!({"name":"Child"}))
        .send()
        .await
        .unwrap()
        .status(),
        StatusCode::NOT_FOUND
    );
    let first = create(&admin, "Independent")
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(first.headers()["cache-control"], "no-store");
    let first: Value = first.json().await.unwrap();
    assert_eq!(first["workspace_id"], json!(workspace));
    assert_eq!(first["created"], true);
    let key = first["key"]["token"].as_str().unwrap();
    let before = service.auth(key).await.unwrap();
    assert_eq!(before.workspace_id, workspace);
    assert_eq!(before.role, "workspace");
    let session = service.private_session(&before).await.unwrap();
    let replay: Value = create(&admin, "Independent")
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        replay,
        json!({"workspace_id":workspace,"name":"Independent","created":false})
    );
    assert_eq!(
        create(&admin, "Different").await.unwrap().status(),
        StatusCode::CONFLICT
    );

    let list: Value = call(Method::GET, &format!("{destination}/keys"), &admin)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list["single_key"], true);
    assert_eq!(list["items"][0]["key_id"], first["key"]["key_id"]);
    assert!(!list.to_string().contains(key));
    let rotate = format!("{destination}/keys/rotate");
    assert_eq!(
        call(Method::POST, &rotate, key)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let replacement: Value = call(Method::POST, &rotate, &admin)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(service.auth(key).await.is_err());
    let after = service
        .auth(replacement["token"].as_str().unwrap())
        .await
        .unwrap();
    assert_eq!(service.private_session(&after).await.unwrap(), session);

    let peer_keys = format!("{path}/{}/keys", member.workspace_id);
    let issued: Value = call(Method::POST, &peer_keys, &admin)
        .json(&json!({"workspace_id":member.workspace_id,"role":"writer"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let writer = service
        .auth(issued["token"].as_str().unwrap())
        .await
        .unwrap();
    assert_eq!(writer.workspace_id, member.workspace_id);
    assert_eq!(writer.role, "writer");
    assert_eq!(
        call(Method::POST, &format!("{destination}/keys"), &admin)
            .json(&json!({"workspace_id":workspace,"role":"writer"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            Method::DELETE,
            &format!("{peer_keys}/{}", first["key"]["key_id"].as_str().unwrap()),
            &admin
        )
        .send()
        .await
        .unwrap()
        .status(),
        StatusCode::NOT_FOUND
    );
    let rotated: Value = call(
        Method::POST,
        &format!("{peer_keys}/{}/rotate", writer.id),
        &admin,
    )
    .send()
    .await
    .unwrap()
    .error_for_status()
    .unwrap()
    .json()
    .await
    .unwrap();
    call(
        Method::DELETE,
        &format!("{peer_keys}/{}", rotated["key_id"].as_str().unwrap()),
        &admin,
    )
    .send()
    .await
    .unwrap()
    .error_for_status()
    .unwrap();
    assert!(
        service
            .auth(rotated["token"].as_str().unwrap())
            .await
            .is_err()
    );
    store.rotate_deployment_admin().await.unwrap();
    assert_eq!(
        call(Method::POST, &rotate, &admin)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    serve.abort();
}
