use crate::support::open_service;
use origence::{error::AppError, models::Models, service::GlobalKnowledgeDecision, storage::Scope};
use reqwest::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

fn id(receipt: &Value, name: &str) -> Uuid {
    serde_json::from_value(receipt[name].clone()).unwrap()
}

#[tokio::test]
async fn global_publication_is_pinned_moderated_and_recalled_across_tenants() {
    let dir = tempfile::tempdir().unwrap();
    let service = open_service(dir.path(), Models::disabled()).await.unwrap();
    let store = service.engine.relational();
    let owner = store
        .create_key_workspace(
            Scope {
                tenant_id: Uuid::new_v4(),
                workspace_id: Uuid::new_v4(),
            },
            "Origin",
        )
        .await
        .unwrap();
    let reader = store
        .create_key_workspace(
            Scope {
                tenant_id: Uuid::new_v4(),
                workspace_id: Uuid::new_v4(),
            },
            "Another tenant",
        )
        .await
        .unwrap();
    let admin = store.rotate_deployment_admin().await.unwrap();
    let author = service.auth(&owner.token).await.unwrap();
    let reader = service.auth(&reader.token).await.unwrap();
    let text = "Cascade 原始版本。Deploy the Zürich service on Mondays.";
    let source = service
        .knowledge(
            &author,
            "knowledge",
            serde_json::from_value(json!({
                "title":"Cascade guide","content":text
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let asset = id(&source, "asset_id");
    let memory = service
        .memory(
            &reader,
            "local-memory",
            serde_json::from_value(json!({
                "fact_key":"local","content":"Cascade is also configured in this workspace."
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    while origence::worker::process_next(&service).await.unwrap() {}

    assert!(matches!(
        service.submit_global_knowledge(&reader, asset, 1).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        service
            .submit_global_knowledge(&reader, id(&memory, "asset_id"), 1)
            .await,
        Err(AppError::Invalid(_))
    ));
    let backend = crate::support::fixture(dir.path(), None);
    backend.lose_next_write();
    assert!(
        service
            .submit_global_knowledge(&author, asset, 1)
            .await
            .is_err()
    );
    assert!(
        service
            .global_review_catalog(&admin, "pending", Uuid::nil(), 10)
            .await
            .unwrap()["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let attempts = backend.posts();
    let submission = service
        .submit_global_knowledge(&author, asset, 1)
        .await
        .unwrap();
    assert_eq!(
        backend.posts(),
        attempts,
        "retry must reuse the native write"
    );
    let publication = id(&submission, "publication_id");
    assert_eq!(submission["state"], "pending");
    assert_eq!(
        service
            .submit_global_knowledge(&author, asset, 1)
            .await
            .unwrap(),
        submission
    );
    assert!(matches!(
        service.global_knowledge(&reader, publication).await,
        Err(AppError::NotFound)
    ));
    assert_eq!(
        service
            .global_review_detail(&admin, publication)
            .await
            .unwrap()["content"],
        text
    );
    assert_eq!(
        service
            .global_review_catalog(&admin, "pending", Uuid::nil(), 1)
            .await
            .unwrap()["items"][0]["asset_id"],
        json!(asset)
    );
    let search: origence::types::SearchInput =
        serde_json::from_value(json!({"query":"Cascade"})).unwrap();
    assert_eq!(
        service.search(&reader, search.clone()).await.unwrap()["hits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    service.knowledge(&author, "new-version", serde_json::from_value(json!({
        "asset_id":asset,"expected_version":1,"title":"Cascade revised","content":"Cascade now deploys on Fridays."
    })).unwrap()).await.unwrap();
    while origence::worker::process_next(&service).await.unwrap() {}
    let rejected = service
        .submit_global_knowledge(&author, asset, 2)
        .await
        .unwrap();
    let rejected = id(&rejected, "publication_id");
    service
        .review_global_knowledge(&admin, rejected, GlobalKnowledgeDecision::Reject)
        .await
        .unwrap();
    assert!(matches!(
        service
            .review_global_knowledge(&admin, rejected, GlobalKnowledgeDecision::Approve)
            .await,
        Err(AppError::Conflict(_))
    ));
    service.engine.shutdown().await.unwrap();
    drop(service);

    let service = open_service(dir.path(), Models::disabled()).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = origence::api::router(service.clone());
    let host = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let http = reqwest::Client::new();
    let review_path = format!("{base}/admin/deployment/global-knowledge/{publication}/review");
    let approval = json!({"decision":"approve"});
    assert_eq!(
        http.post(&review_path)
            .bearer_auth(owner.token)
            .json(&approval)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        http.post(&review_path)
            .bearer_auth(&admin)
            .json(&approval)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let snapshot = service
        .global_knowledge(&reader, publication)
        .await
        .unwrap();
    assert_eq!(snapshot["content"], text);
    assert_eq!(snapshot["version"], 1);
    assert!(snapshot.get("workspace_id").is_none() && snapshot.get("source_event_id").is_none());
    assert!(matches!(
        service.get(&reader, asset, Some(1)).await,
        Err(AppError::NotFound)
    ));
    assert_eq!(
        service
            .global_knowledge_catalog(&reader, Uuid::nil(), 10)
            .await
            .unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let hits = service.search(&reader, search.clone()).await.unwrap();
    assert_eq!(hits["hits"][0]["asset_id"], memory["asset_id"]);
    assert_eq!(hits["hits"][0]["scope"], "workspace");
    assert_eq!(hits["hits"][1]["asset_id"], json!(publication));
    assert_eq!(hits["hits"][1]["scope"], "global");
    assert_eq!(hits["hits"][1]["content"], text);
    let resolve = service
        .resolve(
            &reader,
            serde_json::from_value(json!({
                "query":"Cascade","budget_tokens":4000
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resolve["sources"][1]["scope"], "global");
    let query: origence::evidence::EvidenceInput =
        serde_json::from_value(json!({"query":"Cascade"})).unwrap();
    let context = service
        .evidence_bundle(&reader, query.clone())
        .await
        .unwrap();
    assert!(context.rendered_context.contains(text));
    let run = Uuid::new_v4();
    service
        .create_run(
            &reader,
            serde_json::from_value(json!({"run_id":run,"summary":"global citation"})).unwrap(),
        )
        .await
        .unwrap();
    let event = Uuid::new_v4();
    let prepared = service
        .prepare_run_context(&reader, run, event, query.clone())
        .await
        .unwrap();
    assert!(
        prepared["bundle"]["rendered_context"]
            .as_str()
            .unwrap()
            .contains(text)
    );

    // The published copy remains readable when the source workspace changes its content.
    service
        .delete(&author, "remove-private-source", asset, "asset")
        .await
        .unwrap();
    assert_eq!(
        service
            .global_knowledge(&reader, publication)
            .await
            .unwrap()["content"],
        text
    );
    let native_chunk = backend
        .snapshot_records()
        .into_iter()
        .find(|record| record["document"]["source_id"] == json!(publication))
        .unwrap();
    let chunk = id(&native_chunk["document"], "chunk_id");
    backend.mutate_payload(chunk, |payload| {
        payload["content"] = json!("unverified text")
    });
    assert!(matches!(
        service.global_knowledge(&reader, publication).await,
        Err(AppError::Unavailable(_))
    ));
    assert!(matches!(
        service.search(&reader, search.clone()).await,
        Err(AppError::Unavailable(_))
    ));
    backend.mutate_payload(chunk, |payload| {
        *payload = native_chunk["document"]["payload"].clone()
    });
    let gate = backend.pause_next_content_read();
    let concurrent = tokio::spawn({
        let service = service.clone();
        let reader = reader.clone();
        let search = search.clone();
        async move { service.search(&reader, search).await }
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), gate.entered.notified())
        .await
        .expect("global body read did not start");
    service
        .review_global_knowledge(&admin, publication, GlobalKnowledgeDecision::Withdraw)
        .await
        .unwrap();
    gate.resume.notify_one();
    let returned = concurrent.await.unwrap().unwrap();
    assert_eq!(returned["hits"].as_array().unwrap().len(), 1);
    assert_eq!(
        id(&returned["hits"][0], "asset_id"),
        id(&memory, "asset_id")
    );
    assert!(matches!(
        service.global_knowledge(&reader, publication).await,
        Err(AppError::NotFound)
    ));
    assert_eq!(
        service.search(&reader, search).await.unwrap()["hits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(
        service
            .prepare_run_context(&reader, run, event, query)
            .await,
        Err(AppError::NotFound)
    ));
    let trace = service.run_trace(&reader, run, 0, 20).await.unwrap();
    assert_eq!(
        trace["events"][0]["report"]["global_sources_unavailable"],
        1
    );
    assert!(!trace.to_string().contains(text));
    host.abort();
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn global_search_merges_top_hits_across_candidate_batches() {
    let dir = tempfile::tempdir().unwrap();
    let service = open_service(dir.path(), Models::disabled()).await.unwrap();
    let store = service.engine.relational();
    let workspace = |name| async move {
        let issued = store
            .create_key_workspace(
                Scope {
                    tenant_id: Uuid::new_v4(),
                    workspace_id: Uuid::new_v4(),
                },
                name,
            )
            .await
            .unwrap();
        issued.token
    };
    let writer = service.auth(&workspace("writer").await).await.unwrap();
    let reader = service.auth(&workspace("reader").await).await.unwrap();
    let admin = store.rotate_deployment_admin().await.unwrap();
    let mut matching = Vec::new();
    // Four 64-chunk versions and one more chunk cross the native query batch
    // boundary. Both matching chunks compete for the same response limit.
    for n in 0..5 {
        let content = match n {
            0 => {
                let first = format!("{:<2400}", "reviewboundary filler filler filler");
                first + &format!("{:<2400}", "filler").repeat(63)
            }
            4 => "reviewboundary reviewboundary".into(),
            _ => format!("{:<2400}", "filler").repeat(64),
        };
        let accepted = service
            .knowledge(
                &writer,
                &format!("batch-{n}"),
                serde_json::from_value(json!({
                    "title":format!("Bulk {n}"), "format":"text", "content":content
                }))
                .unwrap(),
            )
            .await
            .unwrap();
        assert!(origence::worker::process_next(&service).await.unwrap());
        let release = service
            .submit_global_knowledge(&writer, id(&accepted, "asset_id"), 1)
            .await
            .unwrap();
        let publication = id(&release, "publication_id");
        service
            .review_global_knowledge(&admin, publication, GlobalKnowledgeDecision::Approve)
            .await
            .unwrap();
        if n == 0 || n == 4 {
            matching.push(publication);
        }
    }
    let peer = crate::support::fixture(dir.path(), None);
    peer.clear_queries();
    let hits = service
        .search(
            &reader,
            serde_json::from_value(json!({
                "query":"reviewboundary", "mode":"keyword", "limit":2
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(hits["hits"].as_array().unwrap().len(), 2);
    assert_eq!(id(&hits["hits"][0], "asset_id"), matching[1]);
    assert_eq!(id(&hits["hits"][1], "asset_id"), matching[0]);
    let keyword_queries = peer
        .queries()
        .into_iter()
        .filter(|query| {
            query["query"]
                .as_str()
                .is_some_and(|terms| !terms.is_empty())
        })
        .count();
    assert_eq!(keyword_queries, 2);
    service.engine.shutdown().await.unwrap();
}
