#![cfg(feature = "local-storage")]
use origence::{
    error::AppError,
    models::Models,
    service::Service,
    storage::{RelationalStore, Scope},
    types::AuthContext,
    worker,
};
use serde_json::{Value, json};
use uuid::Uuid;
async fn provision(s: &Service) -> (AuthContext, String) {
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    s.engine
        .relational()
        .create_workspace(scope.tenant_id, scope.workspace_id, "semantics")
        .await
        .unwrap();
    let key = s
        .engine
        .relational()
        .issue_key(scope, "admin")
        .await
        .unwrap();
    (s.auth(&key.token).await.unwrap(), key.token)
}
fn identity(predicate: &str) -> Value {
    json!({"subject":{"kind":"service","stable_id":"Atlas"},"predicate":predicate,"context":{"env":"production"}})
}
fn input(predicate: &str, value: &str, operation: &str, version: i32) -> Value {
    let quote = format!("证据 🦀 é {value}");
    let content = format!("前言\n{quote}");
    let start = content.find(&quote).unwrap();
    json!({"identity":identity(predicate),"content":content,"operation":operation,"expected_version":version,
    "statements":[{"value":value,"quote":quote,"byte_start":start,"byte_end":start+quote.len()}]})
}
async fn property(s: &Service, a: &AuthContext, predicate: &str, cardinality: &str) {
    s.register_memory_property(
        a,
        &Uuid::new_v4().to_string(),
        serde_json::from_value(
            json!({"predicate":predicate,"cardinality":cardinality,"required_context":["env"]}),
        )
        .unwrap(),
    )
    .await
    .unwrap();
}
async fn submit(s: &Service, a: &AuthContext, input: Value) -> Value {
    s.semantic_memory(
        a,
        &Uuid::new_v4().to_string(),
        serde_json::from_value(input).unwrap(),
    )
    .await
    .unwrap()
}
fn id(v: &Value, key: &str) -> Uuid {
    serde_json::from_value(v[key].clone()).unwrap()
}
async fn finish(s: &Service, a: &AuthContext, r: &Value) {
    // A source withdrawal can have queued cleanup ahead of this publication.
    // Drive the existing serialized worker, checking this job rather than assuming
    // that the next queue item is our target.
    for _ in 0..100 {
        let job = s.job(a, id(r, "job_id")).await.unwrap();
        if job["state"] == "completed" {
            return;
        }
        assert_eq!(job["state"], "pending");
        assert!(worker::process_next(s).await.unwrap());
    }
    panic!("target publication did not complete within the fixture queue bound");
}

async fn lookup(s: &Service, a: &AuthContext, predicate: &str) -> Value {
    s.lookup_memory(a, serde_json::from_value(identity(predicate)).unwrap())
        .await
        .unwrap()
}

