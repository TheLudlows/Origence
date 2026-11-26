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
async fn run_metering_scopes_reports_retries_and_exact_totals() {
    use origence::{client::HostClient, error::AppError, storage::sqlite_db as db};
    use reqwest::Method;
    use std::time::{SystemTime, UNIX_EPOCH};

    let dir = tempfile::tempdir().unwrap();
    let service = open_service(dir.path(), Models::disabled()).await.unwrap();
    let store = service.engine.relational();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    let elsewhere = Scope {
        workspace_id: Uuid::new_v4(),
        ..scope
    };
    for scope in [scope, elsewhere] {
        store
            .create_workspace(scope.tenant_id, scope.workspace_id, "metering")
            .await
            .unwrap();
    }
    let owner = store.issue_key(scope, "writer").await.unwrap();
    let other = store.issue_key(scope, "writer").await.unwrap();
    let admin = store.issue_key(scope, "admin").await.unwrap();
    let outsider = store.issue_key(elsewhere, "admin").await.unwrap();
    let owner_auth = service.auth(&owner.token).await.unwrap();
    let end = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
        - 1000;
    let day = 86_400_000;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = origence::api::router(service.clone());
    let host = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = |token: &str| HostClient::new(&base, token.to_owned()).unwrap();
    let owner_http = client(&owner.token);
    let other_http = client(&other.token);
    let admin_http = client(&admin.token);
    let outsider_http = client(&outsider.token);

    let create = |client: HostClient, scope: Scope, created_at: i64| {
        let store = &store;
        async move {
            let run = Uuid::new_v4();
            client
                .call(
                    Method::POST,
                    "/v1/runs",
                    Some(&json!({"run_id":run,"summary":"private metering test"})),
                    None,
                )
                .await
                .unwrap();
            // Stable event window boundaries in a disposable database.
            db::query(
                "UPDATE oc_runs SET created_at=? WHERE tenant_id=? AND workspace_id=? AND run_id=?",
            )
            .bind(created_at)
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(run)
            .execute(store.pool())
            .await
            .unwrap();
            run
        }
    };
    let report = |mut event: Value| {
        event["kind"] = json!("metering_reported");
        json!({"event_id":Uuid::new_v4(),"reporter":"client","observed_at_ms":end,"event":event})
    };
    let overview = |client: HostClient| async move {
        client
            .call(Method::GET, "/v1/runs/overview", None, None)
            .await
            .unwrap()
    };
    let first = create(owner_http.clone(), scope, end - day).await;
    let events = format!("/v1/runs/{first}/events");
    let zero = report(
        json!({"duration_ms":0,"agent_tokens":0,"estimated_cost":{"currency":"USD","micros":0}}),
    );
    let receipt = owner_http
        .call(Method::POST, &events, Some(&zero), None)
        .await
        .unwrap();
    assert_eq!(
        owner_http
            .call(Method::POST, &events, Some(&zero), None)
            .await
            .unwrap(),
        receipt
    );
    for client in [&other_http, &admin_http, &outsider_http] {
        assert!(matches!(
            client.call(Method::POST, &events, Some(&zero), None).await,
            Err(AppError::NotFound)
        ));
    }
    let rejected = report(json!({"agent_tokens":1,"billed_cost":{"currency":"USD","micros":7}}));
    assert!(matches!(
        owner_http
            .call(Method::POST, &events, Some(&rejected), None)
            .await,
        Err(AppError::Conflict(_))
    ));
    for invalid in [
        json!({}),
        json!({"duration_ms":-1}),
        json!({"agent_tokens":1,"transcript":"forbidden payload"}),
        json!({"estimated_cost":{"currency":"usd","micros":0}}),
        json!({"billed_cost":{"currency":"USD","micros":9_007_199_254_740_992_u64}}),
    ] {
        assert!(
            owner_http
                .call(Method::POST, &events, Some(&report(invalid)), None)
                .await
                .is_err()
        );
    }
    let billed =
        report(json!({"billed_cost":{"currency":"USD","micros":9_007_199_254_740_991_u64}}));
    let second = owner_http
        .call(Method::POST, &events, Some(&billed), None)
        .await
        .unwrap();
    assert_eq!(second["sequence"], 2);
    let trace = owner_http
        .call(Method::GET, &format!("/v1/runs/{first}"), None, None)
        .await
        .unwrap();
    assert_eq!(trace["events"].as_array().unwrap().len(), 2);
    assert_eq!(trace["events"][1]["report"], billed);

    for duration in (10..200).step_by(10) {
        let run = create(owner_http.clone(), scope, end - 1).await;
        owner_http
            .call(
                Method::POST,
                &format!("/v1/runs/{run}/events"),
                Some(&report(json!({"duration_ms":duration}))),
                None,
            )
            .await
            .unwrap();
    }
    let unknown = create(owner_http.clone(), scope, end - 1).await;
    owner_http.call(Method::POST, &format!("/v1/runs/{unknown}/events"), Some(&json!({
        "event_id":Uuid::new_v4(),"reporter":"client","observed_at_ms":end,
        "event":{"kind":"result_reported","outcome":"success","summary":"a result without meters"}
    })), None).await.unwrap();
    for created_at in [end - day - 1, end] {
        let run = create(owner_http.clone(), scope, created_at).await;
        owner_http
            .call(
                Method::POST,
                &format!("/v1/runs/{run}/events"),
                Some(&report(json!({"duration_ms":900,"agent_tokens":100}))),
                None,
            )
            .await
            .unwrap();
    }
    let run = create(other_http.clone(), scope, end - 1).await;
    other_http.call(Method::POST, &format!("/v1/runs/{run}/events"), Some(&report(json!({
        "duration_ms":2000,"agent_tokens":17,"estimated_cost":{"currency":"CNY","micros":1500000},
        "billed_cost":{"currency":"USD","micros":2}
    }))), None).await.unwrap();

    let mut tx = store
        .begin(store.authenticate(&owner.token).await.unwrap())
        .await
        .unwrap();
    let fixed = tx.run_creation_overview_at("24h", end).await.unwrap();
    assert_eq!(fixed["created_runs"], 21);
    assert_eq!(
        fixed["metering"],
        json!({
            "duration_p95_ms":180,"duration_runs":20,"agent_tokens":"0","token_runs":1,
            "costs":[{"basis":"estimated","currency":"USD","micros":"0","runs":1},
                     {"basis":"billed","currency":"USD","micros":"9007199254740991","runs":1}]
        })
    );
    let week = tx.run_creation_overview_at("7d", end).await.unwrap();
    assert_eq!(week["created_runs"], 22);
    assert_eq!(week["metering"]["duration_runs"], 21);
    drop(tx);

    // Use the 7d window for the HTTP and credential checks so no fixtures move.
    let read = |client: HostClient| async move {
        client
            .call(Method::GET, "/v1/runs/overview?window=7d", None, None)
            .await
            .unwrap()
    };
    let workspace = read(admin_http.clone()).await;
    assert_eq!(workspace["scope"], "workspace");
    assert_eq!(workspace["metering"]["agent_tokens"], "217");
    assert_eq!(
        workspace["metering"]["costs"],
        json!([
            {"basis":"estimated","currency":"CNY","micros":"1500000","runs":1},
            {"basis":"estimated","currency":"USD","micros":"0","runs":1},
            {"basis":"billed","currency":"USD","micros":"9007199254740993","runs":2}
        ])
    );
    assert_eq!(overview(other_http).await["metering"]["agent_tokens"], "17");
    assert_eq!(
        overview(outsider_http).await["metering"],
        json!({
            "duration_p95_ms":null,"duration_runs":0,"agent_tokens":null,"token_runs":0,"costs":[]
        })
    );
    let admin_auth = service.auth(&admin.token).await.unwrap();
    let rotated = service.rotate_key(&admin_auth, owner.key_id).await.unwrap();
    assert!(
        owner_http
            .call(Method::GET, "/v1/runs/overview", None, None)
            .await
            .is_err()
    );
    assert_eq!(
        read(client(rotated["token"].as_str().unwrap())).await["metering"]["agent_tokens"],
        "200"
    );
    assert!(service.run_trace(&owner_auth, first, 0, 10).await.is_err());
    host.abort();
    service.engine.shutdown().await.unwrap();
}

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
    let run = Uuid::from_u128(1);
    let other_run = Uuid::from_u128(2);
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
            run_id: other_run,
            summary: "other private task".into(),
        },
    )
    .await
    .unwrap();
    for (kind, outcome) in [
        ("result_reported", "success"),
        ("result_reported", "failure"),
        ("feedback", "success"),
    ] {
        s.append_run_event(
            &other,
            other_run,
            serde_json::from_value(json!({
                "event_id":Uuid::new_v4(),"reporter":"client","observed_at_ms":0,
                "event":{"kind":kind,"outcome":outcome,"summary":"private report"}
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    }
    let runs = s.list_runs(&admin, Uuid::nil(), 1).await.unwrap();
    assert_eq!(runs["scope"], "workspace");
    assert_eq!(runs["items"].as_array().unwrap().len(), 1);
    assert_eq!(runs["items"][0]["run_id"], run.to_string());
    assert_eq!(runs["items"][0]["summary"], "private task");
    assert_eq!(runs["items"][0]["is_owner"], true);
    assert_eq!(runs["items"][0]["status"], "unknown");
    assert_eq!(runs["next_after"], json!(run));
    let next = s.list_runs(&admin, run, 1).await.unwrap();
    assert_eq!(
        next,
        json!({"scope":"workspace","next_after":null,"items":[{
            "run_id":other_run,"owner_key_id":other_key.key_id,"is_owner":false,"summary":null,
            "created_at_ms":next["items"][0]["created_at_ms"],"status":"reported_failure"
        }]})
    );
    assert!(s.run_trace(&admin, other_run, 0, 25).await.is_err());
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
    assert_eq!(
        s.list_runs(&outsider, Uuid::nil(), 25).await.unwrap()["items"],
        json!([])
    );
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
    let admin_list: Value = http
        .get(format!("{base}/v1/runs?after={run}"))
        .bearer_auth(rotated["token"].as_str().unwrap())
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(admin_list, next);
    assert_eq!(
        http.get(format!("{base}/v1/runs/{other_run}"))
            .bearer_auth(rotated["token"].as_str().unwrap())
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
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
    let admin = rotated_auth;
    // Exact server-side filters share the existing authority/visibility checks.
    let staging = s.identified_memory(&admin, "staging", serde_json::from_value(json!({
        "identity":{"subject":{"kind":"service","stable_id":"Atlas-1"},"predicate":"release.rule","context":{"env":"staging"}},
        "content":"staging evidence", "expected_version":0
    })).unwrap()).await.unwrap();
    s.memory(
        &admin,
        "legacy",
        MemoryInput {
            fact_key: "legacy".into(),
            content: "unknown identity".into(),
            publish_if_authorized: true,
        },
    )
    .await
    .unwrap();
    while worker::process_next(&s).await.unwrap() {}
    for (filter, expected) in [
        (json!({"context":{"env":"production"}}), 2),
        (
            json!({"subject":{"kind":"service","stable_id":"Atlas-1"},"predicate":"release.rule"}),
            2,
        ),
        (json!({"context":{"env":"staging"}}), 1),
        (json!({"context":{}}), 0),
        (json!({"identified":false}), 1),
        (json!({"predicate":"absent"}), 0),
        (
            json!({"subject":{"kind":"service","stable_id":"atlas-1"}}),
            0,
        ),
    ] {
        let filter = serde_json::from_value(filter).unwrap();
        let mut cursor = Uuid::nil();
        let mut found = Vec::new();
        loop {
            let page = s.list_memories(&reader, &filter, cursor, 1).await.unwrap();
            for item in page["items"].as_array().unwrap() {
                assert!(item.get("content").is_none());
                assert!(item.get("semantic_values").is_none());
                found.push(item["asset_id"].clone());
            }
            if page["next_after"].is_null() {
                break;
            }
            let next: Uuid = serde_json::from_value(page["next_after"].clone()).unwrap();
            assert!(next > cursor);
            cursor = next;
        }
        assert_eq!(found.len(), expected);
        assert!(
            s.list_memories(&outsider, &filter, Uuid::nil(), 25)
                .await
                .unwrap()["items"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    let filter = json!({"context":{"env":"staging"}}).to_string();
    let filtered = http
        .get(format!("{base}/v1/assets"))
        .query(&[
            ("kind", "memory"),
            ("memory_filter", &filter),
            ("limit", "1"),
        ])
        .bearer_auth(&reader_key.token)
        .send()
        .await
        .unwrap();
    assert_eq!(filtered.status(), 200);
    assert_eq!(
        filtered.json::<Value>().await.unwrap()["items"][0]["asset_id"],
        staging["asset_id"]
    );
    for filter in [
        json!({"identified":false,"predicate":"release.rule"}),
        json!({"context":{"env":1}}),
        json!({"unknown":true}),
    ] {
        let response = http
            .get(format!("{base}/v1/assets"))
            .query(&[("kind", "memory"), ("memory_filter", &filter.to_string())])
            .bearer_auth(&reader_key.token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 422);
    }
    let empty = origence::memory_identity::MemoryCatalogFilter::default();
    assert!(
        s.list_memories(&reader, &empty, Uuid::nil(), 0)
            .await
            .is_err()
    );
    assert!(
        s.list_memories(&reader, &empty, Uuid::nil(), 101)
            .await
            .is_err()
    );
    assert_eq!(
        http.get(format!("{base}/v1/assets?kind=memory&memory_filter=%7B%7D"))
            .bearer_auth(&admin_key.token)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        http.get(format!(
            "{base}/v1/assets?kind=knowledge&memory_filter=%7B%7D"
        ))
        .bearer_auth(&reader_key.token)
        .send()
        .await
        .unwrap()
        .status(),
        422
    );
    // Pending rows also count against the fixed work budget. No worker/model IO
    // is needed to prove empty-page continuation beyond the 100-candidate cap.
    for n in 0..101 {
        s.memory(
            &admin,
            &format!("pending-{n}"),
            MemoryInput {
                fact_key: format!("pending-{n}"),
                content: "pending".into(),
                publish_if_authorized: true,
            },
        )
        .await
        .unwrap();
    }
    let absent = serde_json::from_value(json!({"predicate":"absent"})).unwrap();
    let first = s
        .list_memories(&reader, &absent, Uuid::nil(), 25)
        .await
        .unwrap();
    assert_eq!(first["items"], json!([]));
    assert!(!first["next_after"].is_null());
    let last = s
        .list_memories(
            &reader,
            &absent,
            serde_json::from_value(first["next_after"].clone()).unwrap(),
            25,
        )
        .await
        .unwrap();
    assert_eq!(last["items"], json!([]));
    assert!(last["next_after"].is_null());
    let plan: Vec<(i64,i64,i64,String)> = origence::storage::sqlite_db::query_as(
        "EXPLAIN QUERY PLAN SELECT id,kind,deleted,current_version,identity_json FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND id>? ORDER BY id LIMIT 101"
    ).bind(scope.tenant_id).bind(scope.workspace_id).bind(Uuid::nil()).fetch_all(store.pool()).await.unwrap();
    assert!(
        plan.iter()
            .any(|row| row.3.contains("SEARCH") && row.3.contains("INDEX")),
        "{plan:?}"
    );
    assert!(
        !plan
            .iter()
            .any(|row| row.3.contains("TEMP B-TREE") || row.3.contains("SCAN")),
        "{plan:?}"
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

#[tokio::test]
async fn run_creation_overview_counts_owned_runs_in_half_open_windows() {
    use origence::storage::sqlite_db as db;
    use std::time::{SystemTime, UNIX_EPOCH};

    let dir = tempfile::tempdir().unwrap();
    let service = open_service(dir.path(), Models::disabled()).await.unwrap();
    let store = service.engine.relational();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "trends")
        .await
        .unwrap();
    let owner = store.issue_key(scope, "writer").await.unwrap();
    let admin = store.issue_key(scope, "admin").await.unwrap();
    let reader = store.issue_key(scope, "reader").await.unwrap();
    let owner_auth = service.auth(&owner.token).await.unwrap();
    let admin_auth = service.auth(&admin.token).await.unwrap();
    let mut runs = Vec::new();
    for _ in 0..26 {
        let run = RunInput {
            run_id: Uuid::new_v4(),
            summary: "synthetic task".into(),
        };
        service.create_run(&owner_auth, run.clone()).await.unwrap();
        service.create_run(&owner_auth, run.clone()).await.unwrap();
        runs.push(run.run_id);
    }
    for (tenant, workspace) in [
        (scope.tenant_id, Uuid::new_v4()),
        (Uuid::new_v4(), Uuid::new_v4()),
    ] {
        store
            .create_workspace(tenant, workspace, "other")
            .await
            .unwrap();
        let key = store
            .issue_key(
                Scope {
                    tenant_id: tenant,
                    workspace_id: workspace,
                },
                "admin",
            )
            .await
            .unwrap();
        let auth = service.auth(&key.token).await.unwrap();
        service
            .create_run(
                &auth,
                RunInput {
                    run_id: Uuid::new_v4(),
                    summary: "isolated".into(),
                },
            )
            .await
            .unwrap();
    }

    let end = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
        - 1_000;
    let hour = 3_600_000;
    let day = 24 * hour;
    // Fix the timestamp of existing authoritative records in this disposable database.
    for (index, id) in runs.iter().enumerate() {
        let created_at = match index {
            0 => end - 7 * day - 1,
            1 => end - 7 * day,
            2 => end - day - 1,
            3 => end - day,
            4 => end - day + hour - 1,
            5 => end - day + hour,
            6 => end,
            _ => end - 1,
        };
        db::query(
            "UPDATE oc_runs SET created_at=? WHERE tenant_id=? AND workspace_id=? AND run_id=?",
        )
        .bind(created_at)
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(*id)
        .execute(store.pool())
        .await
        .unwrap();
    }
    let mut tx = store
        .begin(store.authenticate(&owner.token).await.unwrap())
        .await
        .unwrap();
    for (window, count, width, expected_total) in [("24h", 24, hour, 22), ("7d", 7, day, 24)] {
        let value = tx.run_creation_overview_at(window, end).await.unwrap();
        assert_eq!(value["timezone"], "UTC");
        assert_eq!(value["scope"], "current_logical_key");
        assert_eq!(value["start_ms"], end - count * width);
        assert_eq!(value["end_ms"], end);
        assert_eq!(value["created_runs"], expected_total);
        let series = value["series"].as_array().unwrap();
        assert_eq!(series.len(), count as usize);
        for (index, row) in series.iter().enumerate() {
            let start = end - (count - index as i64) * width;
            assert_eq!(row["start_ms"], start);
            assert_eq!(row["end_ms"], start + width);
        }
        let counts = series
            .iter()
            .map(|row| row["created_runs"].as_i64().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(counts.iter().sum::<i64>(), expected_total);
        if window == "24h" {
            assert_eq!(counts[..2], [2, 1]);
            assert_eq!(counts[23], 19);
        } else {
            assert_eq!(counts[0], 1);
            assert_eq!(counts[5], 1);
            assert_eq!(counts[6], 22);
        }
    }
    drop(tx);

    let list = service
        .list_runs(&owner_auth, Uuid::nil(), 25)
        .await
        .unwrap();
    assert_eq!(list["items"].as_array().unwrap().len(), 25);
    assert!(list["next_after"].is_string());
    let plan: Vec<(i64, i64, i64, String)> = db::query_as(
        "EXPLAIN QUERY PLAN SELECT COUNT(*) FROM oc_runs INDEXED BY oc_runs_owner_created_at \
         WHERE tenant_id=? AND workspace_id=? AND owner_key_id=? AND created_at>=? AND created_at<?",
    )
    .bind(scope.tenant_id)
    .bind(scope.workspace_id)
    .bind(owner.key_id)
    .bind(end - day)
    .bind(end)
    .fetch_all(store.pool())
    .await
    .unwrap();
    let [(_, _, _, path)] = plan.as_slice() else {
        panic!("expected a single index search: {plan:?}");
    };
    assert!(path.starts_with("SEARCH oc_runs USING COVERING INDEX oc_runs_owner_created_at"));
    assert!(path.contains(
        "tenant_id=? AND workspace_id=? AND owner_key_id=? AND created_at>? AND created_at<?"
    ));
    println!("Run creation count plan: {path}");
    // Move all fixtures safely inside both rolling windows for the real HTTP check.
    db::query("UPDATE oc_runs SET created_at=? WHERE tenant_id=? AND workspace_id=?")
        .bind(end)
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .execute(store.pool())
        .await
        .unwrap();

    let replacement = service.rotate_key(&admin_auth, owner.key_id).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/runs/overview", listener.local_addr().unwrap());
    let app = origence::api::router(service.clone());
    let host = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let http = reqwest::Client::new();
    for (token, expected, scope) in [
        (
            replacement["token"].as_str().unwrap(),
            26,
            "current_logical_key",
        ),
        (&admin.token, 26, "workspace"),
        (&reader.token, 0, "current_logical_key"),
    ] {
        for (query, buckets) in [("", 24), ("?window=7d", 7)] {
            let value: Value = http
                .get(format!("{endpoint}{query}"))
                .bearer_auth(token)
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .json()
                .await
                .unwrap();
            assert_eq!(value["created_runs"], expected);
            assert_eq!(value["scope"], scope);
            let series = value["series"].as_array().unwrap();
            assert_eq!(series.len(), buckets);
            assert_eq!(
                series
                    .iter()
                    .map(|row| row["created_runs"].as_i64().unwrap())
                    .sum::<i64>(),
                expected
            );
        }
    }
    for (query, token, expected) in [
        ("", None, 401),
        ("", Some(owner.token.as_str()), 401),
        ("?window=30d", Some(admin.token.as_str()), 422),
        ("?tenant_id=forbidden", Some(admin.token.as_str()), 400),
        ("?workspace_id=forbidden", Some(admin.token.as_str()), 400),
        ("?owner_key_id=forbidden", Some(admin.token.as_str()), 400),
    ] {
        let mut request = http.get(format!("{endpoint}{query}"));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        assert_eq!(request.send().await.unwrap().status(), expected, "{query}");
    }
    host.abort();
}
