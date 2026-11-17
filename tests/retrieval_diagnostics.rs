//! Public HTTP/worker/SQLite checks; the protocol peer is not the real engine.
use crate::support::{fixture, open_service};
use origence::{
    client::HostClient,
    models::Models,
    storage::{RelationalStore, Scope},
    worker,
};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn run_diagnostics_are_private_durable_and_replay_original_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let service = open_service(dir.path(), Models::disabled()).await.unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    let authority = service.engine.relational();
    authority
        .create_workspace(scope.tenant_id, scope.workspace_id, "diagnostics")
        .await
        .unwrap();
    let owner = authority.issue_key(scope, "writer").await.unwrap();
    let other = authority.issue_key(scope, "writer").await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = origence::api::router(service.clone());
    let host = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = HostClient::new(&base, owner.token.clone()).unwrap();
    client.call(Method::POST, "/v1/memories", Some(&json!({"fact_key":"diagnostics.policy","content":"A deployment requires token review."})), Some("diagnostics-seed")).await.unwrap();
    while worker::process_next(&service).await.unwrap() {}
    let run = Uuid::new_v4();
    client
        .call(
            Method::POST,
            "/v1/runs",
            Some(&json!({"run_id":run,"summary":"Check deployment policy"})),
            None,
        )
        .await
        .unwrap();
    let path = format!("/v1/runs/{run}/context");
    let request = json!({"event_id":Uuid::new_v4(),"context":{"query":"token review"}});
    let first = client
        .call(Method::POST, &path, Some(&request), None)
        .await
        .unwrap();
    let trace = &first["bundle"]["retrieval"]["requests"];
    assert!(trace.as_array().unwrap().len() >= 2);
    assert!(
        trace
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["route"] == "memory")
    );
    assert!(
        trace
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["mode"] == "lexical")
    );
    assert!(
        trace
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["backend_request_id"] == r["backend"]["request_id"])
    );
    let encoded = trace.to_string();
    assert!(!encoded.contains("token review"));
    assert!(!encoded.contains(&owner.token));
    assert!(!encoded.contains("test-owned-key"));
    let second = client
        .call(Method::POST, &path, Some(&request), None)
        .await
        .unwrap();
    assert_eq!(
        first, second,
        "replay must retain original observation IDs and timing"
    );
    let run_path = format!("/v1/runs/{run}");
    let recorded = client
        .call(Method::GET, &run_path, None, None)
        .await
        .unwrap();
    assert_eq!(recorded["events"].as_array().unwrap().len(), 1);
    assert_eq!(
        recorded["events"][0]["report"]["retrieval"],
        first["bundle"]["retrieval"]
    );
    let bundle: origence::evidence::EvidenceBundle =
        serde_json::from_value(first["bundle"].clone()).unwrap();
    assert_eq!(
        recorded["events"][0]["report"]["bundle_digest"],
        origence::storage::hash(serde_json::to_vec(&bundle).unwrap())
    );
    let remote = fixture(dir.path(), None);
    remote.unavailable(true);
    remote.clear_queries();
    assert_eq!(
        first,
        client
            .call(Method::POST, &path, Some(&request), None)
            .await
            .unwrap()
    );
    assert!(
        remote.queries().is_empty(),
        "successful replay must not call retrieval"
    );
    let mut different = request.clone();
    different["context"]["query"] = json!("different query");
    assert!(matches!(
        client
            .call(Method::POST, &path, Some(&different), None)
            .await,
        Err(origence::error::AppError::Conflict(_))
    ));
    assert!(remote.queries().is_empty());
    assert!(
        recorded["events"][0]["report"]
            .get("replay_bundle")
            .is_none()
    );
    let failed_request = json!({"event_id":Uuid::new_v4(),"context":{"query":"PRIVATE QUERY"}});
    let response = reqwest::Client::new()
        .post(format!("{base}{path}"))
        .bearer_auth(&owner.token)
        .json(&failed_request)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let recorded = client
        .call(Method::GET, &run_path, None, None)
        .await
        .unwrap();
    let failed = &recorded["events"][1];
    assert_eq!(failed["evidence_origin"], "server_observed");
    assert_eq!(failed["report"]["kind"], "context_preparation_failed");
    assert_eq!(
        failed["report"]["retrieval"]["requests"][0]["http_status"],
        404
    );
    assert_eq!(
        failed["report"]["retrieval"]["requests"][0]["backend_code"],
        "not_found"
    );
    assert!(!failed.to_string().contains("PRIVATE QUERY"));
    let repeated_failure = reqwest::Client::new()
        .post(format!("{base}{path}"))
        .bearer_auth(&owner.token)
        .json(&failed_request)
        .send()
        .await
        .unwrap();
    assert_eq!(repeated_failure.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        client
            .call(Method::GET, &run_path, None, None)
            .await
            .unwrap()["events"],
        recorded["events"]
    );
    remote.unavailable(false);
    remote.bad_diagnostics(true);
    let invalid = reqwest::Client::new()
        .post(format!("{base}{path}"))
        .bearer_auth(&owner.token)
        .json(&json!({"event_id":Uuid::new_v4(),"context":{"query":"token review"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::SERVICE_UNAVAILABLE);
    let recorded = client
        .call(Method::GET, &run_path, None, None)
        .await
        .unwrap();
    let invalid = &recorded["events"][2]["report"]["retrieval"]["requests"][0];
    assert_eq!(invalid["outcome"], "invalid_backend_diagnostics");
    assert_eq!(invalid["backend"], Value::Null);
    assert!(!recorded.to_string().contains("PRIVATE BAD REQUEST ID"));
    remote.bad_diagnostics(false);
    let outsider = HostClient::new(&base, other.token).unwrap();
    remote.clear_queries();
    assert!(matches!(
        outsider
            .call(Method::POST, &path, Some(&request), None)
            .await,
        Err(origence::error::AppError::NotFound)
    ));
    assert!(remote.queries().is_empty());
    assert!(matches!(
        outsider.call(Method::GET, &run_path, None, None).await,
        Err(origence::error::AppError::NotFound)
    ));
    host.abort();
    let _ = host.await;
    service.engine.shutdown().await.unwrap();
    drop(service);
    remote.unavailable(false);
    let reopened = open_service(dir.path(), Models::disabled()).await.unwrap();
    let auth = reopened.auth(&owner.token).await.unwrap();
    assert_eq!(
        reopened.run_trace(&auth, run, 0, 100).await.unwrap()["events"],
        recorded["events"]
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = origence::api::router(reopened.clone());
    let host = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = HostClient::new(&base, owner.token.clone()).unwrap();
    remote.unavailable(true);
    remote.clear_queries();
    assert_eq!(
        first,
        client
            .call(Method::POST, &path, Some(&request), None)
            .await
            .unwrap()
    );
    assert!(remote.queries().is_empty());
    let admin = reopened
        .engine
        .relational()
        .issue_key(scope, "admin")
        .await
        .unwrap();
    let admin = HostClient::new(&base, admin.token).unwrap();
    let asset_id = first["bundle"]["items"][0]["provenance"]["asset_id"]
        .as_str()
        .unwrap();
    admin
        .call(
            Method::DELETE,
            &format!("/v1/assets/{asset_id}"),
            None,
            Some("withdraw-replay-source"),
        )
        .await
        .unwrap();
    let withdrawn = client.call(Method::POST, &path, Some(&request), None).await;
    assert!(
        matches!(withdrawn, Err(origence::error::AppError::NotFound)),
        "withdrawn preparation: {withdrawn:?}"
    );
    assert!(
        remote.queries().is_empty(),
        "withdrawal must fail before backend IO"
    );
    assert!(matches!(
        client.call(Method::GET, &run_path, None, None).await,
        Err(origence::error::AppError::NotFound)
    ));
    assert!(matches!(
        client
            .call(
                Method::GET,
                &format!("{run_path}/prefix?through_sequence=1"),
                None,
                None
            )
            .await,
        Err(origence::error::AppError::NotFound)
    ));
    admin
        .call(
            Method::DELETE,
            &format!("/admin/keys/{}", owner.key_id),
            None,
            None,
        )
        .await
        .unwrap();
    assert!(matches!(
        client.call(Method::POST, &path, Some(&request), None).await,
        Err(origence::error::AppError::Unauthorized)
    ));
    assert!(remote.queries().is_empty());
    host.abort();
    let _ = host.await;
    reopened.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn configuration_checks_reject_wrong_embedding_space_and_persist_analysis_binding() {
    use origence::storage::anyseek::AnySeekFullTextStore;
    let vector = crate::support::AnySeekFixture::start(Some(2));
    vector.bind_profile(Some("model-a".into()));
    let backend =
        AnySeekFullTextStore::new(vector.config().with_embedding("model-a".into(), 2).unwrap())
            .unwrap();
    backend.check().await.unwrap();
    use origence::storage::{FullTextEntry, SourceVersion, VectorEntry, VectorQuery};
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    let id = Uuid::new_v4();
    let source = SourceVersion {
        source_id: Uuid::new_v4(),
        version: 1,
    };
    backend
        .upsert_combined(
            scope,
            vec![FullTextEntry {
                id,
                source,
                generation: 1,
                search_terms: "alpha".into(),
            }],
            vec![VectorEntry {
                id,
                source,
                generation: 1,
                embedding: vec![1., 0.],
                profile: "model-a".into(),
                dimension: 2,
            }],
            origence::storage::anyseek::operation_epoch().unwrap(),
        )
        .await
        .unwrap();
    let query = VectorQuery {
        scope,
        artifact_ids: Some(vec![id]),
        profile: "model-a".into(),
        dimension: 2,
        generation: 1,
        embedding: vec![1., 0.],
        limit: 1,
    };
    let expected = [(id, source)].into_iter().collect();
    assert_eq!(
        backend
            .search_vector(query.clone(), &expected)
            .await
            .unwrap()
            .len(),
        1
    );
    vector.info_override("embedding_space_id", json!("model-b"));
    assert!(
        backend.search_vector(query, &expected).await.is_err(),
        "query must detect space drift without a health probe"
    );
    assert!(backend.check().await.is_err());
    let dir = tempfile::tempdir().unwrap();
    let remote = fixture(dir.path(), None);
    let service = open_service(dir.path(), Models::disabled()).await.unwrap();
    service
        .engine
        .relational()
        .create_workspace(scope.tenant_id, scope.workspace_id, "analysis binding")
        .await
        .unwrap();
    let key = service
        .engine
        .relational()
        .issue_key(scope, "writer")
        .await
        .unwrap();
    let auth = service.auth(&key.token).await.unwrap();
    service
        .memory(
            &auth,
            "pin-analysis",
            serde_json::from_value(
                json!({"fact_key":"analysis.policy", "content":"Pin analysis before publication"}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    while worker::process_next(&service).await.unwrap() {}
    service.engine.shutdown().await.unwrap();
    drop(service);
    let binding = std::fs::read(dir.path().join("full-text-backend.json")).unwrap();
    let mut info: Value = reqwest::Client::new()
        .get(format!(
            "{}/api/v1/index/ws_{}",
            remote.base,
            scope.workspace_id.simple()
        ))
        .header("apikey", "test-owned-key")
        .header("X-AnySeek-Tenant", scope.tenant_id.to_string())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let original_analysis = info["analysis"].clone();
    info["analysis"]["fingerprint"] =
        json!(origence::storage::hash("different native configuration"));
    remote.info_override("analysis", info["analysis"].clone());
    assert!(open_service(dir.path(), Models::disabled()).await.is_err());
    assert_eq!(
        binding,
        std::fs::read(dir.path().join("full-text-backend.json")).unwrap()
    );
    remote.info_override("analysis", original_analysis);
    let reopened = open_service(dir.path(), Models::disabled()).await.unwrap();
    reopened.engine.shutdown().await.unwrap();
    drop(reopened);
    let created = remote.creations();
    remote.remove_collection(scope);
    assert!(open_service(dir.path(), Models::disabled()).await.is_err());
    assert_eq!(
        remote.creations(),
        created,
        "restart must not recreate a missing published collection"
    );
}
