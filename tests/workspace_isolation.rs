//! Public library upload/job/search regression for isolated workspaces.
#![cfg(feature = "host")]
use origence::service::Service;
use origence::{
    error::AppError,
    models::Models,
    storage::Scope,
    types::{KnowledgeInput, SearchInput},
    worker,
};
use serde_json::Value;
use uuid::Uuid;

fn id(value: &Value, field: &str) -> Uuid {
    serde_json::from_value(value[field].clone()).unwrap()
}
fn query() -> SearchInput {
    SearchInput {
        query: "Atlas".into(),
        limit: 100,
        mode: "keyword".into(),
        allow_partial: false,
        memory_identity: None,
        components: Default::default(),
    }
}

async fn workspace_pair(
    service: &Service,
) -> (
    (Scope, origence::types::AuthContext),
    (Scope, origence::types::AuthContext),
) {
    let tenant_id = Uuid::new_v4();
    let create = |name| async move {
        let scope = Scope {
            tenant_id,
            workspace_id: Uuid::new_v4(),
        };
        let key = service
            .engine
            .relational()
            .create_key_workspace(scope, name)
            .await
            .unwrap();
        (scope, service.auth(&key.token).await.unwrap())
    };
    (create("Alice").await, create("Bob").await)
}

#[tokio::test]
async fn upload_worker_and_search_stay_scoped_across_workspace_key_rotation() {
    let dir = tempfile::tempdir().unwrap();
    let service = crate::support::open_service(dir.path(), Models::disabled())
        .await
        .unwrap();
    let ((alice_scope, alice), (_, bob)) = workspace_pair(&service).await;
    let mut alice_assets = Vec::new();
    for (auth, content, session) in [
        (&alice, "Atlas 审批需要两人。", "session-1"),
        (&alice, "Atlas release happened on Monday.", "session-2"),
        (&bob, "Atlas 审批不需要两人。", "session-1"),
    ] {
        let file = service
            .upload(auth, session, "session.txt", "text", content.as_bytes())
            .await
            .unwrap();
        let file_id = id(&file, "file_id");
        assert_eq!(
            service.file(auth, file_id).await.unwrap(),
            content.as_bytes()
        );
        let other = if auth.workspace_id == alice.workspace_id {
            &bob
        } else {
            &alice
        };
        assert!(matches!(
            service.file(other, file_id).await,
            Err(AppError::NotFound)
        ));
        let input = KnowledgeInput {
            title: session.into(),
            content: None,
            file_id: Some(file_id),
            format: "text".into(),
            asset_id: None,
            expected_version: None,
        };
        let accepted = service
            .knowledge(auth, session, input.clone())
            .await
            .unwrap();
        assert_eq!(
            service.knowledge(auth, session, input).await.unwrap(),
            accepted
        );
        assert!(worker::process_next(&service).await.unwrap());
        assert_eq!(
            service.job(auth, id(&accepted, "job_id")).await.unwrap()["state"],
            "completed"
        );
        assert!(matches!(
            service.job(other, id(&accepted, "job_id")).await,
            Err(AppError::NotFound)
        ));
        let asset = id(&accepted, "asset_id");
        assert!(matches!(
            service.get(other, asset, None).await,
            Err(AppError::NotFound)
        ));
        if auth.workspace_id == alice.workspace_id {
            alice_assets.push(asset);
        }
    }
    let hits = service.search(&alice, query()).await.unwrap();
    assert_eq!(hits["hits"].as_array().unwrap().len(), 2);
    for hit in hits["hits"].as_array().unwrap() {
        assert!(alice_assets.contains(&id(hit, "asset_id")));
    }
    let hits = service.search(&bob, query()).await.unwrap();
    assert_eq!(hits["hits"].as_array().unwrap().len(), 1);
    assert!(
        hits["hits"][0]["content"]
            .as_str()
            .unwrap()
            .contains("不需要")
    );
    let pending = service
        .knowledge(
            &alice,
            "pending",
            KnowledgeInput {
                title: "pending".into(),
                content: Some("Atlas pending".into()),
                file_id: None,
                format: "text".into(),
                asset_id: None,
                expected_version: None,
            },
        )
        .await
        .unwrap();
    let replacement = service
        .engine
        .relational()
        .rotate_workspace_key(alice_scope)
        .await
        .unwrap();
    let rotated = service.auth(&replacement.token).await.unwrap();
    assert_eq!(
        service.search(&rotated, query()).await.unwrap()["hits"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(matches!(
        service.search(&alice, query()).await,
        Err(AppError::Forbidden)
    ));
    assert!(worker::process_next(&service).await.unwrap());
    let job = service.job(&rotated, id(&pending, "job_id")).await.unwrap();
    assert_eq!(job["state"], "completed");
    assert_eq!(
        service.search(&bob, query()).await.unwrap()["hits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    service.engine.shutdown().await.unwrap();
}

// Fixed model outputs exercise each storage branch, not semantic quality.
#[tokio::test]
async fn workspaces_isolate_summary_graph_and_repeated_queries() {
    use axum::{Json, Router, routing::post};
    use origence::types::{ResolveInput, RetrievalComponents};
    use serde_json::json;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, Router::new()
            .route("/embeddings", post(|| async { Json(json!({"data":[{"embedding":[1.0,0.5]}]})) }))
            .route("/chat/completions", post(|Json(input): Json<Value>| async move {
                let source = input["messages"][1]["content"].as_str().unwrap();
                let owner = if source.contains("ALICE_ONLY") { "ALICE_ONLY" } else { "BOB_ONLY" };
                let content = if input["messages"][0]["content"].as_str().unwrap().contains("Summarize") {
                    format!("summaryneedle {owner}")
                } else {
                    json!({"entities":[{"name":"Atlas","entity_type":"service","description":owner},{"name":owner,"entity_type":"team","description":owner}],"relations":[{"source":"Atlas","predicate":"owned_by","target":owner}]}).to_string()
                };
                Json(json!({"choices":[{"message":{"content":content}}]}))
            })))
            .await.unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let models = Models::configured(
        base,
        "fixture".into(),
        Some("fixture".into()),
        Some("fixture".into()),
        2,
    )
    .unwrap();
    let service = crate::support::open_service(dir.path(), models)
        .await
        .unwrap();
    let ((alice_scope, alice), (bob_scope, bob)) = workspace_pair(&service).await;
    let mut assets = Vec::new();
    for (auth, content) in [
        (&alice, "Atlas ALICE_ONLY requires approval."),
        (&bob, "Atlas BOB_ONLY forbids approval."),
    ] {
        let input = KnowledgeInput {
            title: "same title".into(),
            content: Some(content.into()),
            file_id: None,
            format: "text".into(),
            asset_id: None,
            expected_version: None,
        };
        let accepted = service
            .knowledge(auth, "same-idempotency-key", input.clone())
            .await
            .unwrap();
        assert_eq!(
            service
                .knowledge(auth, "same-idempotency-key", input)
                .await
                .unwrap(),
            accepted
        );
        assert!(worker::process_next(&service).await.unwrap());
        assert_eq!(
            service.job(auth, id(&accepted, "job_id")).await.unwrap()["state"],
            "completed"
        );
        assets.push(id(&accepted, "asset_id"));
    }
    assert_ne!(assets[0], assets[1]);
    // Alternate users repeatedly so cached/reused state cannot hide scope leakage.
    for _ in 0..2 {
        for (index, auth, own, foreign) in [
            (0, &alice, "ALICE_ONLY", "BOB_ONLY"),
            (1, &bob, "BOB_ONLY", "ALICE_ONLY"),
        ] {
            for mode in ["keyword", "vector", "hybrid"] {
                for summaries in [false, true] {
                    for graph in [false, true] {
                        let components = RetrievalComponents { summaries, graph };
                        let result = service
                            .search(
                                auth,
                                SearchInput {
                                    query: "Atlas".into(),
                                    mode: mode.into(),
                                    components: components.clone(),
                                    ..query()
                                },
                            )
                            .await
                            .unwrap();
                        assert_eq!(result["hits"].as_array().unwrap().len(), 1);
                        assert_eq!(id(&result["hits"][0], "asset_id"), assets[index]);
                        assert!(
                            !result
                                .to_string()
                                .to_lowercase()
                                .contains(&foreign.to_lowercase())
                        );
                        assert_eq!(
                            result["graph"]["entities"].as_array().unwrap().len(),
                            if mode == "hybrid" && graph { 2 } else { 0 }
                        );
                        if mode == "hybrid" && graph {
                            assert_eq!(result["graph"]["relations"].as_array().unwrap().len(), 1);
                            assert!(
                                result["graph"]
                                    .to_string()
                                    .to_lowercase()
                                    .contains(&own.to_lowercase())
                            );
                        }
                        let context = service
                            .resolve(
                                auth,
                                ResolveInput {
                                    query: "Atlas".into(),
                                    mode: mode.into(),
                                    components,
                                    budget_tokens: 16000,
                                    allow_partial: false,
                                    memory_identity: None,
                                },
                            )
                            .await
                            .unwrap();
                        assert!(context["rendered_context"].as_str().unwrap().contains(own));
                        assert!(
                            !context
                                .to_string()
                                .to_lowercase()
                                .contains(&foreign.to_lowercase())
                        );
                    }
                }
            }
            // A summary-only token proves the summary branch contributes a score.
            let mut scores = Vec::new();
            for summaries in [false, true] {
                let result = service
                    .search(
                        auth,
                        SearchInput {
                            query: "summaryneedle".into(),
                            mode: "hybrid".into(),
                            components: RetrievalComponents {
                                summaries,
                                graph: false,
                            },
                            ..query()
                        },
                    )
                    .await
                    .unwrap();
                assert_eq!(id(&result["hits"][0], "asset_id"), assets[index]);
                scores.push(result["hits"][0]["score"].as_f64().unwrap());
                assert!(
                    !result
                        .to_string()
                        .to_lowercase()
                        .contains(&foreign.to_lowercase())
                );
            }
            assert!(scores[1] > scores[0]);
        }
    }
    for scope in [alice_scope, bob_scope] {
        service
            .engine
            .relational()
            .rotate_workspace_key(scope)
            .await
            .unwrap();
    }
    for auth in [&alice, &bob] {
        for mode in ["keyword", "vector", "hybrid"] {
            assert!(matches!(
                service
                    .search(
                        auth,
                        SearchInput {
                            mode: mode.into(),
                            ..query()
                        }
                    )
                    .await,
                Err(AppError::Forbidden)
            ));
        }
    }
    service.engine.shutdown().await.unwrap();
    server.abort();
}
