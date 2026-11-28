use crate::support::open_service;
use origence::{
    client::HostClient,
    error::AppError,
    models::Models,
    storage::{RelationalStore, Scope},
};
use reqwest::{Method, StatusCode};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn deployment_admin_reads_across_tenants_and_cannot_write_workspace_data() {
    let dir = tempfile::tempdir().unwrap();
    let service = open_service(dir.path(), Models::disabled()).await.unwrap();
    let store = service.engine.relational();
    let first = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    let second = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    let owner = store.create_key_workspace(first, "First").await.unwrap();
    store.create_key_workspace(second, "Second").await.unwrap();
    let admin = store.rotate_deployment_admin().await.unwrap();
    let auth = service.auth(&owner.token).await.unwrap();
    let session =
        serde_json::from_value(service.private_session(&auth).await.unwrap()["session_id"].clone())
            .unwrap();
    let turn = Uuid::new_v4();
    service
        .archive_turn(
            &auth,
            "archive",
            session,
            serde_json::from_value(
                json!({"turn_id":turn,"question":"初始问题","answer":"独立工作空间的会话","used_evidence":[]}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let run = Uuid::new_v4();
    service
        .create_run(
            &auth,
            serde_json::from_value(json!({"run_id":run,"summary":"recorded execution"})).unwrap(),
        )
        .await
        .unwrap();
    let asset = service
        .memory(
            &auth,
            "memory",
            serde_json::from_value(json!({"fact_key":"local","content":"源空间里的记忆"})).unwrap(),
        )
        .await
        .unwrap();
    while origence::worker::process_next(&service).await.unwrap() {}
    let asset_id = serde_json::from_value(asset["asset_id"].clone()).unwrap();
    store.revoke_key(owner.key_id).await.unwrap();

    let first_page = service
        .inspect_workspaces(&admin, Uuid::nil(), 1)
        .await
        .unwrap();
    let cursor = serde_json::from_value(first_page["next_after"].clone()).unwrap();
    let second_page = service.inspect_workspaces(&admin, cursor, 1).await.unwrap();
    assert_ne!(
        first_page["items"][0]["workspace_id"],
        second_page["items"][0]["workspace_id"]
    );
    assert!(second_page["next_after"].is_null());
    assert_eq!(
        service
            .inspect_sessions(&admin, first.workspace_id, Uuid::nil(), 1)
            .await
            .unwrap()["items"],
        json!([session])
    );
    assert_eq!(
        service
            .inspect_turns(&admin, first.workspace_id, session, 0, 1)
            .await
            .unwrap()["turns"][0]["answer"],
        "独立工作空间的会话"
    );
    assert!(matches!(
        service
            .inspect_turns(&admin, second.workspace_id, session, 0, 1)
            .await,
        Err(AppError::NotFound)
    ));
    assert_eq!(
        service
            .inspect_runs(&admin, first.workspace_id, Uuid::nil(), 1)
            .await
            .unwrap()["items"][0]["run_id"],
        json!(run)
    );
    assert_eq!(
        service
            .inspect_run(&admin, first.workspace_id, run, 0, 10)
            .await
            .unwrap()["run"]["summary"],
        "recorded execution"
    );
    assert_eq!(
        service
            .inspect_assets(&admin, first.workspace_id, "memory", Uuid::nil(), 1)
            .await
            .unwrap()["items"][0]["asset_id"],
        json!(asset_id)
    );
    assert_eq!(
        service
            .inspect_asset(&admin, first.workspace_id, asset_id, Some(1))
            .await
            .unwrap()["content"],
        "源空间里的记忆"
    );
    assert!(matches!(
        service
            .inspect_asset(&admin, second.workspace_id, asset_id, None)
            .await,
        Err(AppError::NotFound)
    ));

    let ordinary = store.rotate_workspace_key(first).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = origence::api::router(service.clone());
    let host = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let http = reqwest::Client::new();
    let path = "/admin/deployment/workspaces";
    let request = http
        .get(format!(
            "{base}{path}/{}/sessions/{session}/turns",
            first.workspace_id
        ))
        .bearer_auth(&admin);
    let response = request.send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        http.get(format!("{base}{path}"))
            .bearer_auth(&ordinary.token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        http.post(format!("{base}{path}"))
            .bearer_auth(&admin)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    assert_eq!(
        http.post(format!("{base}/v1/memories"))
            .bearer_auth(&admin)
            .header("idempotency-key", "denied")
            .json(&json!({"fact_key":"denied","content":"denied"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let new_admin = store.rotate_deployment_admin().await.unwrap();
    assert!(matches!(
        service.inspect_workspaces(&admin, Uuid::nil(), 1).await,
        Err(AppError::Unauthorized)
    ));
    service
        .authenticate_deployment_admin(&new_admin)
        .await
        .unwrap();
    host.abort();
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn workspace_archive_extracts_once_across_restart_and_rotation() {
    use axum::{Json, Router, routing::post};
    use serde_json::Value;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let calls = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let record_call = calls.clone();
    let model = Router::new().route(
        "/chat/completions",
        post(move |Json(input): Json<Value>| {
            let came_from_archive = record_call.clone();
            async move {
                let source: Value =
                    serde_json::from_str(input["messages"][1]["content"].as_str().unwrap())
                        .unwrap();
                assert_eq!(source["question"], "Atlas is deployed in Zürich.");
                assert_eq!(source["answer"], "Deployment completed.");
                came_from_archive.fetch_add(1, Ordering::SeqCst);
                let content = json!({"memories":[{
                    "fact_key":"atlas/location","content":"Atlas is deployed in Zürich."
                }]})
                .to_string();
                Json(json!({"choices":[{"message":{"content":content}}]}))
            }
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, model).await.unwrap() });
    let models =
        Models::configured(endpoint, "fixture".into(), None, Some("extract".into()), 2).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let service = open_service(dir.path(), models.clone()).await.unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    let key = service
        .engine
        .relational()
        .create_key_workspace(scope, "Atlas")
        .await
        .unwrap();
    let auth = service.auth(&key.token).await.unwrap();
    let session =
        serde_json::from_value(service.private_session(&auth).await.unwrap()["session_id"].clone())
            .unwrap();
    let turn = Uuid::new_v4();
    let input: origence::session::SessionTurnInput = serde_json::from_value(json!({
        "turn_id":turn, "question":"Atlas is deployed in Zürich.",
        "answer":"Deployment completed.", "used_evidence":[]
    }))
    .unwrap();
    let receipt = service
        .archive_turn(&auth, "initial", session, input.clone())
        .await
        .unwrap();
    let job = serde_json::from_value(receipt["memory_job_id"].clone()).unwrap();
    assert_eq!(service.job(&auth, job).await.unwrap()["state"], "pending");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    service.engine.shutdown().await.unwrap();
    drop(service);

    let service = open_service(dir.path(), models).await.unwrap();
    let store = service.engine.relational();
    let rotated = store.rotate_workspace_key(scope).await.unwrap();
    let auth = service.auth(&rotated.token).await.unwrap();
    let replay = service
        .archive_turn(&auth, "after-restart", session, input.clone())
        .await
        .unwrap();
    assert_eq!(replay, receipt);
    assert!(origence::worker::process_next(&service).await.unwrap());
    let result = service.job(&auth, job).await.unwrap();
    assert_eq!(result["state"], "completed");
    let asset =
        serde_json::from_value(result["result"]["memories"][0]["asset_id"].clone()).unwrap();
    let memory = service.get(&auth, asset, None).await.unwrap();
    assert_eq!(memory["content"], "Atlas is deployed in Zürich.");
    assert_eq!(memory["source_event_id"], receipt["source_event_id"]);
    let query: origence::types::SearchInput =
        serde_json::from_value(json!({"query":"Zürich"})).unwrap();
    assert!(
        service.search(&auth, query.clone()).await.unwrap()["hits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|hit| hit["asset_id"] == json!(asset))
    );
    let other = store
        .create_key_workspace(
            Scope {
                workspace_id: Uuid::new_v4(),
                ..scope
            },
            "Other",
        )
        .await
        .unwrap();
    let other_auth = service.auth(&other.token).await.unwrap();
    assert!(matches!(
        service.get(&other_auth, asset, None).await,
        Err(AppError::NotFound)
    ));
    service
        .delete(&auth, "delete-extracted-memory", asset, "asset")
        .await
        .unwrap();
    assert_eq!(
        service
            .archive_turn(&auth, "after-delete", session, input)
            .await
            .unwrap(),
        receipt
    );
    assert!(matches!(
        service
            .job_action(&auth, "retry-complete", job, "retry")
            .await,
        Err(AppError::Conflict(_))
    ));
    while origence::worker::process_next(&service).await.unwrap() {}
    assert!(matches!(
        service.get(&auth, asset, Some(1)).await,
        Err(AppError::NotFound)
    ));
    assert!(
        service.search(&auth, query).await.unwrap()["hits"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        service.session_turn(&auth, session, turn).await.unwrap()["answer"],
        "Deployment completed."
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    service.engine.shutdown().await.unwrap();
    server.abort();
}

#[tokio::test]
async fn single_workspace_key_owns_content_but_cannot_delete_turns_or_expand_access() {
    let dir = tempfile::tempdir().unwrap();
    let service = open_service(dir.path(), Models::disabled()).await.unwrap();
    let store = service.engine.relational();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    let other = Scope {
        workspace_id: Uuid::new_v4(),
        ..scope
    };
    let key = store
        .create_key_workspace(scope, "Workspace A")
        .await
        .unwrap();
    let other_key = store
        .create_key_workspace(other, "Workspace B")
        .await
        .unwrap();
    assert!(matches!(
        store.issue_key(scope, "admin").await,
        Err(origence::storage::StorageError::Forbidden)
    ));

    let auth = service.auth(&key.token).await.unwrap();
    let session = service.private_session(&auth).await.unwrap();
    let session_id: Uuid = serde_json::from_value(session["session_id"].clone()).unwrap();
    let turn = Uuid::new_v4();
    let receipt = service
        .archive_turn(
            &auth,
            "archive",
            session_id,
            serde_json::from_value(json!({
                "turn_id":turn,"question":"原始问题","answer":"归档只能查看","used_evidence":[]
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let source = receipt["source_event_id"].as_str().unwrap();
    let run = Uuid::new_v4();
    service
        .create_run(
            &auth,
            serde_json::from_value(json!({"run_id":run,"summary":"Rotation fixture"})).unwrap(),
        )
        .await
        .unwrap();
    assert!(matches!(
        service
            .withdraw_session_archive(&auth, session_id, turn)
            .await,
        Err(AppError::Forbidden)
    ));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let client = HostClient::new(&base, key.token.clone()).unwrap();
    let router = origence::api::router(service.clone());
    let host = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    assert_eq!(
        client
            .call(Method::GET, "/v1/whoami", None, None)
            .await
            .unwrap()["role"],
        "workspace"
    );
    assert_eq!(
        client
            .call(Method::GET, "/v1/workspaces", None, None)
            .await
            .unwrap()["workspaces"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for path in ["/admin/keys", "/v1/workspaces/owned"] {
        assert!(matches!(
            client.call(Method::GET, path, None, None).await,
            Err(AppError::Forbidden)
        ));
    }
    for path in [
        format!("/v1/sessions/{session_id}/turns/{turn}"),
        format!("/v1/sessions/{session_id}?through_sequence=1"),
        format!("/v1/events/{source}"),
    ] {
        assert!(matches!(
            client
                .call(Method::DELETE, &path, None, Some("delete-turn"))
                .await,
            Err(AppError::Forbidden)
        ));
    }
    assert_eq!(
        client
            .call(
                Method::GET,
                &format!("/v1/sessions/{session_id}/turns/{turn}"),
                None,
                None
            )
            .await
            .unwrap()["answer"],
        "归档只能查看"
    );
    assert!(matches!(
        client
            .clone()
            .with_workspace(other.workspace_id)
            .call(Method::GET, "/v1/whoami", None, None)
            .await,
        Err(AppError::Forbidden)
    ));

    let memory = service
        .memory(
            &auth,
            "remember",
            serde_json::from_value(json!({
                "fact_key":"fixture.workspace.preference","content":"保留中文偏好"
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let knowledge = client
        .call(
            Method::POST,
            "/v1/knowledge",
            Some(&json!({"title":"Workflow","content":"Original procedure"})),
            Some("knowledge-v1"),
        )
        .await
        .unwrap();
    while origence::worker::process_next(&service).await.unwrap() {}
    let asset: Uuid = serde_json::from_value(memory["asset_id"].clone()).unwrap();
    let knowledge_path = format!("/v1/assets/{}", knowledge["asset_id"].as_str().unwrap());
    let queued_knowledge = client
        .call(
            Method::POST,
            "/v1/knowledge",
            Some(
                &json!({"asset_id":knowledge["asset_id"],"expected_version":1,
                "title":"Workflow","content":"Procedure accepted before rotation"}),
            ),
            Some("knowledge-v2"),
        )
        .await
        .unwrap();
    let queued_memory = client
        .call(
            Method::POST,
            "/v1/memories",
            Some(&json!({"fact_key":"fixture.retry","content":"Retry after rotation"})),
            Some("queued-memory"),
        )
        .await
        .unwrap();
    let job_id = queued_memory["job_id"].as_str().unwrap();
    client
        .call(
            Method::POST,
            &format!("/v1/jobs/{job_id}/cancel"),
            None,
            Some("cancel-before-rotation"),
        )
        .await
        .unwrap();

    let replacement = store.rotate_workspace_key(scope).await.unwrap();
    assert!(matches!(
        client.call(Method::GET, "/v1/whoami", None, None).await,
        Err(AppError::Unauthorized)
    ));
    let peer = super::mcp_http::Peer::at(format!("{base}/mcp"));
    assert_eq!(
        peer.call(&key.token, None, "context_session", json!({}))
            .await,
        Err(json!({"http_status":401}))
    );
    assert_eq!(
        peer.call(&replacement.token, None, "context_session", json!({}))
            .await
            .unwrap(),
        session
    );
    assert_eq!(
        peer.call(
            &replacement.token,
            None,
            "context_turn_get",
            json!({"turn_id":turn})
        )
        .await
        .unwrap()["source_event_id"],
        receipt["source_event_id"]
    );
    let current = HostClient::new(&base, replacement.token.clone()).unwrap();
    assert_eq!(
        current
            .call(Method::GET, &format!("/v1/runs/{run}"), None, None)
            .await
            .unwrap()["run"]["summary"],
        "Rotation fixture"
    );
    assert_eq!(
        current
            .call(Method::GET, &knowledge_path, None, None)
            .await
            .unwrap()["content"],
        "Original procedure"
    );
    let rotated = service.auth(&replacement.token).await.unwrap();
    assert_eq!(
        service.get(&rotated, asset, None).await.unwrap()["content"],
        "保留中文偏好"
    );
    let neighboring = HostClient::new(&base, other_key.token).unwrap();
    assert_eq!(
        neighboring
            .call(Method::GET, "/v1/whoami", None, None)
            .await
            .unwrap()["workspace_id"],
        json!(other.workspace_id)
    );
    assert!(matches!(
        neighboring
            .call(Method::GET, &knowledge_path, None, None)
            .await,
        Err(AppError::NotFound)
    ));

    current
        .call(
            Method::POST,
            &format!("/v1/jobs/{job_id}/retry"),
            None,
            Some("retry-after-rotation"),
        )
        .await
        .unwrap();
    while origence::worker::process_next(&service).await.unwrap() {}
    let knowledge_job: Uuid = serde_json::from_value(queued_knowledge["job_id"].clone()).unwrap();
    assert_eq!(
        service.job(&rotated, knowledge_job).await.unwrap()["state"],
        "completed"
    );
    assert_eq!(
        current
            .call(Method::GET, &knowledge_path, None, None)
            .await
            .unwrap()["content"],
        "Procedure accepted before rotation"
    );
    let memory_job: Uuid = job_id.parse().unwrap();
    assert_eq!(
        service.job(&rotated, memory_job).await.unwrap()["state"],
        "completed"
    );
    let stored_creator: Uuid = origence::storage::sqlite_db::query_scalar(
        "SELECT created_by FROM oc_jobs WHERE tenant_id=? AND workspace_id=? AND id=?",
    )
    .bind(scope.tenant_id)
    .bind(scope.workspace_id)
    .bind(knowledge_job)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(stored_creator, key.key_id);
    service
        .delete(&rotated, "delete-memory", asset, "asset")
        .await
        .unwrap();
    assert!(matches!(
        service.get(&rotated, asset, None).await,
        Err(AppError::NotFound)
    ));
    host.abort();
    service.engine.shutdown().await.unwrap();
}

async fn bootstrap(dir: &std::path::Path, name: &str, existing: Option<Uuid>) -> serde_json::Value {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_origence"));
    command
        .args(["--offline", "workspace-create", name])
        .env("OC_DATA_DIR", dir);
    if let Some(workspace) = existing {
        command.args(["--in-tenant-of", &workspace.to_string()]);
    }
    let output = command.output().await.unwrap();
    assert!(output.status.success(), "workspace bootstrap failed");
    serde_json::from_slice(&output.stdout).unwrap()
}

#[tokio::test]
async fn admin_key_inventory_is_scoped_paginated_and_reflects_credential_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let a = bootstrap(dir.path(), "A", None).await;
    let a_id: Uuid = a["workspace_id"].as_str().unwrap().parse().unwrap();
    let b = bootstrap(dir.path(), "B", Some(a_id)).await;
    let other = bootstrap(dir.path(), "Other tenant", None).await;
    let service = open_service(dir.path(), Models::disabled()).await.unwrap();
    let a_token = a["token"].as_str().unwrap();
    let a_admin = service.auth(a_token).await.unwrap();
    let b_admin = service.auth(b["token"].as_str().unwrap()).await.unwrap();
    let reader = service.issue_key(&a_admin, "reader").await.unwrap();
    let writer = service.issue_key(&a_admin, "writer").await.unwrap();
    let token = writer["token"].as_str().unwrap();
    let logical: Uuid = writer["key_id"].as_str().unwrap().parse().unwrap();
    service
        .set_workspace_key_grant(&b_admin, logical, Some("reader"))
        .await
        .unwrap();

    let issued = service
        .list_issued_keys(&a_admin, Uuid::nil(), 100)
        .await
        .unwrap();
    assert_eq!(issued["items"].as_array().unwrap().len(), 3);
    assert!(!issued.to_string().contains(token));
    assert!(!issued.to_string().contains("token_hash"));
    let grants = service
        .list_workspace_key_grants(&b_admin, Uuid::nil(), 1)
        .await
        .unwrap();
    assert_eq!(
        grants["items"],
        json!([{"logical_key_id":logical,"role":"reader","active":true}])
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = origence::api::router(service.clone());
    let host = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = |key: &str| HostClient::new(&base, key.into()).unwrap();
    let a_client = client(a_token);
    let b_client = client(b["token"].as_str().unwrap());
    let foreign = client(other["token"].as_str().unwrap());
    let read_client = client(reader["token"].as_str().unwrap());
    let keys = "/admin/keys";
    let grants = "/admin/workspace-key-grants";
    for path in [keys, grants] {
        assert!(matches!(
            read_client.call(Method::GET, path, None, None).await,
            Err(AppError::Forbidden)
        ));
        assert!(matches!(
            a_client
                .call(Method::GET, &format!("{path}?limit=0"), None, None)
                .await,
            Err(AppError::Invalid(_))
        ));
    }
    assert!(matches!(
        a_client
            .call(
                Method::POST,
                keys,
                Some(&json!({"workspace_id":a_id, "role":"reviewer"})),
                None
            )
            .await,
        Err(AppError::Conflict(message)) if message == "invalid role"
    ));
    assert!(matches!(
        b_client
            .call(
                Method::PUT,
                &format!("{grants}/{logical}"),
                Some(&json!({"role":"reviewer"})),
                None
            )
            .await,
        Err(AppError::Invalid(_))
    ));
    let first = a_client
        .call(Method::GET, "/admin/keys?limit=1", None, None)
        .await
        .unwrap();
    let next = first["next_after"].as_str().unwrap();
    assert_eq!(first["items"][0]["key_id"], next);
    let rest = a_client
        .call(Method::GET, &format!("{keys}?after={next}"), None, None)
        .await
        .unwrap();
    assert_eq!(rest["items"].as_array().unwrap().len(), 2);
    assert!(
        rest["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["key_id"] != next)
    );
    assert_eq!(
        foreign.call(Method::GET, keys, None, None).await.unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        foreign.call(Method::GET, grants, None, None).await.unwrap()["items"],
        json!([])
    );

    let rotate = format!("{keys}/{logical}/rotate");
    assert!(matches!(
        a_client.call(Method::POST, &rotate, None, None).await,
        Err(AppError::Forbidden)
    ));
    service
        .set_workspace_key_grant(&b_admin, a_admin.id, Some("admin"))
        .await
        .unwrap();
    let rotated = a_client
        .call(Method::POST, &rotate, None, None)
        .await
        .unwrap();
    let replacement = rotated["key_id"].as_str().unwrap();
    assert!(matches!(
        service.auth(token).await,
        Err(AppError::Unauthorized)
    ));
    let refreshed = a_client.call(Method::GET, keys, None, None).await.unwrap();
    let row = refreshed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["key_id"] == replacement)
        .unwrap();
    assert_eq!(row["logical_key_id"], logical.to_string());
    assert!(
        !refreshed
            .to_string()
            .contains(rotated["token"].as_str().unwrap())
    );
    a_client
        .call(Method::DELETE, &format!("{keys}/{replacement}"), None, None)
        .await
        .unwrap();
    let incoming = b_client
        .call(Method::GET, grants, None, None)
        .await
        .unwrap();
    let row = incoming["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["logical_key_id"] == logical.to_string())
        .unwrap();
    assert_eq!(row["active"], false);
    host.abort();
}

#[tokio::test]
async fn one_credential_uses_live_workspace_grants_without_sharing_private_archives() {
    let dir = tempfile::tempdir().unwrap();
    let admin_a = bootstrap(dir.path(), "Engineering", None).await;
    let a: Scope = serde_json::from_value(admin_a.clone()).unwrap();
    let admin_b = bootstrap(dir.path(), "Documents", Some(a.workspace_id)).await;
    let b: Scope = serde_json::from_value(admin_b.clone()).unwrap();
    let admin_external = bootstrap(dir.path(), "External", None).await;
    let foreign: Scope = serde_json::from_value(admin_external.clone()).unwrap();
    assert_eq!(a.tenant_id, b.tenant_id);
    assert_ne!(a.workspace_id, b.workspace_id);
    assert_ne!(a.tenant_id, foreign.tenant_id);
    let service = open_service(dir.path(), Models::disabled()).await.unwrap();
    let store = service.engine.relational();
    let writer = store.issue_key(a, "writer").await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = origence::api::router(service.clone());
    let host = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = |key: &str| HostClient::new(&base, key.into()).unwrap();
    let current = client(&writer.token);
    let other = current.clone().with_workspace(b.workspace_id);
    let grant = format!("/admin/workspace-key-grants/{}", writer.key_id);
    let whoami = "/v1/whoami";
    let session = "/v1/session";
    let list = "/v1/workspaces";

    assert_eq!(
        current.call(Method::GET, whoami, None, None).await.unwrap()["workspace_id"],
        a.workspace_id.to_string()
    );
    assert!(matches!(
        other.call(Method::GET, whoami, None, None).await,
        Err(AppError::Forbidden)
    ));
    assert!(matches!(
        client(admin_external["token"].as_str().unwrap())
            .call(Method::PUT, &grant, Some(&json!({"role":"reader"})), None)
            .await,
        Err(AppError::NotFound)
    ));
    let response = reqwest::Client::new()
        .get(format!("{base}{whoami}"))
        .bearer_auth(&writer.token)
        .header("X-Origence-Workspace", a.workspace_id.to_string())
        .header("X-Origence-Workspace", b.workspace_id.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(matches!(
        client("invalid").call(Method::GET, list, None, None).await,
        Err(AppError::Unauthorized)
    ));

    let documents_admin = client(admin_b["token"].as_str().unwrap());
    documents_admin
        .call(Method::PUT, &grant, Some(&json!({"role":"reader"})), None)
        .await
        .unwrap();
    assert_eq!(
        current.call(Method::GET, list, None, None).await.unwrap(),
        json!({"workspaces":[
            {"workspace_id":a.workspace_id,"name":"Engineering","role":"writer","is_default":true},
            {"workspace_id":b.workspace_id,"name":"Documents","role":"reader","is_default":false}
        ]})
    );
    assert_eq!(
        other.call(Method::GET, whoami, None, None).await.unwrap()["role"],
        "reader"
    );
    let a_writer = service.auth(&writer.token).await.unwrap();
    let b_admin_auth = service
        .auth(admin_b["token"].as_str().unwrap())
        .await
        .unwrap();
    let mut assets = Vec::new();
    for (auth, title) in [
        (&a_writer, "Atlas engineering"),
        (&b_admin_auth, "Atlas documents"),
    ] {
        let receipt = service
            .knowledge(
                auth,
                "workspace-local-publish",
                serde_json::from_value(json!({"title":title,"content":title,"format":"text"}))
                    .unwrap(),
            )
            .await
            .unwrap();
        assets.push(receipt["asset_id"].clone());
    }
    while origence::worker::process_next(&service).await.unwrap() {}
    for (scoped, asset) in [(&current, &assets[0]), (&other, &assets[1])] {
        let response = scoped
            .call(
                Method::POST,
                "/v1/search",
                Some(&json!({"query":"Atlas"})),
                None,
            )
            .await
            .unwrap();
        let matches = response["hits"].as_array().unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(&matches[0]["asset_id"], asset);
    }
    let session_a = current
        .call(Method::GET, session, None, None)
        .await
        .unwrap();
    let session_b = other.call(Method::GET, session, None, None).await.unwrap();
    assert_eq!(
        crate::foundation_http::session_from_mcp(&base, &writer.token, Some(b.workspace_id)).await,
        session_b
    );
    assert_eq!(session_a["key_id"], session_b["key_id"]);
    assert_eq!(session_b["workspace_id"], b.workspace_id.to_string());
    assert_ne!(session_a["session_id"], session_b["session_id"]);
    let turn_id = Uuid::new_v4();
    let turn = json!({"turn_id":turn_id,"question":"Synthetic question","answer":"Private A","used_evidence":[]});
    let archive = |session: &serde_json::Value| {
        format!(
            "/v1/sessions/{}/turns",
            session["session_id"].as_str().unwrap()
        )
    };
    let a_archive = archive(&session_a);
    assert!(matches!(
        other
            .call(
                Method::POST,
                &archive(&session_b),
                Some(&turn),
                Some("test")
            )
            .await,
        Err(AppError::Forbidden)
    ));
    current
        .call(Method::POST, &a_archive, Some(&turn), Some("test"))
        .await
        .unwrap();

    let rotate = format!("/admin/keys/{}/rotate", writer.key_id);
    let engineering_admin = client(admin_a["token"].as_str().unwrap());
    assert!(matches!(
        engineering_admin
            .call(Method::POST, &rotate, None, None)
            .await,
        Err(AppError::Forbidden)
    ));
    service
        .set_workspace_key_grant(&b_admin_auth, writer.key_id, Some("writer"))
        .await
        .unwrap();
    let b_write = service
        .auth_workspace(&writer.token, Some(b.workspace_id))
        .await
        .unwrap();
    assert_eq!(b_write.role, "writer");
    assert!(matches!(
        other
            .call(Method::GET, &format!("{a_archive}/{turn_id}"), None, None)
            .await,
        Err(AppError::NotFound)
    ));
    service
        .set_workspace_key_grant(
            &b_admin_auth,
            serde_json::from_value(admin_a["key_id"].clone()).unwrap(),
            Some("admin"),
        )
        .await
        .unwrap();
    let new = engineering_admin
        .call(Method::POST, &rotate, None, None)
        .await
        .unwrap();
    assert!(matches!(
        other.call(Method::GET, whoami, None, None).await,
        Err(AppError::Unauthorized)
    ));
    let replacement = client(new["token"].as_str().unwrap());
    let replacement_b = replacement.clone().with_workspace(b.workspace_id);
    assert_eq!(
        replacement_b
            .call(Method::GET, session, None, None)
            .await
            .unwrap(),
        session_b
    );
    let b_live = service
        .auth_workspace(new["token"].as_str().unwrap(), Some(b.workspace_id))
        .await
        .unwrap();
    engineering_admin
        .call(Method::DELETE, &grant, None, None)
        .await
        .unwrap();
    assert!(matches!(
        replacement.call(Method::GET, whoami, None, None).await,
        Err(AppError::Invalid(_))
    ));
    assert_eq!(
        replacement_b
            .call(Method::GET, whoami, None, None)
            .await
            .unwrap()["role"],
        "writer"
    );
    documents_admin
        .call(Method::DELETE, &grant, None, None)
        .await
        .unwrap();
    assert!(matches!(
        service.private_session(&b_live).await,
        Err(AppError::Forbidden)
    ));
    assert_eq!(
        replacement
            .call(Method::GET, list, None, None)
            .await
            .unwrap(),
        json!({"workspaces":[]})
    );
    host.abort();
    service.engine.shutdown().await.unwrap();
}
