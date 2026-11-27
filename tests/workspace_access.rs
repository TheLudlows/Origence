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