/// Shared by explicit writes and capture: every read surface must apply the same
/// evidence rules while derived cleanup is still pending.
pub(super) async fn assert_evidence_matrix(
    s: &Service,
    a: &AuthContext,
    snapshot: &Value,
    query: &str,
    visible: bool,
) {
    let asset = id(snapshot, "asset_id");
    let version = snapshot["version"].as_i64().unwrap() as i32;
    for requested_version in [None, Some(version)] {
        let got = s.get(a, asset, requested_version).await;
        if visible {
            assert_eq!(got.unwrap(), *snapshot);
        } else {
            assert!(matches!(got, Err(AppError::NotFound)));
        }
    }
    let confirmed = !snapshot["identity"].is_null();
    let identity = if confirmed {
        snapshot["identity"].clone()
    } else {
        snapshot["semantic_values"][0]["locator"]["suggested_identity"].clone()
    };
    let mut filters = vec![Value::Null];
    if !identity.is_null() {
        let got = s
            .lookup_memory(a, serde_json::from_value(identity.clone()).unwrap())
            .await;
        if confirmed && visible {
            assert_eq!(got.unwrap(), *snapshot);
        } else if confirmed {
            assert!(matches!(got, Err(AppError::NotFound)));
        } else {
            match got {
                Ok(fact) => assert_ne!(fact["asset_id"], snapshot["asset_id"]),
                Err(AppError::NotFound) => {}
                Err(error) => panic!("unexpected lookup failure: {error}"),
            }
        }
        filters.push(identity);
    }
    for mode in ["keyword", "vector", "hybrid"] {
        for filter in &filters {
            let search = s
                .search(
                    a,
                    serde_json::from_value(json!({
                        "query":query,"mode":mode,"limit":100,"memory_identity":filter
                    }))
                    .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(search["effective_mode"], mode);
            assert_eq!(search["warnings"], json!([]));
            let hits = search["hits"].as_array().unwrap();
            let target_hits: Vec<_> = hits
                .iter()
                .filter(|hit| hit["asset_id"] == snapshot["asset_id"])
                .collect();
            let expected = visible && (confirmed || filter.is_null());
            assert_eq!(!target_hits.is_empty(), expected, "{mode}: {search}");
            if confirmed && !filter.is_null() {
                assert_eq!(hits.len(), target_hits.len());
            }
            let resolved = s
                .resolve(
                    a,
                    serde_json::from_value(json!({
                        "query":query,"mode":mode,"budget_tokens":32000,
                        "memory_identity":filter
                    }))
                    .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resolved["effective_mode"], mode);
            assert_eq!(resolved["warnings"], json!([]));
            let context = resolved["rendered_context"].as_str().unwrap();
            assert_eq!(resolved["count"], json!(context.len()));
            assert!(context.len() <= 32000);
            let sources = resolved["sources"].as_array().unwrap();
            let target_sources: Vec<_> = sources
                .iter()
                .filter(|source| source["asset_id"] == snapshot["asset_id"])
                .collect();
            assert_eq!(target_sources.len(), target_hits.len(), "{resolved}");
            if confirmed && !filter.is_null() {
                assert_eq!(sources.len(), target_sources.len());
            }
            if !expected {
                assert!(!context.contains(&asset.to_string()));
            }
            for hit in target_hits {
                let member = snapshot["semantic_values"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|member| member["value"] == hit["locator"]["semantic_value"])
                    .unwrap();
                assert_eq!(hit["version"], snapshot["version"]);
                assert_eq!(hit["identity"], snapshot["identity"]);
                assert_eq!(hit["normalization_status"], snapshot["normalization_status"]);
                assert_eq!(hit["source_event_id"], member["source_event_id"]);
                assert_eq!(hit["locator"], member["locator"]);
                let source = target_sources
                    .iter()
                    .find(|source| source["chunk_id"] == hit["chunk_id"])
                    .unwrap();
                for key in [
                    "version",
                    "identity",
                    "normalization_status",
                    "source_event_id",
                    "locator",
                ] {
                    assert_eq!(source[key], hit[key], "{mode}: {key}");
                }
                assert!(context.contains(source["citation"].as_str().unwrap()));
                assert!(context.contains(hit["content"].as_str().unwrap()));
            }
        }
    }
}

pub(super) async fn assert_evidence_budget(
    s: &Service,
    a: &AuthContext,
    identity: &Value,
    query: &str,
) {
    for mode in ["keyword", "vector", "hybrid"] {
        let request = json!({
            "query":query,"mode":mode,"budget_tokens":32000,"memory_identity":identity
        });
        let full = s
            .resolve(a, serde_json::from_value(request.clone()).unwrap())
            .await
            .unwrap();
        assert_eq!(full["sources"].as_array().unwrap().len(), 1);
        let bytes = full["rendered_context"].as_str().unwrap().len();
        for budget in [0, bytes - 1, bytes] {
            let mut request = request.clone();
            request["budget_tokens"] = json!(budget);
            let bounded = s
                .resolve(a, serde_json::from_value(request).unwrap())
                .await
                .unwrap();
            let context = bounded["rendered_context"].as_str().unwrap();
            assert_eq!(bounded["count"], json!(context.len()));
            assert!(context.len() <= budget);
            if budget == bytes {
                assert_eq!(bounded["rendered_context"], full["rendered_context"]);
                assert_eq!(bounded["sources"], full["sources"]);
            } else {
                assert!(context.is_empty());
                assert_eq!(bounded["sources"], json!([]));
            }
        }
    }
}

#[tokio::test]
async fn semantic_single_duplicate_update_conflict_and_uncertain_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s).await;
    property(&s, &a, "database.engine", "single").await;
    let first = submit(&s, &a, input("database.engine", "PostgreSQL", "assert", 0)).await;
    finish(&s, &a, &first).await;
    let original = lookup(&s, &a, "database.engine").await;
    let duplicate_input = input("database.engine", "PostgreSQL", "assert", 1);
    let duplicate = submit(&s, &a, duplicate_input.clone()).await;
    assert_eq!(duplicate["state"], "duplicate");
    assert!(duplicate.get("job_id").is_none());
    assert_ne!(duplicate["source_event_id"], first["source_event_id"]);
    assert_eq!(lookup(&s, &a, "database.engine").await, original);
    let conflict = submit(&s, &a, input("database.engine", "MySQL", "assert", 1)).await;
    assert_eq!(conflict["semantic_status"], "conflict");
    finish(&s, &a, &conflict).await;
    assert_eq!(lookup(&s, &a, "database.engine").await["version"], 1);
    for operation in ["negate", "propose", "ambiguous"] {
        let r = submit(&s, &a, input("database.engine", "PostgreSQL", operation, 1)).await;
        finish(&s, &a, &r).await;
        let evidence = s.get(&a, id(&r, "asset_id"), None).await.unwrap();
        assert_eq!(evidence["normalization_status"], "unnormalized");
        assert!(evidence["identity"].is_null());
    }
    let update = submit(&s, &a, input("database.engine", "MySQL", "update", 1)).await;
    finish(&s, &a, &update).await;
    assert_eq!(lookup(&s, &a, "database.engine").await["version"], 2);
    assert!(matches!(
        s.semantic_memory(
            &a,
            "stale",
            serde_json::from_value(duplicate_input).unwrap()
        )
        .await,
        Err(AppError::Conflict(_))
    ));
    let search = s
        .search(
            &a,
            serde_json::from_value(json!({"query":"MySQL"})).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(search["hits"].as_array().unwrap().len(), 2);
    for hit in search["hits"].as_array().unwrap() {
        let event = s
            .source_event(&a, id(hit, "source_event_id"))
            .await
            .unwrap();
        let l = &hit["locator"];
        assert_eq!(
            &event["content"].as_str().unwrap()[l["byte_start"].as_u64().unwrap() as usize
                ..l["byte_end"].as_u64().unwrap() as usize],
            hit["content"].as_str().unwrap()
        );
    }
    let resolve = s
        .resolve(
            &a,
            serde_json::from_value(json!({"query":"PostgreSQL","budget_tokens":2000})).unwrap(),
        )
        .await
        .unwrap();
    assert!(resolve.to_string().contains("unnormalized"));
    assert!(resolve.to_string().contains("proposed"));
}
#[tokio::test]
async fn semantic_set_union_keeps_original_sources_and_rebuilds_after_withdrawal() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s).await;
    property(&s, &a, "service.languages", "multi").await;
    let first = submit(&s, &a, input("service.languages", "Rust", "assert", 0)).await;
    finish(&s, &a, &first).await;
    let second = submit(
        &s,
        &a,
        input("service.languages", "Python", "supplement", 1),
    )
    .await;
    finish(&s, &a, &second).await;
    let v = lookup(&s, &a, "service.languages").await;
    assert_eq!(v["semantic_values"].as_array().unwrap().len(), 2);
    assert_eq!(
        v["semantic_values"][0]["source_event_id"],
        first["source_event_id"]
    );
    let search = s
        .search(&a, serde_json::from_value(json!({"query":"Rust"})).unwrap())
        .await
        .unwrap();
    assert_eq!(
        search["hits"][0]["source_event_id"],
        first["source_event_id"]
    );
    s.delete(&a, "withdraw", id(&first, "source_event_id"), "event")
        .await
        .unwrap();
    assert!(matches!(
        s.lookup_memory(
            &a,
            serde_json::from_value(identity("service.languages")).unwrap()
        )
        .await,
        Err(AppError::NotFound)
    ));
    let search = s
        .search(
            &a,
            serde_json::from_value(json!({"query":"Python"})).unwrap(),
        )
        .await
        .unwrap();
    assert!(search["hits"].as_array().unwrap().is_empty());
    // Drain cleanup through the same serialized worker before rebuilding.
    while worker::process_next(&s).await.unwrap() {}
    let rebuild = submit(
        &s,
        &a,
        input("service.languages", "Python", "supplement", 2),
    )
    .await;
    assert_eq!(rebuild["state"], "accepted");
    finish(&s, &a, &rebuild).await;
    let v = lookup(&s, &a, "service.languages").await;
    assert_eq!(v["version"], 3);
    assert_eq!(v["semantic_values"].as_array().unwrap().len(), 1);
    assert_eq!(
        v["semantic_values"][0]["source_event_id"],
        second["source_event_id"]
    );
}

