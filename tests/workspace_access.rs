use crate::support::open_service;
use origence::{client::HostClient, error::AppError, models::Models, storage::Scope};
use reqwest::{Method, StatusCode};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn deployment_admin_reads_across_tenants_and_requires_explicit_write_scope() {
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
    store.rotate_workspace_key(first).await.unwrap();

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
async fn deployment_admin_content_writes_use_workspace_scope_and_audit_actor() {
    let dir = tempfile::tempdir().unwrap();
    let service = open_service(dir.path(), Models::disabled()).await.unwrap();
    let store = service.engine.relational();
    let first = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    let second = Scope {
        tenant_id: first.tenant_id,
        workspace_id: Uuid::new_v4(),
    };
    let first_key = store.create_key_workspace(first, "First").await.unwrap();
    let second_key = store.create_key_workspace(second, "Second").await.unwrap();
    let admin = store.rotate_deployment_admin().await.unwrap();
    let actor = service
        .authorize_deployment_workspace(&admin, first.workspace_id)
        .await
        .unwrap();
    assert!(!actor.id.is_nil());
    assert_ne!(actor.id, first_key.key_id);
    service
        .register_memory_property(
            &actor,
            "test-property",
            serde_json::from_value(
                json!({"predicate":"hotel.limit","cardinality":"single","required_context":[]}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = origence::api::router(service.clone());
    let host = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    // A workspace header never overrides the selected deployment path.
    let operator = HostClient::new(&base, admin.clone())
        .unwrap()
        .with_workspace(second.workspace_id);
    let agent = HostClient::new(&base, first_key.token).unwrap();
    let other_agent = HostClient::new(&base, second_key.token).unwrap();
    let a = format!("/admin/deployment/workspaces/{}", first.workspace_id);
    let b = format!("/admin/deployment/workspaces/{}", second.workspace_id);
    let publish = format!("{a}/knowledge");
    let input = json!({"title":"Internal policy","content":"Original policy"});
    assert!(matches!(
        agent
            .call(Method::POST, &publish, Some(&input), Some("write"))
            .await,
        Err(AppError::Unauthorized)
    ));
    assert!(matches!(
        operator
            .call(Method::POST, &publish, Some(&input), None)
            .await,
        Err(AppError::Invalid(_))
    ));
    let created = operator
        .call(Method::POST, &publish, Some(&input), Some("write"))
        .await
        .unwrap();
    assert_eq!(
        operator
            .call(Method::POST, &publish, Some(&input), Some("write"))
            .await
            .unwrap(),
        created
    );
    let asset = created["asset_id"].as_str().unwrap();
    let job = created["job_id"].as_str().unwrap();
    let job_path = format!("{a}/jobs/{job}");
    assert!(matches!(
        operator
            .call(Method::GET, &format!("{b}/jobs/{job}"), None, None)
            .await,
        Err(AppError::NotFound)
    ));
    for (action, state) in [("cancel", "cancelled"), ("retry", "pending")] {
        assert_eq!(
            operator
                .call(
                    Method::POST,
                    &format!("{job_path}/{action}"),
                    None,
                    Some(action)
                )
                .await
                .unwrap()["state"],
            state
        );
    }
    while origence::worker::process_next(&service).await.unwrap() {}
    assert_eq!(
        operator
            .call(Method::GET, &job_path, None, None)
            .await
            .unwrap()["state"],
        "completed"
    );
    let update = json!({"title":"Internal policy","content":"Revised policy","asset_id":asset,"expected_version":1});
    assert!(matches!(
        operator
            .call(
                Method::POST,
                &format!("{b}/knowledge"),
                Some(&update),
                Some("update")
            )
            .await,
        Err(AppError::NotFound)
    ));
    operator
        .call(Method::POST, &publish, Some(&update), Some("update"))
        .await
        .unwrap();

    let written_memory = operator.call(Method::POST, &format!("{a}/memories/identified"), Some(&json!({
        "identity":{"subject":{"kind":"service","stable_id":"atlas"},"predicate":"db.engine","context":{}},
        "content":"Atlas uses SQLite"
    })), Some("fact")).await.unwrap();
    let semantic_memory = operator.call(Method::POST, &format!("{a}/memories/semantic"), Some(&json!({
        "identity":{"subject":{"kind":"team","stable_id":"operations"},"predicate":"hotel.limit","context":{}},
        "content":"80", "operation":"assert", "expected_version":0,
        "statements":[{"value":"80","quote":"80","byte_start":0,"byte_end":2}]
    })), Some("semantic")).await.unwrap();
    let uploaded: serde_json::Value = reqwest::Client::new()
        .post(format!("{base}{a}/files?name=note.txt&format=text"))
        .bearer_auth(&admin)
        .header("idempotency-key", "upload")
        .body("Separate attached text")
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let document = json!({"title":"Attached note","format":"text","file_id":uploaded["file_id"]});
    assert!(matches!(
        operator
            .call(
                Method::POST,
                &format!("{b}/knowledge"),
                Some(&document),
                Some("attached")
            )
            .await,
        Err(AppError::NotFound)
    ));
    let attached = operator
        .call(Method::POST, &publish, Some(&document), Some("attached"))
        .await
        .unwrap();
    while origence::worker::process_next(&service).await.unwrap() {}
    for (receipt, content) in [
        (&written_memory, "Atlas uses SQLite"),
        (&semantic_memory, "80"),
        (&attached, "Separate attached text"),
    ] {
        let path = format!("/v1/assets/{}", receipt["asset_id"].as_str().unwrap());
        assert_eq!(
            agent.call(Method::GET, &path, None, None).await.unwrap()["content"],
            content
        );
        assert!(matches!(
            other_agent.call(Method::GET, &path, None, None).await,
            Err(AppError::NotFound)
        ));
    }
    let normal_path = format!("/v1/assets/{asset}");
    assert_eq!(
        agent
            .call(Method::GET, &normal_path, None, None)
            .await
            .unwrap()["content"],
        "Revised policy"
    );
    assert_eq!(
        operator
            .call(
                Method::GET,
                &format!("{a}/assets/{asset}?version=1"),
                None,
                None
            )
            .await
            .unwrap()["content"],
        "Original policy"
    );
    assert!(matches!(
        operator
            .call(
                Method::DELETE,
                &format!("{b}/assets/{asset}"),
                None,
                Some("remove")
            )
            .await,
        Err(AppError::NotFound)
    ));
    let delete_path = format!("{a}/assets/{asset}");
    let deleted = operator
        .call(Method::DELETE, &delete_path, None, Some("remove"))
        .await
        .unwrap();
    assert_eq!(deleted["blocked"], true);
    assert_eq!(
        operator
            .call(Method::DELETE, &delete_path, None, Some("remove"))
            .await
            .unwrap(),
        deleted
    );
    assert!(matches!(
        agent.call(Method::GET, &normal_path, None, None).await,
        Err(AppError::NotFound)
    ));

    let audit: Vec<(String, Uuid)> = origence::storage::sqlite_db::query_as(
        "SELECT action,actor FROM oc_audit WHERE tenant_id=? AND workspace_id=? AND target=? AND action IN ('knowledge.ingest','source.logical_delete') ORDER BY action",
    )
    .bind(first.tenant_id).bind(first.workspace_id).bind(Uuid::parse_str(asset).unwrap())
    .fetch_all(store.pool()).await.unwrap();
    assert_eq!(
        audit,
        vec![
            ("knowledge.ingest".into(), actor.id),
            ("knowledge.ingest".into(), actor.id),
            ("source.logical_delete".into(), actor.id)
        ]
    );
    store.rotate_deployment_admin().await.unwrap();
    assert!(matches!(
        service.job(&actor, Uuid::parse_str(job).unwrap()).await,
        Err(AppError::Forbidden)
    ));
    assert!(matches!(
        operator.call(Method::GET, &job_path, None, None).await,
        Err(AppError::Unauthorized)
    ));
    host.abort();
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn administrator_restores_valid_history_and_withdraws_only_scoped_content_sources() {
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
    let workspace_key = store.create_key_workspace(scope, "First").await.unwrap();
    store.create_key_workspace(other, "Other").await.unwrap();
    let admin = store.rotate_deployment_admin().await.unwrap();
    let actor = service
        .authorize_deployment_workspace(&admin, scope.workspace_id)
        .await
        .unwrap();
    let agent = service.auth(&workspace_key.token).await.unwrap();
    let first = service
        .knowledge(
            &agent,
            "v1",
            serde_json::from_value(json!({
                "title":"Hotel policy","content":"100 per night"
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let asset: Uuid = serde_json::from_value(first["asset_id"].clone()).unwrap();
    let source: Uuid = serde_json::from_value(first["source_event_id"].clone()).unwrap();
    while origence::worker::process_next(&service).await.unwrap() {}
    service.knowledge(&agent, "v2", serde_json::from_value(json!({
        "title":"Hotel policy","content":"80 per night","asset_id":asset,"expected_version":1
    })).unwrap()).await.unwrap();
    let session: Uuid = serde_json::from_value(
        service.private_session(&agent).await.unwrap()["session_id"].clone(),
    )
    .unwrap();
    let turn = Uuid::new_v4();
    let archived = service.archive_turn(&agent, "archive", session, serde_json::from_value(json!({
        "turn_id":turn,"question":"Private question","answer":"Private answer","used_evidence":[]
    })).unwrap()).await.unwrap();
    let archive_source = archived["source_event_id"].as_str().unwrap();
    while origence::worker::process_next(&service).await.unwrap() {}

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let router = origence::api::router(service.clone());
    let host = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = HostClient::new(&origin, admin.clone()).unwrap();
    let ordinary = HostClient::new(&origin, workspace_key.token).unwrap();
    let selected = format!("/admin/deployment/workspaces/{}", scope.workspace_id);
    let outside = format!("/admin/deployment/workspaces/{}", other.workspace_id);
    let restore_path = format!("{selected}/assets/{asset}/restore");
    let retract_path = format!("{selected}/events/{source}");
    let restore =
        json!({"target_version":1,"expected_version":2,"reason":"Correct mistaken revision"});
    assert!(matches!(
        ordinary
            .call(Method::POST, &restore_path, Some(&restore), Some("restore"))
            .await,
        Err(AppError::Unauthorized)
    ));
    assert!(matches!(
        ordinary
            .call(Method::DELETE, &retract_path, None, Some("withdraw"))
            .await,
        Err(AppError::Unauthorized)
    ));
    assert!(matches!(
        client
            .call(
                Method::POST,
                &format!("{outside}/assets/{asset}/restore"),
                Some(&restore),
                Some("restore")
            )
            .await,
        Err(AppError::NotFound)
    ));
    for method in [Method::GET, Method::DELETE] {
        assert!(matches!(
            client
                .call(
                    method.clone(),
                    &format!("{outside}/events/{source}"),
                    None,
                    Some("withdraw")
                )
                .await,
            Err(AppError::NotFound)
        ));
        assert!(matches!(
            client
                .call(
                    method,
                    &format!("{selected}/events/{archive_source}"),
                    None,
                    Some("withdraw")
                )
                .await,
            Err(AppError::Forbidden)
        ));
    }
    assert_eq!(
        service.session_turn(&agent, session, turn).await.unwrap()["answer"],
        "Private answer"
    );
    assert_eq!(
        client
            .call(Method::GET, &retract_path, None, None)
            .await
            .unwrap()["content"],
        "100 per night"
    );
    assert!(matches!(
        client
            .call(
                Method::POST,
                &restore_path,
                Some(&json!({"target_version":1,"expected_version":1,"reason":"Stale revision"})),
                Some("stale")
            )
            .await,
        Err(AppError::Conflict(_))
    ));
    let receipt = client
        .call(Method::POST, &restore_path, Some(&restore), Some("restore"))
        .await
        .unwrap();
    assert_eq!(
        client
            .call(Method::POST, &restore_path, Some(&restore), Some("restore"))
            .await
            .unwrap(),
        receipt
    );
    while origence::worker::process_next(&service).await.unwrap() {}
    let restored = service.get(&agent, asset, None).await.unwrap();
    assert_eq!(restored["version"], 3);
    assert_eq!(restored["restored_from"], 1);
    assert_eq!(restored["content"], "100 per night");
    assert_eq!(restored["source_event_id"], json!(source));

    let pending = service
        .restore(
            &agent,
            "pending-restore",
            asset,
            serde_json::from_value(
                json!({"target_version":1,"expected_version":3,"reason":"Pending restore"}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let pending_job = serde_json::from_value(pending["job_id"].clone()).unwrap();
    let removed = client
        .call(Method::DELETE, &retract_path, None, Some("withdraw"))
        .await
        .unwrap();
    assert_eq!(removed["originals_retained"], true);
    assert_eq!(
        service.job(&agent, pending_job).await.unwrap()["state"],
        "cancelled"
    );
    assert_eq!(
        client
            .call(Method::DELETE, &retract_path, None, Some("withdraw"))
            .await
            .unwrap(),
        removed
    );
    assert!(matches!(
        service
            .inspect_content_source(&admin, scope.workspace_id, source)
            .await,
        Err(AppError::NotFound)
    ));
    for version in [1, 3] {
        assert!(matches!(
            service.get(&agent, asset, Some(version)).await,
            Err(AppError::NotFound)
        ));
    }
    assert_eq!(
        service.get(&agent, asset, Some(2)).await.unwrap()["content"],
        "80 per night"
    );
    assert!(matches!(
        client
            .call(
                Method::POST,
                &restore_path,
                Some(&json!({"target_version":1,"expected_version":3,"reason":"Revoked source"})),
                Some("revoked")
            )
            .await,
        Err(AppError::NotFound)
    ));
    while origence::worker::process_next(&service).await.unwrap() {}
    let retained: (String, String) = origence::storage::sqlite_db::query_as(
        "SELECT state,content FROM oc_events WHERE tenant_id=? AND workspace_id=? AND id=?",
    )
    .bind(scope.tenant_id)
    .bind(scope.workspace_id)
    .bind(source)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(retained, ("retracted".into(), "100 per night".into()));
    let audited: Vec<(String, Uuid)> = origence::storage::sqlite_db::query_as(
        "SELECT action,target FROM oc_audit WHERE tenant_id=? AND workspace_id=? AND actor=? AND action IN ('asset.restore_authorized','source.logical_delete') ORDER BY action",
    ).bind(scope.tenant_id).bind(scope.workspace_id).bind(actor.id).fetch_all(store.pool()).await.unwrap();
    assert_eq!(
        audited,
        vec![
            ("asset.restore_authorized".into(), asset),
            ("source.logical_delete".into(), source)
        ]
    );
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
    let http = reqwest::Client::new();
    for (method, path) in [
        (Method::GET, "/admin/keys".to_string()),
        (Method::POST, "/admin/keys".to_string()),
        (Method::POST, format!("/admin/keys/{}/rotate", key.key_id)),
        (Method::DELETE, format!("/admin/keys/{}", key.key_id)),
    ] {
        assert_eq!(
            http.request(method, format!("{base}{path}"))
                .bearer_auth(&key.token)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND,
            "{path}",
        );
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
