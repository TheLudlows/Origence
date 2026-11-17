//! Real SQLite + Service + HTTP contracts for the UI's bounded catalogs.
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

#[tokio::test]
async fn asset_run_references_are_exact_bounded_private_and_withdrawal_gated() {
    let dir = tempfile::tempdir().unwrap();
    let s = open_service(dir.path(), Models::disabled()).await.unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    let store = s.engine.relational();
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "reverse references")
        .await
        .unwrap();
    let key = store.issue_key(scope, "admin").await.unwrap();
    let other_key = store.issue_key(scope, "admin").await.unwrap();
    let a = s.auth(&key.token).await.unwrap();
    let other = s.auth(&other_key.token).await.unwrap();
    let receipt = s
        .knowledge(
            &a,
            "v1",
            serde_json::from_value(json!({
                "title":"Atlas", "content":"Atlas version one", "format":"text"
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    while worker::process_next(&s).await.unwrap() {}
    let asset: Uuid = serde_json::from_value(receipt["asset_id"].clone()).unwrap();
    let hit = s
        .search(
            &a,
            serde_json::from_value(json!({"query":"Atlas","mode":"keyword"})).unwrap(),
        )
        .await
        .unwrap();
    let hit = &hit["hits"][0];
    let reference = json!({"kind":"chunk","id":hit["chunk_id"],"chunk_id":hit["chunk_id"],"asset_id":asset,"asset_version":1});
    let mut runs = Vec::new();
    for _ in 0..3 {
        let run = Uuid::new_v4();
        s.create_run(
            &a,
            RunInput {
                run_id: run,
                summary: "private body must not escape".into(),
            },
        )
        .await
        .unwrap();
        let event: origence::run::RunEventInput = serde_json::from_value(json!({
            "event_id":Uuid::new_v4(),"reporter":"client","observed_at_ms":1,
            "event":{"kind":"context_reported","references":[reference.clone()]}
        }))
        .unwrap();
        if runs.is_empty() {
            origence::storage::sqlite_db::raw_sql("CREATE TRIGGER reject_run_projection BEFORE INSERT ON oc_run_asset_refs BEGIN SELECT RAISE(ABORT,'injected projection failure'); END;")
                .execute(store.pool()).await.unwrap();
            assert!(s.append_run_event(&a, run, event.clone()).await.is_err());
            let events: i64 = origence::storage::sqlite_db::query_scalar(
                "SELECT count(*) FROM oc_run_events WHERE run_id=?",
            )
            .bind(run)
            .fetch_one(store.pool())
            .await
            .unwrap();
            let refs: i64 = origence::storage::sqlite_db::query_scalar(
                "SELECT count(*) FROM oc_run_asset_refs WHERE run_id=?",
            )
            .bind(run)
            .fetch_one(store.pool())
            .await
            .unwrap();
            assert_eq!((events, refs), (0, 0));
            origence::storage::sqlite_db::raw_sql("DROP TRIGGER reject_run_projection")
                .execute(store.pool())
                .await
                .unwrap();
        }
        s.append_run_event(&a, run, event.clone()).await.unwrap();
        s.append_run_event(&a, run, event).await.unwrap();
        runs.push(run);
    }
    s.knowledge(&a, "v2", serde_json::from_value(json!({
        "title":"Atlas two","content":"Atlas version two","format":"text","asset_id":asset,"expected_version":1
    })).unwrap()).await.unwrap();
    while worker::process_next(&s).await.unwrap() {}
    assert!(
        s.asset_runs(&a, asset, 2, Uuid::nil(), 25).await.unwrap()["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        s.asset_runs(&other, asset, 1, Uuid::nil(), 25)
            .await
            .unwrap()["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let rotated = s.rotate_key(&other, key.key_id).await.unwrap();
    let a = s.auth(rotated["token"].as_str().unwrap()).await.unwrap();
    let mut after = Uuid::nil();
    let mut found = Vec::new();
    loop {
        let page = s.asset_runs(&a, asset, 1, after, 1).await.unwrap();
        assert_eq!(page["coverage"], "recorded_references");
        assert_eq!(page["scope"], "current_logical_key");
        assert!(!page.to_string().contains("private body"));
        assert_eq!(page["items"].as_array().unwrap().len(), 1);
        found.push(serde_json::from_value::<Uuid>(page["items"][0]["run_id"].clone()).unwrap());
        if page["next_after"].is_null() {
            break;
        }
        let next = serde_json::from_value(page["next_after"].clone()).unwrap();
        assert!(next > after);
        after = next;
    }
    runs.sort();
    assert_eq!(found, runs);
    // The lookup projection is not authority: an absent immutable reference
    // must not turn a stale pointer into a visible association.
    let pool = s.engine.relational().pool();
    let saved: String = origence::storage::sqlite_db::query_scalar("SELECT references_json FROM oc_run_events WHERE tenant_id=? AND workspace_id=? AND run_id=?")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(runs[0]).fetch_one(pool).await.unwrap();
    origence::storage::sqlite_db::query("UPDATE oc_run_events SET references_json='[]' WHERE tenant_id=? AND workspace_id=? AND run_id=?")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(runs[0]).execute(pool).await.unwrap();
    let skipped = s.asset_runs(&a, asset, 1, Uuid::nil(), 1).await.unwrap();
    assert!(skipped["items"].as_array().unwrap().is_empty());
    assert_eq!(skipped["next_after"], runs[0].to_string());
    origence::storage::sqlite_db::query("UPDATE oc_run_events SET references_json=? WHERE tenant_id=? AND workspace_id=? AND run_id=?")
        .bind(saved).bind(scope.tenant_id).bind(scope.workspace_id).bind(runs[0]).execute(pool).await.unwrap();

    for (version, limit) in [(0, 25), (1, 0), (1, 101)] {
        assert!(
            s.asset_runs(&a, asset, version, Uuid::nil(), limit)
                .await
                .is_err()
        );
    }
    let separate = Scope {
        tenant_id: scope.tenant_id,
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(separate.tenant_id, separate.workspace_id, "separate")
        .await
        .unwrap();
    let outsider_key = store.issue_key(separate, "admin").await.unwrap();
    let outsider = s.auth(&outsider_key.token).await.unwrap();
    assert!(
        s.asset_runs(&outsider, asset, 1, Uuid::nil(), 25)
            .await
            .is_err()
    );
    s.engine.shutdown().await.unwrap();
    drop(s);
    let s = open_service(dir.path(), Models::disabled()).await.unwrap();
    let a = s.auth(rotated["token"].as_str().unwrap()).await.unwrap();
    assert_eq!(
        s.asset_runs(&a, asset, 1, Uuid::nil(), 25).await.unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = origence::api::router(s.clone());
    let host = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let http = reqwest::Client::new();
    for query in [
        "",
        "?version=0",
        "?version=1&limit=101",
        "?version=1&owner_key_id=any",
    ] {
        assert!(
            http.get(format!("{base}/v1/assets/{asset}/runs{query}"))
                .bearer_auth(rotated["token"].as_str().unwrap())
                .send()
                .await
                .unwrap()
                .status()
                .is_client_error()
        );
    }
    let response = http
        .get(format!("{base}/v1/assets/{asset}/runs?version=1&limit=1"))
        .bearer_auth(rotated["token"].as_str().unwrap())
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    assert_eq!(
        response.json::<Value>().await.unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // Injected fault after the prepared event insert but before its
    // reverse projection: neither half may survive the failed transaction.
    let pool = s.engine.relational().pool();
    origence::storage::sqlite_db::raw_sql("CREATE TRIGGER reject_run_projection BEFORE INSERT ON oc_run_asset_refs BEGIN SELECT RAISE(ABORT,'injected projection failure'); END;").execute(pool).await.unwrap();
    let event_id = Uuid::new_v4();
    let failed = http
        .post(format!("{base}/v1/runs/{}/context", runs[0]))
        .bearer_auth(rotated["token"].as_str().unwrap())
        .json(&json!({"event_id":event_id,"context":{"query":"Atlas","mode":"keyword"}}))
        .send()
        .await
        .unwrap();
    assert!(failed.status().is_server_error());
    let events: i64 = origence::storage::sqlite_db::query_scalar(
        "SELECT count(*) FROM oc_run_events WHERE event_id=?",
    )
    .bind(event_id)
    .fetch_one(pool)
    .await
    .unwrap();
    let refs: i64 = origence::storage::sqlite_db::query_scalar(
        "SELECT count(*) FROM oc_run_asset_refs WHERE event_id=?",
    )
    .bind(event_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!((events, refs), (0, 0));
    origence::storage::sqlite_db::raw_sql("DROP TRIGGER reject_run_projection")
        .execute(pool)
        .await
        .unwrap();
    let prepared = http
        .post(format!("{base}/v1/runs/{}/context", runs[0]))
        .bearer_auth(rotated["token"].as_str().unwrap())
        .json(&json!({"event_id":event_id,"context":{"query":"Atlas","mode":"keyword"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(prepared.status(), 200);
    let linked = s.asset_runs(&a, asset, 2, Uuid::nil(), 25).await.unwrap();
    assert_eq!(linked["items"].as_array().unwrap().len(), 1);
    assert_eq!(linked["items"][0]["run_id"], runs[0].to_string());
    let source: Uuid = serde_json::from_value(receipt["source_event_id"].clone()).unwrap();
    s.delete(&a, "withdraw-v1", source, "event").await.unwrap();
    assert!(s.asset_runs(&a, asset, 1, Uuid::nil(), 25).await.is_err());
    // The successful preparation now links v2; withdrawing v1 leaves it visible.
    assert_eq!(
        s.asset_runs(&a, asset, 2, Uuid::nil(), 25).await.unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    host.abort();
}