struct Server(tokio::task::JoinHandle<()>);
impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn serve(router: axum::Router) -> (String, Server) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (url, Server(task))
}
async fn http(base: &str, token: &str, path: &str, key: &str, body: Value) -> (u16, Value) {
    let response = reqwest::Client::new()
        .post(format!("{base}{path}"))
        .bearer_auth(token)
        .header("Idempotency-Key", key)
        .json(&body)
        .send()
        .await
        .unwrap();
    (response.status().as_u16(), response.json().await.unwrap())
}
#[tokio::test]
async fn semantic_http_vector_permissions_isolation_and_model_retry() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let unavailable = Arc::new(AtomicBool::new(true));
    let flag = unavailable.clone();
    let (model, _model) = serve(axum::Router::new().route(
        "/embeddings",
        axum::routing::post(move || {
            let flag = flag.clone();
            async move {
                if flag.load(Ordering::SeqCst) {
                    (
                        axum::http::StatusCode::SERVICE_UNAVAILABLE,
                        axum::Json(json!({})),
                    )
                } else {
                    (
                        axum::http::StatusCode::OK,
                        axum::Json(json!({"data":[{"embedding":[1.0,0.5,0.25]}]})),
                    )
                }
            }
        }),
    ))
    .await;
    let dir = tempfile::tempdir().unwrap();
    let models =
        Models::configured(model, "fixture".into(), Some("fixture".into()), None, 3).unwrap();
    let s = Service::open(dir.path(), models).await.unwrap();
    let (a, token) = provision(&s).await;
    let (other, other_token) = provision(&s).await;
    let reader = s
        .engine
        .relational()
        .issue_key(
            Scope {
                tenant_id: a.tenant_id,
                workspace_id: a.workspace_id,
            },
            "reader",
        )
        .await
        .unwrap();
    let (base, _server) = serve(origence::api::router(s.clone())).await;
    let prop =
        json!({"predicate":"database.engine","cardinality":"single","required_context":["env"]});
    assert_eq!(
        http(
            &base,
            &reader.token,
            "/admin/memory-properties",
            "reader",
            prop.clone()
        )
        .await
        .0,
        403
    );
    assert_eq!(
        http(
            &base,
            &token,
            "/admin/memory-properties",
            "property",
            prop.clone()
        )
        .await
        .0,
        200
    );
    assert_eq!(
        http(
            &base,
            &other_token,
            "/admin/memory-properties",
            "property",
            prop
        )
        .await
        .0,
        200
    );
    let body = input("database.engine", "needle", "assert", 0);
    assert_eq!(
        http(
            &base,
            &reader.token,
            "/v1/memories/semantic",
            "reader",
            body.clone()
        )
        .await
        .0,
        403
    );
    let (code, r) = http(
        &base,
        &token,
        "/v1/memories/semantic",
        "request",
        body.clone(),
    )
    .await;
    assert_eq!(code, 200);
    assert_eq!(
        http(
            &base,
            &token,
            "/v1/memories/semantic",
            "request",
            body.clone()
        )
        .await
        .1,
        r
    );
    let mut changed = body.clone();
    changed["operation"] = json!("update");
    assert_eq!(
        http(&base, &token, "/v1/memories/semantic", "request", changed)
            .await
            .0,
        409
    );
    assert!(worker::process_next(&s).await.unwrap());
    assert_eq!(
        s.job(&a, id(&r, "job_id")).await.unwrap()["state"],
        "failed"
    );
    unavailable.store(false, Ordering::SeqCst);
    assert_eq!(
        http(
            &base,
            &token,
            &format!("/v1/jobs/{}/retry", id(&r, "job_id")),
            "retry",
            json!({})
        )
        .await
        .0,
        200
    );
    finish(&s, &a, &r).await;
    let foreign = http(
        &base,
        &other_token,
        "/v1/memories/semantic",
        "request",
        body,
    )
    .await
    .1;
    finish(&s, &other, &foreign).await;
    // Same tenant, another workspace must also be isolated.
    let workspace = Uuid::new_v4();
    s.engine
        .relational()
        .create_workspace(a.tenant_id, workspace, "other workspace")
        .await
        .unwrap();
    let key = s
        .engine
        .relational()
        .issue_key(
            Scope {
                tenant_id: a.tenant_id,
                workspace_id: workspace,
            },
            "admin",
        )
        .await
        .unwrap();
    assert_eq!(
        http(
            &base,
            &key.token,
            "/v1/memories/lookup",
            "lookup",
            identity("database.engine")
        )
        .await
        .0,
        404
    );
    for (field, value) in [("subject", "Boreal"), ("context", "test")] {
        let mut body = input("database.engine", "needle", "assert", 0);
        if field == "subject" {
            body["identity"]["subject"]["stable_id"] = json!(value);
        } else {
            body["identity"]["context"]["env"] = json!(value);
        }
        let other = http(&base, &token, "/v1/memories/semantic", value, body)
            .await
            .1;
        finish(&s, &a, &other).await;
        assert_ne!(other["asset_id"], r["asset_id"]);
    }
    let proposal = http(
        &base,
        &token,
        "/v1/memories/semantic",
        "proposal",
        input("database.engine", "needle", "propose", 1),
    )
    .await
    .1;
    finish(&s, &a, &proposal).await;
    let published = lookup(&s, &a, "database.engine").await;
    let uncertain = s.get(&a, id(&proposal, "asset_id"), None).await.unwrap();
    assert!(uncertain["identity"].is_null());
    assert_eq!(uncertain["normalization_status"], "unnormalized");
    assert_evidence_matrix(&s, &a, &published, "needle", true).await;
    assert_evidence_matrix(&s, &a, &uncertain, "needle", true).await;
    assert_evidence_budget(&s, &a, &identity("database.engine"), "needle").await;
    let duplicate = submit(&s, &a, input("database.engine", "needle", "assert", 1)).await;
    assert_eq!(duplicate["state"], "duplicate");
    assert_ne!(duplicate["source_event_id"], r["source_event_id"]);
    assert_eq!(lookup(&s, &a, "database.engine").await, published);
    s.delete(
        &a,
        "withdraw-duplicate",
        id(&duplicate, "source_event_id"),
        "event",
    )
    .await
    .unwrap();
    // A duplicate audit source never replaces or adds publication support.
    assert_evidence_matrix(&s, &a, &published, "needle", true).await;
    for mode in ["keyword", "vector", "hybrid"] {
        let (status, result) = http(
            &base,
            &reader.token,
            "/v1/search",
            "search",
            json!({"query":"needle","mode":mode,"memory_identity":identity("database.engine")}),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(result["hits"].as_array().unwrap().len(), 1);
        assert_eq!(result["hits"][0]["asset_id"], r["asset_id"]);
        assert_ne!(result["hits"][0]["asset_id"], foreign["asset_id"]);
        let hit = &result["hits"][0];
        let source = s
            .source_event(&a, id(hit, "source_event_id"))
            .await
            .unwrap();
        let l = &hit["locator"];
        assert_eq!(
            &source["content"].as_str().unwrap()[l["byte_start"].as_u64().unwrap() as usize
                ..l["byte_end"].as_u64().unwrap() as usize],
            hit["content"].as_str().unwrap()
        );
    }
    let (status, resolved) = http(
        &base,
        &reader.token,
        "/v1/resolve",
        "resolve",
        json!({"query":"needle","mode":"vector","budget_tokens":2000}),
    )
    .await;
    assert_eq!(status, 200);
    assert!(resolved.to_string().contains("proposed"));
    let mut malformed = input("database.engine", "needle", "update", 1);
    malformed["statements"][0]["byte_start"] = json!(1);
    let (status, error) = http(&base, &token, "/v1/memories/semantic", "badspan", malformed).await;
    assert_eq!(status, 422);
    assert!(!error.to_string().contains("needle"));
    let withdrawal = s
        .delete(&a, "withdraw", id(&r, "source_event_id"), "event")
        .await
        .unwrap();
    assert_eq!(
        s.job(&a, id(&withdrawal, "cleanup_job_id")).await.unwrap()["state"],
        "pending"
    );
    assert_evidence_matrix(&s, &a, &published, "needle", false).await;
    // Withdrawing the confirmed fact must not hide independent proposal evidence.
    assert_evidence_matrix(&s, &a, &uncertain, "needle", true).await;
    let result = http(
        &base,
        &reader.token,
        "/v1/search",
        "search",
        json!({"query":"needle","mode":"vector","memory_identity":identity("database.engine")}),
    )
    .await
    .1;
    assert!(result["hits"].as_array().unwrap().is_empty());
    assert_eq!(http(&base,&token,"/admin/memory-properties","multi-property",json!({"predicate":"service.languages","cardinality":"multi","required_context":["env"]})).await.0,200);
    let first = http(
        &base,
        &token,
        "/v1/memories/semantic",
        "multi-first",
        input("service.languages", "Rust", "assert", 0),
    )
    .await
    .1;
    finish(&s, &a, &first).await;
    let second = http(
        &base,
        &token,
        "/v1/memories/semantic",
        "multi-second",
        input("service.languages", "Python", "supplement", 1),
    )
    .await
    .1;
    finish(&s, &a, &second).await;
    let query =
        json!({"query":"needle","mode":"vector","memory_identity":identity("service.languages")});
    let before = http(
        &base,
        &reader.token,
        "/v1/search",
        "multi-search",
        query.clone(),
    )
    .await
    .1;
    assert_eq!(before["hits"].as_array().unwrap().len(), 2);
    let retained = before["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["locator"]["semantic_value"] == "Rust")
        .unwrap();
    assert_eq!(retained["source_event_id"], first["source_event_id"]);
    assert_eq!(retained["version"], 2);
    assert_eq!(retained["locator"]["source_version"], 1);
    let set = lookup(&s, &a, "service.languages").await;
    assert_evidence_matrix(&s, &a, &set, "Python", true).await;
    let withdrawal = s
        .delete(
            &a,
            "withdraw-member",
            id(&first, "source_event_id"),
            "event",
        )
        .await
        .unwrap();
    assert_eq!(
        s.job(&a, id(&withdrawal, "cleanup_job_id")).await.unwrap()["state"],
        "pending"
    );
    assert_evidence_matrix(&s, &a, &set, "Python", false).await;
    let hidden = http(
        &base,
        &reader.token,
        "/v1/search",
        "multi-search",
        query.clone(),
    )
    .await
    .1;
    assert!(hidden["hits"].as_array().unwrap().is_empty());
    while worker::process_next(&s).await.unwrap() {}
    let rebuild = http(
        &base,
        &token,
        "/v1/memories/semantic",
        "multi-rebuild",
        input("service.languages", "Python", "supplement", 2),
    )
    .await
    .1;
    finish(&s, &a, &rebuild).await;
    let after = http(&base, &reader.token, "/v1/search", "multi-search", query)
        .await
        .1;
    assert_eq!(after["hits"].as_array().unwrap().len(), 1);
    assert_eq!(
        after["hits"][0]["source_event_id"],
        second["source_event_id"]
    );
    let rebuilt = lookup(&s, &a, "service.languages").await;
    assert_evidence_matrix(&s, &a, &rebuilt, "Python", true).await;
}

#[tokio::test]
async fn semantic_concurrent_versions_recover_claim_and_retry_cancelled_job() {
    use origence::storage::JobQueue;
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, token) = provision(&s).await;
    property(&s, &a, "database.engine", "single").await;
    let x = input("database.engine", "Rust", "assert", 0);
    let y = input("database.engine", "Go", "assert", 0);
    let (x, y) = tokio::join!(
        s.semantic_memory(&a, "race-x", serde_json::from_value(x).unwrap()),
        s.semantic_memory(&a, "race-y", serde_json::from_value(y).unwrap())
    );
    let (x, y) = (x.unwrap(), y.unwrap());
    assert_eq!(x["asset_id"], y["asset_id"]);
    // Simulate restart after claim, before publication; recovery uses the saved payload.
    let claim = s.engine.queue().claim_next().await.unwrap().unwrap();
    assert_eq!(claim.kind, "semantic_publish");
    s.engine.relational().pool().close().await;
    drop(s);
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let a = s.auth(&token).await.unwrap();
    assert!(worker::process_next(&s).await.unwrap());
    assert!(worker::process_next(&s).await.unwrap());
    let mut states = vec![
        s.job(&a, id(&x, "job_id")).await.unwrap()["state"]
            .as_str()
            .unwrap()
            .to_string(),
        s.job(&a, id(&y, "job_id")).await.unwrap()["state"]
            .as_str()
            .unwrap()
            .to_string(),
    ];
    states.sort();
    assert_eq!(states, ["completed", "superseded"]);
    assert_eq!(lookup(&s, &a, "database.engine").await["version"], 1);
    let update = submit(&s, &a, input("database.engine", "Python", "update", 1)).await;
    s.job_action(&a, "cancel", id(&update, "job_id"), "cancel")
        .await
        .unwrap();
    s.job_action(&a, "retry", id(&update, "job_id"), "retry")
        .await
        .unwrap();
    finish(&s, &a, &update).await;
    assert_eq!(lookup(&s, &a, "database.engine").await["version"], 2);
    let replay = s
        .semantic_memory(
            &a,
            "race-x",
            serde_json::from_value(input("database.engine", "Rust", "assert", 0)).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replay, x);
}

