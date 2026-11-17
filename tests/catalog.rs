//! Real Turso + Service + HTTP contracts for the UI's bounded catalogs.
use crate::support::open_service;
use origence::{
    models::Models,
    run::RunInput,
    storage::{RelationalStore, Scope},
    types::*,
    worker,
};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn catalog_pages_preserve_visibility_ownership_and_rotation() {
    let dir = tempfile::tempdir().unwrap();
    let s = open_service(dir.path(), Models::disabled()).await.unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    let store = s.engine.relational();
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "UI")
        .await
        .unwrap();
    let admin_key = store.issue_key(scope, "admin").await.unwrap();
    let other_key = store.issue_key(scope, "admin").await.unwrap();
    let reader_key = store.issue_key(scope, "reader").await.unwrap();
    let admin = s.auth(&admin_key.token).await.unwrap();
    let other = s.auth(&other_key.token).await.unwrap();
    let reader = s.auth(&reader_key.token).await.unwrap();
    let mut sources = Vec::new();
    for index in 0..3 {
        let receipt = s.identified_memory(&admin, &Uuid::new_v4().to_string(), serde_json::from_value(json!({
            "identity":{"subject":{"kind":"service","stable_id":format!("Atlas-{index}")},"predicate":"release.rule","context":{"env":"production"}},
            "content":"中文 🦀 é evidence", "expected_version":0
        })).unwrap()).await.unwrap();
        sources.push(serde_json::from_value::<Uuid>(receipt["source_event_id"].clone()).unwrap());
    }
    let knowledge = s
        .knowledge(
            &admin,
            "document",
            KnowledgeInput {
                title: "知识文档".into(),
                content: Some("可读取知识".into()),
                file_id: None,
                format: "text".into(),
                asset_id: None,
                expected_version: None,
            },
        )
        .await
        .unwrap();
    while worker::process_next(&s).await.unwrap() {}
    let mut after = Uuid::nil();
    let mut ids = Vec::new();
    loop {
        let page = s.list_assets(&reader, "memory", after, 1).await.unwrap();
        for asset in page["items"].as_array().unwrap() {
            assert!(asset.get("content").is_none());
            assert!(asset.get("semantic_values").is_none());
            assert_eq!(asset["identity"]["context"]["env"], "production");
            ids.push(asset["asset_id"].as_str().unwrap().to_owned());
        }
        if page["next_after"].is_null() {
            break;
        }
        after = serde_json::from_value(page["next_after"].clone()).unwrap();
    }
    assert_eq!(ids.len(), 3);
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 3);
    let docs = s
        .list_assets(&reader, "knowledge", Uuid::nil(), 25)
        .await
        .unwrap();
    assert_eq!(docs["items"][0]["asset_id"], knowledge["asset_id"]);
    s.delete(&admin, "withdraw", sources[0], "event")
        .await
        .unwrap();
    let visible = s
        .list_assets(&reader, "memory", Uuid::nil(), 25)
        .await
        .unwrap();
    assert_eq!(visible["items"].as_array().unwrap().len(), 2);
    let mut cursor = Uuid::nil();
    let mut visible_count = 0;
    loop {
        let page = s.list_assets(&reader, "memory", cursor, 1).await.unwrap();
        visible_count += page["items"].as_array().unwrap().len();
        if page["next_after"].is_null() {
            break;
        }
        let next: Uuid = serde_json::from_value(page["next_after"].clone()).unwrap();
        assert!(next > cursor);
        cursor = next;
    }
    assert_eq!(visible_count, 2);
    let run = Uuid::new_v4();
    s.create_run(
        &admin,
        RunInput {
            run_id: run,
            summary: "private task".into(),
        },
    )
    .await
    .unwrap();
    s.create_run(
        &other,
        RunInput {
            run_id: Uuid::new_v4(),
            summary: "other private task".into(),
        },
    )
    .await
    .unwrap();
    let runs = s.list_runs(&admin, Uuid::nil(), 1).await.unwrap();
    assert_eq!(runs["items"].as_array().unwrap().len(), 1);
    assert_eq!(runs["items"][0]["run_id"], run.to_string());
    assert!(
        s.list_runs(&reader, Uuid::nil(), 25).await.unwrap()["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let rotated = s.rotate_key(&other, admin_key.key_id).await.unwrap();
    let rotated_auth = s.auth(rotated["token"].as_str().unwrap()).await.unwrap();
    assert_eq!(
        s.list_runs(&rotated_auth, Uuid::nil(), 25).await.unwrap()["items"][0]["run_id"],
        run.to_string()
    );
    assert!(s.list_runs(&admin, Uuid::nil(), 25).await.is_err());
    let separate = Scope {
        tenant_id: scope.tenant_id,
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(separate.tenant_id, separate.workspace_id, "other workspace")
        .await
        .unwrap();
    let outsider_key = store.issue_key(separate, "admin").await.unwrap();
    let outsider = s.auth(&outsider_key.token).await.unwrap();
    assert!(
        s.list_assets(&outsider, "memory", Uuid::nil(), 25)
            .await
            .unwrap()["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        s.list_assets(&reader, "memory", Uuid::nil(), 0)
            .await
            .is_err()
    );
    assert!(
        s.list_assets(&reader, "invalid", Uuid::nil(), 10)
            .await
            .is_err()
    );
    assert!(s.list_runs(&other, Uuid::nil(), 101).await.is_err());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = origence::api::router(s.clone());
    let host = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let http = reqwest::Client::new();
    for path in ["/v1/assets?kind=knowledge&limit=1", "/v1/runs?limit=1"] {
        let result = http
            .get(format!("{base}{path}"))
            .bearer_auth(&reader_key.token)
            .send()
            .await
            .unwrap();
        assert!(result.status().is_success());
        assert!(result.json::<Value>().await.unwrap()["items"].is_array());
    }
    assert_eq!(
        http.get(format!("{base}/v1/assets?kind=memory"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    host.abort();
}
