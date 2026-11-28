//! Actual standalone CLI against loopback HTTP and a fresh SQLite authority.
mod support;
use origence::{
    client::HostClient,
    models::Models,
    storage::{Scope, sqlite_db},
};
use reqwest::{Method, StatusCode};
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
    let writer = store
        .create_key_workspace(scope, "synthetic skill CLI")
        .await
        .unwrap();
    let foreign_scope = Scope {
        workspace_id: Uuid::new_v4(),
        ..scope
    };
    let other = store
        .create_key_workspace(foreign_scope, "foreign workspace")
        .await
        .unwrap();
    let administrator = store.rotate_deployment_admin().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = origence::api::router(service.clone());
    let host = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let writer_client = HostClient::new(&base, writer.token.clone()).unwrap();
    let admin_client = HostClient::new(&base, administrator).unwrap();
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
    cli(&base, &writer.token, &["package", &skill, "1"], false).await;
    let tenant = scope.tenant_id.to_string();
    let workspace = scope.workspace_id.to_string();
    let review = Uuid::new_v4().to_string();
    let submit = |review: &str, revision: &str| {
        vec![
            "submit".to_string(),
            candidate.clone(),
            review.into(),
            "--revision".into(),
            revision.into(),
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
    cli(&base, &writer.token, &["publish", &review, digest], false).await;
    let review_path =
        |id: &str| format!("/admin/deployment/workspaces/{workspace}/skill-reviews/{id}");
    let displayed = admin_client
        .call(Method::GET, &review_path(&review), None, None)
        .await
        .unwrap();
    assert_eq!(displayed["packet"], packet);
    admin_client
        .call(
            Method::POST,
            &format!("{}/decision", review_path(&review)),
            Some(&json!({"digest":digest,"approve":true})),
            None,
        )
        .await
        .unwrap();
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
    cli(&base, &writer.token, &["publish", &review, digest], false).await;
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
    admin_client
        .call(
            Method::POST,
            &format!("{}/decision", review_path(&review2)),
            Some(&json!({"digest":digest,"approve":true})),
            None,
        )
        .await
        .unwrap();
    let package = cli(&base, &writer.token, &["publish", &review2, digest], true).await;
    assert!(!package.to_string().contains("observation"));
    cli(&base, &writer.token, &["package", &skill, "1"], true).await;
    cli(&base, &writer.token, &["withdraw", &skill, "1"], true).await;
    cli(&base, &writer.token, &["package", &skill, "1"], false).await;
    host.abort();
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn single_workspace_key_submits_and_only_administrator_decides() {
    let dir = tempfile::tempdir().unwrap();
    let service = support::open_service(dir.path(), Models::disabled())
        .await
        .unwrap();
    let store = service.engine.relational();
    let tenant = Uuid::new_v4();
    let scope = Scope {
        tenant_id: tenant,
        workspace_id: Uuid::new_v4(),
    };
    let foreign = Scope {
        workspace_id: Uuid::new_v4(),
        ..scope
    };
    let key = store
        .create_key_workspace(scope, "Skill workspace")
        .await
        .unwrap();
    let foreign_key = store
        .create_key_workspace(foreign, "Other workspace")
        .await
        .unwrap();
    let old_admin = store.rotate_deployment_admin().await.unwrap();
    let owner = service.auth(&key.token).await.unwrap();
    let session = service.private_session(&owner).await.unwrap();
    let session = serde_json::from_value(session["session_id"].clone()).unwrap();
    let turn = Uuid::new_v4();
    let question = "syntheticreviewproofneedle";
    service.archive_turn(&owner, "skill-private-source", session, serde_json::from_value(json!({
        "turn_id":turn,"question":question,"answer":"PRIVATE source answer","used_evidence":[]
    })).unwrap()).await.unwrap();
    let run = Uuid::new_v4();
    service
        .create_run(
            &owner,
            serde_json::from_value(json!({"run_id":run,"summary":"PRIVATE Run"})).unwrap(),
        )
        .await
        .unwrap();
    let prepared = service
        .prepare_run_context(
            &owner,
            run,
            Uuid::new_v4(),
            serde_json::from_value(json!({
                "query":question,"routes":[{"route":"session","limit":1}]
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        prepared["bundle"]["items"][0]["provenance"]["turn_id"],
        json!(turn)
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = origence::api::router(service.clone());
    let host = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::new();
    let request = async |method, path: &str, token: &str, body: Option<&Value>| {
        let mut req = client
            .request(method, format!("{base}{path}"))
            .bearer_auth(token);
        if let Some(body) = body {
            req = req.json(body);
        }
        req.send().await.unwrap()
    };
    let candidate = Uuid::new_v4().to_string();
    let skill = Uuid::new_v4().to_string();
    let tenant = tenant.to_string();
    let workspace = scope.workspace_id.to_string();
    let review = Uuid::new_v4().to_string();
    let source = cli(
        &base,
        &key.token,
        &["run-source", &run.to_string(), "1"],
        true,
    )
    .await;
    let path = dir.path().join("single-workspace-selection.json");
    tokio::fs::write(&path, json!({
        "source":source,"name":"synthetic-review","description":"Check output","steps":"Verify output independently.",
        "selected_basis":"Only this redacted observation","disclosure_confirmed":true,"private_notes":"PRIVATE notes"
    }).to_string()).await.unwrap();
    cli(
        &base,
        &key.token,
        &[
            "save",
            &candidate,
            "--expected-revision",
            "0",
            "--input",
            path.to_str().unwrap(),
        ],
        true,
    )
    .await;
    let mut submit = vec![
        "submit",
        &candidate,
        &review,
        "--revision",
        "1",
        "--tenant-id",
        &tenant,
        "--workspace-id",
        &workspace,
        "--skill-id",
        &skill,
        "--version",
        "1",
    ];
    let key_id = key.key_id.to_string();
    submit.extend(["--reviewer-id", &key_id]);
    cli(&base, &key.token, &submit, false).await;
    submit.truncate(submit.len() - 2);
    let packet = cli(&base, &key.token, &submit, true).await;
    assert_eq!(cli(&base, &key.token, &submit, true).await, packet);
    let digest = packet["digest"].as_str().unwrap();
    let list = format!("/admin/deployment/workspaces/{workspace}/skill-reviews");
    let detail = format!("{list}/{review}");
    let decision = format!("{detail}/decision");
    let publish = format!("{detail}/publish");
    let owner_decision = format!("/v1/skill-reviews/{review}/decision");
    let approved = json!({"digest":digest,"approve":true});
    let foreign_list = format!(
        "/admin/deployment/workspaces/{}/skill-reviews",
        foreign.workspace_id
    );
    for (method, path, token, body, status) in [
        (
            Method::POST,
            owner_decision.as_str(),
            key.token.as_str(),
            Some(&approved),
            StatusCode::NOT_FOUND,
        ),
        (
            Method::POST,
            decision.as_str(),
            key.token.as_str(),
            Some(&approved),
            StatusCode::UNAUTHORIZED,
        ),
        (
            Method::GET,
            list.as_str(),
            foreign_key.token.as_str(),
            None,
            StatusCode::UNAUTHORIZED,
        ),
        (
            Method::GET,
            &format!("{foreign_list}/{review}"),
            &old_admin,
            None,
            StatusCode::NOT_FOUND,
        ),
    ] {
        assert_eq!(
            request(method, path, token, body).await.status(),
            status,
            "{path}"
        );
    }
    cli(&base, &key.token, &["publish", &review, digest], false).await;

    let admin = store.rotate_deployment_admin().await.unwrap();
    assert_eq!(
        request(Method::POST, &decision, &old_admin, Some(&approved))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let admin_auth = service
        .authorize_deployment_workspace(&admin, scope.workspace_id)
        .await
        .unwrap();
    let review_list = service
        .deployment_skill_reviews(&admin_auth, Uuid::nil(), 1)
        .await
        .unwrap();
    assert_eq!(
        review_list,
        json!({"items":[{"review_id":review,"revision":1,"decision":"pending"}],"next_after":null})
    );
    let foreign_auth = service
        .authorize_deployment_workspace(&admin, foreign.workspace_id)
        .await
        .unwrap();
    assert_eq!(
        service
            .deployment_skill_reviews(&foreign_auth, Uuid::nil(), 1)
            .await
            .unwrap()["items"],
        json!([])
    );
    assert!(matches!(
        service
            .deployment_skill_reviews(&owner, Uuid::nil(), 1)
            .await,
        Err(origence::error::AppError::Forbidden)
    ));
    assert!(matches!(
        service
            .skill_candidate(&admin_auth, Uuid::parse_str(&candidate).unwrap())
            .await,
        Err(origence::error::AppError::Forbidden)
    ));
    let response = request(Method::GET, &detail, &admin, None)
        .await
        .error_for_status()
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let displayed: Value = response.json().await.unwrap();
    assert_eq!(displayed["packet"], packet);
    assert_eq!(displayed["published"], false);
    assert!(
        !displayed.to_string().contains("PRIVATE") && !displayed.to_string().contains(question)
    );
    let wrong_digest = json!({"digest":"wrong","approve":true});
    assert_eq!(
        request(Method::POST, &decision, &admin, Some(&wrong_digest))
            .await
            .status(),
        StatusCode::CONFLICT
    );
    let rejected = Uuid::new_v4().to_string();
    submit[2] = &rejected;
    let rejected_packet = cli(&base, &key.token, &submit, true).await;
    let reject_digest = rejected_packet["digest"].as_str().unwrap();
    let rejected_detail = format!("{list}/{rejected}");
    let first: Value = request(Method::GET, &format!("{list}?limit=1"), &admin, None)
        .await
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let cursor = first["next_after"].as_str().unwrap();
    assert_eq!(first["items"][0]["review_id"], cursor);
    let second: Value = request(
        Method::GET,
        &format!("{list}?limit=1&after={cursor}"),
        &admin,
        None,
    )
    .await
    .error_for_status()
    .unwrap()
    .json()
    .await
    .unwrap();
    let remaining = if cursor == review {
        rejected.as_str()
    } else {
        review.as_str()
    };
    assert_eq!(second["items"][0]["review_id"], remaining);
    assert!(second["next_after"].is_null());
    let rejection = json!({"digest":reject_digest,"approve":false});
    request(
        Method::POST,
        &format!("{rejected_detail}/decision"),
        &admin,
        Some(&rejection),
    )
    .await
    .error_for_status()
    .unwrap();
    assert_eq!(
        request(
            Method::POST,
            &format!("{rejected_detail}/publish"),
            &admin,
            Some(&json!({"digest":reject_digest}))
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    for _ in 0..2 {
        request(Method::POST, &decision, &admin, Some(&approved))
            .await
            .error_for_status()
            .unwrap();
    }
    let actors: Vec<Uuid> = sqlite_db::query_scalar("SELECT actor FROM oc_audit WHERE tenant_id=? AND workspace_id=? AND target=? AND action='skill.review.approved'")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(Uuid::parse_str(&review).unwrap()).fetch_all(store.pool()).await.unwrap();
    assert_eq!(actors, vec![admin_auth.id]);
    let next_key = store.rotate_workspace_key(scope).await.unwrap();
    cli(&base, &key.token, &["candidate", &candidate], false).await;
    assert_eq!(
        cli(&base, &next_key.token, &["candidate", &candidate], true).await["revision"],
        1
    );
    let next_admin = store.rotate_deployment_admin().await.unwrap();
    let exported = cli(&base, &next_key.token, &["publish", &review, digest], true).await;
    assert_eq!(exported, packet["package"]);
    let published: Value = request(
        Method::POST,
        &publish,
        &next_admin,
        Some(&json!({"digest":digest})),
    )
    .await
    .error_for_status()
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(published, exported);
    let displayed: Value = request(Method::GET, &detail, &next_admin, None)
        .await
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(displayed["published"], true);
    assert_eq!(
        cli(&base, &next_key.token, &["package", &skill, "1"], true).await,
        exported
    );
    host.abort();
    service.engine.shutdown().await.unwrap();
}

#[path = "support/fixture_keys.rs"]
mod fixture_keys;