#[tokio::test]
async fn semantic_guards_limits_legacy_bypass_and_conflicting_batch() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s).await;
    let request = input("database.engine", "Rust", "assert", 0);
    assert!(matches!(
        s.semantic_memory(
            &a,
            "unknown",
            serde_json::from_value(request.clone()).unwrap()
        )
        .await,
        Err(AppError::Invalid(_))
    ));
    property(&s, &a, "database.engine", "single").await;
    let mut no_context = request.clone();
    no_context["identity"]["context"] = json!({});
    assert!(matches!(
        s.semantic_memory(
            &a,
            "no-context",
            serde_json::from_value(no_context).unwrap()
        )
        .await,
        Err(AppError::Invalid(_))
    ));
    let legacy =
        json!({"identity":identity("database.engine"),"content":"bypass","expected_version":0});
    assert!(matches!(
        s.identified_memory(
            &a,
            "bypass",
            serde_json::from_value(legacy.clone()).unwrap()
        )
        .await,
        Err(AppError::Conflict(_))
    ));
    // Configure extraction so the capture guard is reached without model calls.
    let mut enabled = s.clone();
    enabled.models = Models::configured(
        "http://127.0.0.1:1".into(),
        "fixture".into(),
        None,
        Some("fixture".into()),
        3,
    )
    .unwrap();
    assert!(matches!(
        enabled
            .identified_capture(
                &a,
                "capture-bypass",
                serde_json::from_value(legacy).unwrap()
            )
            .await,
        Err(AppError::Conflict(_))
    ));
    let first = submit(&s, &a, request).await;
    finish(&s, &a, &first).await;
    let mut conflict = input("database.engine", "Yes No", "update", 1);
    conflict["statements"] = json!([{"value":"yes","quote":"Yes","byte_start":conflict["content"].as_str().unwrap().find("Yes").unwrap(),"byte_end":conflict["content"].as_str().unwrap().find("Yes").unwrap()+3},{"value":"no","quote":"No","byte_start":conflict["content"].as_str().unwrap().find("No").unwrap(),"byte_end":conflict["content"].as_str().unwrap().find("No").unwrap()+2}]);
    let r = submit(&s, &a, conflict).await;
    assert_eq!(r["semantic_status"], "conflict");
    finish(&s, &a, &r).await;
    assert_eq!(lookup(&s, &a, "database.engine").await["version"], 1);
    assert!(matches!(
        s.restore(
            &a,
            "restore",
            id(&first, "asset_id"),
            serde_json::from_value(
                json!({"target_version":1,"expected_version":1,"reason":"restore"})
            )
            .unwrap()
        )
        .await,
        Err(AppError::Conflict(_))
    ));
    let mut unknown = input("database.engine", "Unknown", "assert", 0);
    unknown["identity"] = Value::Null;
    let unknown = submit(&s, &a, unknown).await;
    finish(&s, &a, &unknown).await;
    assert_eq!(
        s.get(&a, id(&unknown, "asset_id"), None).await.unwrap()["normalization_status"],
        "unnormalized"
    );
    let legacy = s
        .memory(
            &a,
            "legacy",
            serde_json::from_value(json!({"fact_key":"database.engine","content":"legacy"}))
                .unwrap(),
        )
        .await
        .unwrap();
    finish(&s, &a, &legacy).await;
    assert_ne!(legacy["asset_id"], first["asset_id"]);
    property(&s, &a, "service.languages", "multi").await;
    let values: Vec<String> = (0..32).map(|i| format!("value{i}")).collect();
    let content = values.join(" ");
    let statements: Vec<Value> = values
        .iter()
        .map(|v| {
            let start = content.find(v).unwrap();
            json!({"value":v,"quote":v,"byte_start":start,"byte_end":start+v.len()})
        })
        .collect();
    let full=submit(&s,&a,json!({"identity":identity("service.languages"),"content":content,"operation":"assert","expected_version":0,"statements":statements})).await;
    finish(&s, &a, &full).await;
    assert!(matches!(
        s.semantic_memory(
            &a,
            "overflow",
            serde_json::from_value(input("service.languages", "extra", "supplement", 1)).unwrap()
        )
        .await,
        Err(AppError::Invalid(_))
    ));
    assert_eq!(
        lookup(&s, &a, "service.languages").await["semantic_values"]
            .as_array()
            .unwrap()
            .len(),
        32
    );
}

#[tokio::test]
async fn semantic_source_withdrawal_during_pending_union_cannot_publish() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s).await;
    property(&s, &a, "service.languages", "multi").await;
    let first = submit(&s, &a, input("service.languages", "Rust", "assert", 0)).await;
    finish(&s, &a, &first).await;
    let pending = submit(
        &s,
        &a,
        input("service.languages", "Python", "supplement", 1),
    )
    .await;
    s.delete(&a, "withdraw", id(&first, "source_event_id"), "event")
        .await
        .unwrap();
    while worker::process_next(&s).await.unwrap() {}
    assert_eq!(
        s.job(&a, id(&pending, "job_id")).await.unwrap()["state"],
        "superseded"
    );
    assert!(matches!(
        s.lookup_memory(
            &a,
            serde_json::from_value(identity("service.languages")).unwrap()
        )
        .await,
        Err(AppError::NotFound)
    ));
}

