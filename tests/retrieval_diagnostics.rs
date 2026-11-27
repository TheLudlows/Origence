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
async fn context_query_embedding_usage_is_scoped_and_replayed() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let model_calls = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let model_base = format!("http://{}", listener.local_addr().unwrap());
    let handler = {
        let model_calls = model_calls.clone();
        move |axum::Json(body): axum::Json<Value>| {
            let model_calls = model_calls.clone();
            async move {
                model_calls.fetch_add(1, Ordering::SeqCst);
                let input = body["input"].as_str().unwrap();
                let mut response = json!({"data":[{"embedding":[1.0,0.5,0.25]}],"unused_provider_body":"PRIVATE MODEL RESPONSE"});
                let usage = match input {
                    "PRIVATE QUERY known" => Some(json!(7)),
                    "explicit zero" => Some(json!(0)),
                    "malformed usage" => Some(json!("0")),
                    "out of range usage" => Some(json!(u64::MAX)),
                    "invalid vector" => {
                        response["data"][0]["embedding"] = json!([0, 0, 0]);
                        Some(json!(4))
                    }
                    _ => None,
                };
                response["usage"] = json!({"total_tokens":99,"completion_tokens":99});
                if let Some(usage) = usage {
                    response["usage"]["prompt_tokens"] = usage;
                }
                axum::Json(response)
            }
        }
    };
    let app = axum::Router::new().route("/embeddings", axum::routing::post(handler));
    let model_host = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let models = Models::configured(
        model_base.clone(),
        "private-fixture-model-key".into(),
        Some("private-fixture-model-label".into()),
        None,
        3,
    )
    .unwrap();
    let fingerprint = origence::storage::hash(models.profile.as_ref().unwrap());
    let dir = tempfile::tempdir().unwrap();
    let service = open_service(dir.path(), models).await.unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    let store = service.engine.relational();
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "query embedding usage")
        .await
        .unwrap();
    let owner = store.issue_key(scope, "writer").await.unwrap();
    let admin = store.issue_key(scope, "admin").await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = origence::api::router(service);
    let host = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = HostClient::new(&base, owner.token).unwrap();
    let admin = HostClient::new(&base, admin.token).unwrap();
    let run = Uuid::new_v4();
    let run_path = format!("/v1/runs/{run}");
    let context_path = format!("{run_path}/context");
    let get = |path: String| {
        let client = &client;
        async move { client.call(Method::GET, &path, None, None).await.unwrap() }
    };
    let prepare = |request: Value| {
        let (client, path) = (&client, &context_path);
        async move { client.call(Method::POST, path, Some(&request), None).await }
    };
    let request =
        |query, mode| json!({"event_id":Uuid::new_v4(),"context":{"query":query,"mode":mode}});
    let observation = |value: &Value| value["retrieval"]["context_query_embedding"].clone();
    let expected = |tokens: Value| {
        json!({
            "model_config_sha256":fingerprint,
            "provider_reported_prompt_tokens":tokens,
        })
    };

    client
        .call(
            Method::POST,
            "/v1/runs",
            Some(&json!({"run_id":run,"summary":"Synthetic private Run"})),
            None,
        )
        .await
        .unwrap();
    let forged = json!({
        "event_id": Uuid::new_v4(),
        "reporter": "client",
        "observed_at_ms": 0,
        "event": {
            "kind": "context_prepared",
            "retrieval": {"context_query_embedding":expected(json!(42))},
        },
    });
    assert!(
        client
            .call(
                Method::POST,
                &format!("{run_path}/events"),
                Some(&forged),
                None
            )
            .await
            .is_err()
    );
    let known = request("PRIVATE QUERY known", "hybrid");
    assert!(
        admin
            .call(Method::POST, &context_path, Some(&known), None)
            .await
            .is_err()
    );
    assert_eq!(model_calls.load(Ordering::SeqCst), 0);
    let first = prepare(known.clone()).await.unwrap();
    assert_eq!(
        model_calls.load(Ordering::SeqCst),
        1,
        "one query embedding for all routes"
    );
    assert_eq!(observation(&first["bundle"]), expected(json!(7)));
    let prefix_path = format!("{run_path}/prefix?through_sequence=1");
    let prefix = get(prefix_path.clone()).await;
    assert_eq!(prepare(known).await.unwrap(), first);
    assert_eq!(get(prefix_path).await, prefix);
    assert_eq!(
        model_calls.load(Ordering::SeqCst),
        1,
        "successful replay did not call the endpoint"
    );

    let preview = client
        .call(
            Method::POST,
            "/v1/context",
            Some(&json!({"query":"explicit zero","mode":"vector"})),
            None,
        )
        .await
        .unwrap();
    assert_eq!(observation(&preview), expected(json!(0)));
    assert_eq!(
        get(run_path.clone()).await["events"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "preview does not append to a Run"
    );
    for (query, tokens) in [
        ("explicit zero", json!(0)),
        ("missing prompt usage", Value::Null),
        ("malformed usage", Value::Null),
        ("out of range usage", Value::Null),
    ] {
        let result = prepare(request(query, "vector")).await.unwrap();
        assert_eq!(observation(&result["bundle"]), expected(tokens));
    }
    let model_calls_before_keyword = model_calls.load(Ordering::SeqCst);
    let keyword = prepare(request("keyword only", "keyword")).await.unwrap();
    assert!(
        keyword["bundle"]["retrieval"]
            .get("context_query_embedding")
            .is_none()
    );
    assert_eq!(
        model_calls.load(Ordering::SeqCst),
        model_calls_before_keyword
    );
    assert!(matches!(
        prepare(request("invalid vector", "vector")).await,
        Err(origence::error::AppError::Unavailable(_))
    ));

    let trace = get(run_path.clone()).await;
    let events = trace["events"].as_array().unwrap();
    assert_eq!(events.len(), 7);
    assert_eq!(observation(&events[0]["report"]), expected(json!(7)));
    assert_eq!(observation(&events[1]["report"]), expected(json!(0)));
    assert_eq!(observation(&events[2]["report"]), expected(Value::Null));
    assert!(
        events[5]["report"]["retrieval"]
            .get("context_query_embedding")
            .is_none()
    );
    let failed = &events[6];
    assert_eq!(failed["report"]["kind"], "context_preparation_failed");
    assert_eq!(observation(&failed["report"]), expected(json!(4)));
    let admin_trace = admin
        .call(Method::GET, &run_path, None, None)
        .await
        .unwrap();
    assert_eq!(admin_trace["is_owner"], false);
    assert_eq!(admin_trace["events"], trace["events"]);
    let trace = trace.to_string();
    for private in [
        "PRIVATE QUERY",
        "PRIVATE MODEL RESPONSE",
        &model_base,
        "private-fixture-model-key",
        "private-fixture-model-label",
    ] {
        assert!(!trace.contains(private));
    }
    model_host.abort();
    host.abort();
}

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
                payload: origence::storage::ChunkPayload {
                    content: "alpha".into(),
                    locator: json!({}),
                    summary: None,
                },
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
