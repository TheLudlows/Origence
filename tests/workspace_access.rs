use crate::support::open_service;
use origence::{
    client::HostClient,
    error::AppError,
    models::Models,
    storage::{RelationalStore, Scope},
};
use reqwest::{Method, StatusCode};
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn mcp_refuses_explicit_workspace_when_host_ignores_selection() {
    let default_workspace = Uuid::new_v4();
    let requested_workspace = Uuid::new_v4();
    let router = axum::Router::new().route(
        "/v1/whoami",
        axum::routing::get(move |headers: axum::http::HeaderMap| async move {
            assert_eq!(
                headers["X-Origence-Workspace"],
                requested_workspace.to_string()
            );
            axum::Json(json!({"workspace_id":default_workspace,"role":"reader"}))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let host = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let key = "oc_synthetic_workspace_startup_check";
    let output = tokio::time::timeout(
        Duration::from_secs(20),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_origence"))
            .args(["--server-url", &base, "mcp", "--workspace"])
            .arg(requested_workspace.to_string())
            .env("OC_API_KEY", key)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("MCP startup exceeded its time budget")
    .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(stderr.contains("Origence host did not confirm the selected workspace"));
    assert!(!stderr.contains(key));
    host.abort();
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
        crate::foundation_http::session_from_stdio(&base, &writer.token, Some(b.workspace_id))
            .await,
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
