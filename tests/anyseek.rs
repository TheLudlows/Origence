//! Local HTTP protocol tests plus opt-in separately provisioned real AnySeek contracts.
#[cfg(all(feature = "sqlite", feature = "host"))]
use crate::fixture_keys::FixtureKeys;
#[cfg(feature = "host")]
mod support;
use origence::storage::{
    FullTextEntry, FullTextQuery, Scope, SourceVersion,
    anyseek::{AnySeekConfig, AnySeekFullTextStore, operation_epoch},
};
use std::collections::HashMap;
use uuid::Uuid;

fn scope() -> Scope {
    Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    }
}
fn entry(text: &str) -> FullTextEntry {
    FullTextEntry {
        id: Uuid::new_v4(),
        source: SourceVersion {
            source_id: Uuid::new_v4(),
            version: 1,
        },
        generation: 1,
        search_terms: text.into(),
        payload: origence::storage::ChunkPayload {
            content: text.into(),
            locator: serde_json::json!({}),
            summary: None,
        },
    }
}
fn config() -> AnySeekConfig {
    AnySeekConfig::new(
        &std::env::var("OC_ANYSEEK_URL").expect("real AnySeek URL required"),
        &std::env::var("OC_ANYSEEK_API_KEY").expect("real AnySeek service key required"),
    )
    .unwrap()
}
fn query(
    scope: Scope,
    entries: &[FullTextEntry],
    terms: &str,
) -> (FullTextQuery, HashMap<Uuid, SourceVersion>) {
    (
        FullTextQuery {
            scope,
            artifact_ids: Some(entries.iter().map(|e| e.id).collect()),
            generation: 1,
            search_terms: terms.into(),
            limit: 1,
        },
        entries.iter().map(|e| (e.id, e.source)).collect(),
    )
}

#[cfg(all(feature = "host", not(feature = "embedded-anyseek")))]
#[test]
fn cloud_host_requires_remote_configuration_and_rejects_local_mode() {
    let dir = tempfile::tempdir().unwrap();
    let command = || {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_origence"));
        command
            .args(["serve", "--bind", "127.0.0.1:0"])
            .env("OC_DATA_DIR", dir.path())
            .env("OC_ENABLE_MODELS", "false")
            .env_remove("OC_ANYSEEK_URL")
            .env_remove("OC_ANYSEEK_API_KEY")
            .env_remove("OC_FULL_TEXT_BACKEND");
        command
    };
    let missing = command().output().unwrap();
    assert!(!missing.status.success());
    assert!(
        String::from_utf8_lossy(&missing.stderr)
            .contains("AnySeek URL and service key are required")
    );
    let local = command().arg("--local").output().unwrap();
    assert_eq!(local.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&local.stderr).contains("--local"));
    assert_eq!(dir.path().read_dir().unwrap().count(), 0);
}

#[test]
fn connection_rejects_unsafe_origins_and_invalid_service_keys() {
    for base in [
        "http://example.com",
        "https://user:secret@example.com",
        "https://example.com/path",
        "https://example.com/?key=secret",
    ] {
        assert!(AnySeekConfig::new(base, "test").is_err());
    }
    for key in ["", "bad\r\nheader"] {
        assert!(AnySeekConfig::new("http://127.0.0.1:8080", key).is_err());
    }
    assert!(AnySeekConfig::new("https://example.com", "test").is_ok());
}

#[test]
fn transient_dependency_errors_keep_the_safe_503_contract() {
    use axum::response::IntoResponse;
    let error: origence::error::AppError =
        origence::storage::StorageError::TransientUnavailable("raw backend diagnostic".into())
            .into();
    assert_eq!(error.code(), "DEPENDENCY_UNAVAILABLE");
    assert!(!error.safe_message().contains("raw backend"));
    assert_eq!(
        error.into_response().status(),
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
#[ignore = "requires an isolated real AnySeek collection; run tools/anyseek_smoke.py"]
async fn real_anyseek_scoped_and_search_replay_cleanup_and_source_validation() {
    let store = AnySeekFullTextStore::new(config()).unwrap();
    store.check().await.unwrap();
    let scope = scope();
    let mut allowed = entry("鉴权 token policies");
    allowed.payload.content = "原文 <b>literal</b>\r\n中文🙂 café".into();
    allowed.payload.locator = serde_json::json!({"page":1,"byte_start":0,"parser":"fixture-v1"});
    allowed.payload.summary = Some(origence::storage::ChunkSummary {
        text: "完整摘要 <literal>".into(),
        model_revision: "summary-v1".into(),
    });
    let forbidden = entry("鉴权 token token token");
    let epoch = operation_epoch().unwrap();
    let entries = vec![allowed.clone(), forbidden.clone()];
    store.upsert(scope, entries.clone(), epoch).await.unwrap();
    assert_eq!(
        store.read_payloads(scope, &[allowed.id]).await.unwrap()[&allowed.id],
        allowed.payload
    );
    // A verified immutable group can settle an old/unknown publication without
    // regenerating an operation or mutating the group after receipt expiration.
    store
        .upsert(scope, entries.clone(), epoch.saturating_sub(5))
        .await
        .unwrap();
    let (q, sources) = query(scope, std::slice::from_ref(&allowed), "鉴权 token");
    let hits = store.search(q.clone(), &sources).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, allowed.id);
    assert!(
        store
            .search(
                FullTextQuery {
                    search_terms: "鉴权 missing".into(),
                    ..q.clone()
                },
                &sources
            )
            .await
            .unwrap()
            .is_empty()
    );
    let other = Scope {
        workspace_id: Uuid::new_v4(),
        ..scope
    };
    assert!(
        store
            .search(
                FullTextQuery {
                    scope: other,
                    ..q.clone()
                },
                &sources
            )
            .await
            .is_err()
    );
    let wrong: HashMap<_, _> = [(
        allowed.id,
        SourceVersion {
            source_id: Uuid::new_v4(),
            version: 1,
        },
    )]
    .into();
    assert!(store.search(q.clone(), &wrong).await.is_err());
    // Cleanup removes one member while retaining its sibling and its identity.
    store
        .delete_objects(scope, vec![forbidden.id])
        .await
        .unwrap();
    assert_eq!(
        store.search(q.clone(), &sources).await.unwrap()[0].id,
        allowed.id
    );
    let (missing, expected) = query(scope, std::slice::from_ref(&forbidden), "token");
    assert!(store.search(missing, &expected).await.is_err());
    store
        .delete_objects(scope, vec![forbidden.id])
        .await
        .unwrap();
    store.delete_objects(scope, vec![allowed.id]).await.unwrap();
    assert!(store.search(q, &sources).await.is_err());
    // Startup cleanup can clear an unpublished group. Rebuilding its exact
    // saved publication uses the original epoch and a current group CAS.
    store.upsert(scope, entries, epoch).await.unwrap();
    let (q, sources) = query(scope, std::slice::from_ref(&allowed), "token");
    assert_eq!(store.search(q, &sources).await.unwrap()[0].id, allowed.id);
    store
        .delete_objects(scope, vec![allowed.id, forbidden.id])
        .await
        .unwrap();
}

async fn lost_write_proxy() -> (
    String,
    std::sync::Arc<std::sync::Mutex<Vec<Option<u64>>>>,
    tokio::task::JoinHandle<()>,
) {
    use axum::{
        Router,
        body::{Body, to_bytes},
        extract::Request,
        http::Response,
        routing::any,
    };
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };
    let backend = std::env::var("OC_ANYSEEK_URL").unwrap();
    let lost = Arc::new(AtomicBool::new(true));
    let epochs = Arc::new(Mutex::new(Vec::new()));
    let captured = epochs.clone();
    let proxy = Router::new().fallback(any(move |request: Request| {
        let backend = backend.clone();
        let lost = lost.clone();
        let epochs = captured.clone();
        async move {
            let method = request.method().clone();
            let path = request.uri().to_string();
            let headers = request.headers().clone();
            let body = to_bytes(request.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap();
            if path.ends_with("/group/replace") {
                let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
                let epoch = request["operation_id"]["epoch"].as_u64().unwrap();
                let removes_group = request["records"].as_array().unwrap().is_empty();
                epochs
                    .lock()
                    .unwrap()
                    .push((!removes_group).then_some(epoch));
            }
            let response = reqwest::Client::new()
                .request(method, format!("{backend}{path}"))
                .headers(headers)
                .body(body)
                .send()
                .await
                .unwrap();
            let status = response.status();
            let response_headers = response.headers().clone();
            let body = response.bytes().await.unwrap();
            if path.ends_with("/group/replace")
                && status.is_success()
                && lost.swap(false, Ordering::SeqCst)
            {
                return Response::builder()
                    .status(503)
                    .body(Body::from("secret backend diagnostic"))
                    .unwrap();
            }
            let mut response = Response::builder()
                .status(status)
                .body(Body::from(body))
                .unwrap();
            for name in ["x-anyseek-request-id", "x-anyseek-error-code"] {
                if let Some(value) = response_headers.get(name) {
                    response.headers_mut().insert(name, value.clone());
                }
            }
            response
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, proxy).await.unwrap() });
    (base, epochs, server)
}

#[tokio::test]
#[ignore = "requires real AnySeek; verifies a committed write with a lost HTTP response"]
async fn real_anyseek_lost_write_response_is_verified_without_resubmission() {
    let (base, epochs, server) = lost_write_proxy().await;
    let store = AnySeekFullTextStore::new(
        AnySeekConfig::new(&base, &std::env::var("OC_ANYSEEK_API_KEY").unwrap()).unwrap(),
    )
    .unwrap();
    let scope = scope();
    let original = entry("lost response");
    let epoch = operation_epoch().unwrap();
    let error = store
        .upsert(scope, vec![original.clone()], epoch)
        .await
        .unwrap_err();
    assert!(!error.to_string().contains("secret backend"));
    store
        .upsert(scope, vec![original.clone()], epoch.saturating_sub(5))
        .await
        .unwrap();
    assert_eq!(*epochs.lock().unwrap(), vec![Some(epoch)]);
    let (q, expected) = query(scope, std::slice::from_ref(&original), "response");
    assert_eq!(store.search(q, &expected).await.unwrap()[0].id, original.id);
    store
        .delete_objects(scope, vec![original.id])
        .await
        .unwrap();
    server.abort();
}

#[tokio::test]
#[ignore = "requires an isolated real AnySeek collection; run tools/anyseek_smoke.py"]
async fn real_anyseek_rejects_missing_duplicate_and_changed_publication_identities() {
    let store = AnySeekFullTextStore::new(config()).unwrap();
    let scope = scope();
    let epoch = operation_epoch().unwrap();
    let original = entry("hello 世界");
    store
        .upsert(scope, vec![original.clone()], epoch)
        .await
        .unwrap();
    let mut changed = original.clone();
    changed.search_terms = "different".into();
    assert!(store.upsert(scope, vec![changed], epoch).await.is_err());
    let duplicate = FullTextEntry {
        source: SourceVersion {
            source_id: Uuid::new_v4(),
            version: 1,
        },
        ..original.clone()
    };
    store.upsert(scope, vec![duplicate], epoch).await.unwrap();
    let (q, sources) = query(scope, std::slice::from_ref(&original), "hello");
    assert!(store.search(q, &sources).await.is_err());
    assert!(
        store
            .upsert(scope, (0..65).map(|_| entry("oversized")).collect(), epoch)
            .await
            .is_err()
    );
    store
        .delete_objects(scope, vec![original.id])
        .await
        .unwrap();
}

#[cfg(feature = "host")]
mod service_contract {
    use super::*;
    use origence::{models::Models, service::Service, worker};
    use serde_json::{Value, json};

    async fn provision(service: &Service) -> (String, origence::types::AuthContext) {
        let scope = scope();
        service
            .engine
            .relational()
            .create_workspace(scope.tenant_id, scope.workspace_id, "AnySeek contract")
            .await
            .unwrap();
        let key = service
            .engine
            .relational()
            .issue_key(scope, "admin")
            .await
            .unwrap();
        let auth = service.auth(&key.token).await.unwrap();
        (key.token, auth)
    }

    #[tokio::test]
    #[ignore = "requires real AnySeek; rejects silently changing the data directory's lexical backend"]
    async fn anyseek_backend_selection_is_bound_before_serving_data() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!dir.path().join("context.db").exists());

        let remote_dir = tempfile::tempdir().unwrap();
        let remote = Service::open_with_anyseek(remote_dir.path(), Models::disabled(), config())
            .await
            .unwrap();
        remote.engine.shutdown().await.unwrap();
        drop(remote);
        let binding = tokio::fs::read_to_string(remote_dir.path().join("full-text-backend.json"))
            .await
            .unwrap();
        assert!(!binding.contains(&std::env::var("OC_ANYSEEK_API_KEY").unwrap()));
        let changed = AnySeekConfig::new(
            "http://127.0.0.1:1",
            &std::env::var("OC_ANYSEEK_API_KEY").unwrap(),
        )
        .unwrap();
        assert!(
            Service::open_with_anyseek(remote_dir.path(), Models::disabled(), changed)
                .await
                .is_err(),
            "a bound directory must reject changed selection before backend IO"
        );
        let remote = Service::open_with_anyseek(remote_dir.path(), Models::disabled(), config())
            .await
            .unwrap();
        remote.engine.shutdown().await.unwrap();
        drop(remote);
        tokio::fs::write(
            remote_dir.path().join("full-text-backend.json"),
            b"malformed",
        )
        .await
        .unwrap();
        assert!(
            Service::open_with_anyseek(remote_dir.path(), Models::disabled(), config())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    #[ignore = "requires real AnySeek; verifies pending publication recovery after a lost write response"]
    async fn anyseek_pending_publication_reopens_with_its_saved_epoch() {
        let (base, epochs, proxy) = lost_write_proxy().await;
        let config =
            || AnySeekConfig::new(&base, &std::env::var("OC_ANYSEEK_API_KEY").unwrap()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let service = Service::open_with_anyseek(dir.path(), Models::disabled(), config())
            .await
            .unwrap();
        let (token, auth) = provision(&service).await;
        let accepted = service
            .knowledge(
                &auth,
                "lost-publication",
                serde_json::from_value(
                    json!({"title":"recovery","format":"text","content":"token recovery evidence"}),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let job_id = serde_json::from_value(accepted["job_id"].clone()).unwrap();
        assert!(worker::process_next(&service).await.unwrap());
        assert_eq!(
            service.job(&auth, job_id).await.unwrap()["state"],
            "retry_wait"
        );
        let saved: String =
            origence::storage::sqlite_db::query_scalar("SELECT payload FROM oc_jobs WHERE id=?")
                .bind(job_id)
                .fetch_one(service.engine.relational().pool())
                .await
                .unwrap();
        let saved: Value = serde_json::from_str(&saved).unwrap();
        assert!(saved.get("publication").is_none());
        let retry = &saved["lexical_publication"];
        let epoch = retry["epoch"].as_u64().unwrap();
        assert!(
            retry["digest"]
                .as_str()
                .is_some_and(|digest| digest.len() == 64)
        );
        assert_eq!(
            epochs
                .lock()
                .unwrap()
                .iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>(),
            vec![epoch]
        );
        service.engine.shutdown().await.unwrap();
        drop(service);
        let service = Service::open_with_anyseek(dir.path(), Models::disabled(), config())
            .await
            .unwrap();
        let auth = service.auth(&token).await.unwrap();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            worker::process_next(&service).await.unwrap();
            let job = service.job(&auth, job_id).await.unwrap();
            if job["state"] == "completed" {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "publication failed to recover: {job}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let after: String =
            origence::storage::sqlite_db::query_scalar("SELECT payload FROM oc_jobs WHERE id=?")
                .bind(job_id)
                .fetch_one(service.engine.relational().pool())
                .await
                .unwrap();
        let after: Value = serde_json::from_str(&after).unwrap();
        assert!(after.get("publication").is_none());
        assert!(after.get("lexical_publication").is_none());
        let input =
            serde_json::from_value(json!({"query":"token recovery","mode":"keyword"})).unwrap();
        assert!(
            !service.search(&auth, input).await.unwrap()["hits"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        // The first write committed, cleanup removed the unpublished group,
        // and the resumed write reused the original operation epoch.
        assert_eq!(
            *epochs.lock().unwrap(),
            vec![Some(epoch), None, Some(epoch)]
        );
        service.engine.shutdown().await.unwrap();
        proxy.abort();
    }

    #[tokio::test]
    #[ignore = "requires real AnySeek; run tools/anyseek_smoke.py with a built Origence binary"]
    async fn anyseek_public_service_ingest_search_get_delete_and_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let service = Service::open_with_anyseek(dir.path(), Models::disabled(), config())
            .await
            .unwrap();
        let (token, auth) = provision(&service).await;
        let (_, other) = provision(&service).await;
        let ingest = |text: &str| {
            serde_json::from_value(json!({"title":"鉴权规范","content":text,"format":"markdown"}))
                .unwrap()
        };
        let accepted = service
            .knowledge(
                &auth,
                "anyseek-first",
                ingest("# Token 鉴权\n\n接口必须校验 token。"),
            )
            .await
            .unwrap();
        assert!(worker::process_next(&service).await.unwrap());
        let job = service
            .job(
                &auth,
                serde_json::from_value(accepted["job_id"].clone()).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(job["state"], "completed", "{job}");
        let input = |query: &str| serde_json::from_value(json!({"query":query})).unwrap();
        let hits = service.search(&auth, input("token 鉴权")).await.unwrap();
        assert_eq!(hits["hits"].as_array().unwrap().len(), 1);
        assert_eq!(hits["hits"][0]["asset_id"], accepted["asset_id"]);
        assert_eq!(
            hits["hits"][0]["source_event_id"],
            accepted["source_event_id"]
        );
        let id = serde_json::from_value(hits["hits"][0]["asset_id"].clone()).unwrap();
        assert_eq!(
            service.get(&auth, id, None).await.unwrap()["content"],
            "# Token 鉴权\n\n接口必须校验 token。"
        );
        for query in ["token fixtureabsent", "fixtureabsent fixturemissing"] {
            assert_eq!(
                service.search(&auth, input(query)).await.unwrap()["hits"],
                json!([]),
                "query with missing terms: {query}"
            );
        }
        assert_eq!(
            service.search(&other, input("token 鉴权")).await.unwrap()["hits"],
            json!([])
        );
        assert!(service.get(&other, id, None).await.is_err());
        let saved: String =
            origence::storage::sqlite_db::query_scalar("SELECT payload FROM oc_jobs WHERE id=?")
                .bind(serde_json::from_value::<Uuid>(accepted["job_id"].clone()).unwrap())
                .fetch_one(service.engine.relational().pool())
                .await
                .unwrap();
        let payload: Value = serde_json::from_str(&saved).unwrap();
        assert!(payload.get("publication").is_none());
        assert!(payload.get("lexical_publication").is_none());
        service.engine.shutdown().await.unwrap();
        drop(service);
        let service = Service::open_with_anyseek(dir.path(), Models::disabled(), config())
            .await
            .unwrap();
        let auth = service.auth(&token).await.unwrap();
        assert!(
            !service.search(&auth, input("token 鉴权")).await.unwrap()["hits"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        service
            .delete(&auth, "anyseek-delete", id, "asset")
            .await
            .unwrap();
        assert_eq!(
            service.search(&auth, input("token 鉴权")).await.unwrap()["hits"],
            json!([])
        );
        while worker::process_next(&service).await.unwrap() {}
        service.engine.shutdown().await.unwrap();
    }
}

#[cfg(feature = "host")]
#[tokio::test]
async fn combined_projection_replay_vector_scope_cleanup_and_unavailability() {
    use origence::storage::{VectorEntry, VectorQuery};
    let peer = support::AnySeekFixture::start(Some(2));
    peer.bind_profile(Some("test-profile".into()));
    let store = AnySeekFullTextStore::new(
        peer.config()
            .with_embedding("test-profile".into(), 2)
            .unwrap(),
    )
    .unwrap();
    store.check().await.unwrap();
    let scope = scope();
    let mut allowed = entry("鉴权 token policies");
    allowed.payload.content = "原文 <b>literal</b>\r\n中文🙂 café".into();
    allowed.payload.locator = serde_json::json!({"page":1,"byte_start":0,"parser":"fixture-v1"});
    allowed.payload.summary = Some(origence::storage::ChunkSummary {
        text: "完整摘要 <literal>".into(),
        model_revision: "summary-v1".into(),
    });
    let sibling = entry("token unrelated sibling");
    let vector = |e: &FullTextEntry, embedding: Vec<f32>| VectorEntry {
        id: e.id,
        source: e.source,
        generation: e.generation,
        profile: "test-profile".into(),
        dimension: 2,
        embedding,
    };
    let vectors = vec![
        vector(&allowed, vec![0.1, 0.3]),
        vector(&sibling, vec![0.7, 0.2]),
    ];
    let entries = vec![allowed.clone(), sibling.clone()];
    let epoch = operation_epoch().unwrap();
    peer.lose_next_write();
    let error = store
        .upsert_combined(scope, entries.clone(), vectors.clone(), epoch)
        .await
        .unwrap_err();
    assert!(!error.to_string().contains("private test backend"));
    assert_eq!(peer.posts(), 1);
    // The f32 payload's JSON roundtrip must settle the exact original group,
    // including an old operation epoch, rather than submit another replacement.
    store
        .upsert_combined(scope, entries, vectors, epoch.saturating_sub(5))
        .await
        .unwrap();
    assert_eq!(peer.posts(), 1);
    let q = VectorQuery {
        scope,
        artifact_ids: Some(vec![allowed.id]),
        generation: 1,
        profile: "test-profile".into(),
        dimension: 2,
        embedding: vec![0.1, 0.3],
        limit: 1,
    };
    let expected = [(allowed.id, allowed.source)].into();
    let hits = store.search_vector(q.clone(), &expected).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, allowed.id);
    assert!(hits[0].score.is_finite());
    assert!(
        store
            .search_vector(
                VectorQuery {
                    artifact_ids: None,
                    ..q.clone()
                },
                &expected
            )
            .await
            .is_err()
    );
    assert!(
        store
            .search_vector(
                VectorQuery {
                    dimension: 3,
                    ..q.clone()
                },
                &expected
            )
            .await
            .is_err()
    );
    assert!(
        store
            .search_vector(
                VectorQuery {
                    scope: Scope {
                        workspace_id: Uuid::new_v4(),
                        ..scope
                    },
                    ..q.clone()
                },
                &expected
            )
            .await
            .is_err()
    );
    assert!(
        store
            .search_vector(q.clone(), &[(allowed.id, sibling.source)].into())
            .await
            .is_err()
    );
    store.delete_objects(scope, vec![sibling.id]).await.unwrap();
    store.delete_objects(scope, vec![sibling.id]).await.unwrap();
    assert_eq!(
        store.search_vector(q.clone(), &expected).await.unwrap()[0].id,
        allowed.id
    );
    let (lexical, sources) = query(scope, std::slice::from_ref(&allowed), "鉴权 token");
    assert_eq!(
        store.search(lexical, &sources).await.unwrap()[0].id,
        allowed.id
    );
    peer.unavailable(true);
    assert!(
        store.search_vector(q.clone(), &expected).await.is_err(),
        "outage must not look like empty recall"
    );
    peer.unavailable(false);
    store.delete_objects(scope, vec![allowed.id]).await.unwrap();
    assert!(store.search_vector(q, &expected).await.is_err());
}

#[cfg(feature = "host")]
#[tokio::test]
async fn record_count_limit_is_optional_and_present_limits_are_positive() {
    use serde_json::json;

    let fixtures = [
        (json!({"max_candidates":10_000}), true),
        (json!({"max_candidates":10_000,"max_records":10_000}), true),
        (json!({"max_candidates":10_000,"max_records":0}), false),
        (json!({"max_candidates":10_000,"max_records":null}), false),
        (
            json!({"max_candidates":10_000,"max_records":"10000"}),
            false,
        ),
        (json!({"max_candidates":0}), false),
        (json!({"max_records":10_000}), false),
    ];
    let peer = support::AnySeekFixture::start(None);
    for (limits, accepted) in fixtures {
        peer.info_override("limits", limits.clone());
        let result = AnySeekFullTextStore::new(peer.config())
            .unwrap()
            .check_scope(scope(), true)
            .await;
        assert_eq!(result.is_ok(), accepted, "limits={limits}: {result:?}");
    }
}

#[cfg(feature = "host")]
#[tokio::test]
async fn unified_collection_configuration_is_exact_and_credentials_are_not_persisted() {
    let peer = support::AnySeekFixture::start(Some(2));
    assert!(
        AnySeekFullTextStore::new(peer.config())
            .unwrap()
            .check_scope(scope(), true)
            .await
            .is_err()
    );
    let configured = peer.config().with_embedding("model:v1".into(), 2).unwrap();
    assert!(configured.with_embedding("different".into(), 2).is_err());
    assert!(AnySeekFullTextStore::vector_collection_config(0).is_err());
    assert!(AnySeekFullTextStore::vector_collection_config(4097).is_err());
    let dir = tempfile::tempdir().unwrap();
    let _peer = support::fixture(dir.path(), None);
    let service = support::open_service(dir.path(), origence::models::Models::disabled())
        .await
        .unwrap();
    service.engine.shutdown().await.unwrap();
    drop(service);
    let binding = tokio::fs::read_to_string(dir.path().join("full-text-backend.json"))
        .await
        .unwrap();
    assert!(!binding.contains("test-owned-key"));
    let selected: serde_json::Value = serde_json::from_str(&binding).unwrap();
    assert_eq!(
        selected["routing"],
        "tenant-uuid/workspace-ws-simple-uuid-v1"
    );
    assert!(selected.get("tenant").is_none());
    assert!(selected.get("collection").is_none());
    // Endpoint changes remain fenced, without a configured namespace/collection.
    let wrong = AnySeekConfig::new("http://127.0.0.1:1", "test-owned-key").unwrap();
    assert!(
        origence::service::Service::open_with_anyseek(
            dir.path(),
            origence::models::Models::disabled(),
            wrong
        )
        .await
        .is_err()
    );
    assert_eq!(
        tokio::fs::read_to_string(dir.path().join("full-text-backend.json"))
            .await
            .unwrap(),
        binding
    );
    assert!(
        support::open_service(dir.path(), origence::models::Models::disabled())
            .await
            .is_ok()
    );
}

#[tokio::test]
#[ignore = "requires an isolated real vector AnySeek collection; run tools/anyseek_smoke.py"]
async fn real_anyseek_vector_group_replay_and_partial_cleanup() {
    use origence::storage::{VectorEntry, VectorQuery};
    let config = AnySeekConfig::new(
        &std::env::var("OC_ANYSEEK_URL").unwrap(),
        &std::env::var("OC_ANYSEEK_API_KEY").unwrap(),
    )
    .unwrap()
    .with_embedding("real-fixture:v1".into(), 2)
    .unwrap();
    let store = AnySeekFullTextStore::new(config).unwrap();
    store.check().await.unwrap();
    let scope = scope();
    let a = entry("token 鉴权");
    let b = entry("other token");
    let vector = |e: &FullTextEntry, embedding| VectorEntry {
        id: e.id,
        source: e.source,
        generation: 1,
        profile: "real-fixture:v1".into(),
        dimension: 2,
        embedding,
    };
    let vectors = vec![vector(&a, vec![0.1, 0.3]), vector(&b, vec![0.7, 0.2])];
    let epoch = operation_epoch().unwrap();
    store
        .upsert_combined(scope, vec![a.clone(), b.clone()], vectors.clone(), epoch)
        .await
        .unwrap();
    store
        .upsert_combined(
            scope,
            vec![a.clone(), b.clone()],
            vectors,
            epoch.saturating_sub(5),
        )
        .await
        .unwrap();
    let q = VectorQuery {
        scope,
        artifact_ids: Some(vec![a.id]),
        profile: "real-fixture:v1".into(),
        dimension: 2,
        generation: 1,
        embedding: vec![0.1, 0.3],
        limit: 1,
    };
    let sources = [(a.id, a.source)].into();
    assert_eq!(
        store.search_vector(q.clone(), &sources).await.unwrap()[0].id,
        a.id
    );
    store.delete_objects(scope, vec![b.id]).await.unwrap();
    assert_eq!(store.search_vector(q, &sources).await.unwrap()[0].id, a.id);
    store.delete_objects(scope, vec![a.id]).await.unwrap();
}

/// Actual engine StringSet filtering through the production Origence adapter.
/// Run only against a separately provisioned v3 test collection.
#[tokio::test]
#[ignore = "requires an isolated real AnySeek content collection"]
async fn real_anyseek_graph_string_sets_direction_scope_and_replacement() {
    use origence::{
        graph::entity_id,
        storage::anyseek::GraphPublication,
        types::{Entity, GraphExtraction, Relation},
    };
    let store = AnySeekFullTextStore::new(config()).unwrap();
    store.check().await.unwrap();
    let scope = scope();
    let mut row = entry("real graph publication");
    row.generation = 7;
    let mut competitor = row.clone();
    competitor.id = Uuid::new_v4();
    competitor.search_terms = "real real graph publication additional unrelated words".into();
    let entries = vec![row.clone(), competitor.clone()];
    let sources = entries
        .iter()
        .map(|e| (e.id, e.source))
        .collect::<HashMap<_, _>>();
    let rank_query = FullTextQuery {
        scope,
        artifact_ids: Some(entries.iter().map(|e| e.id).collect()),
        generation: 7,
        search_terms: "real".into(),
        limit: 10,
    };
    store
        .upsert(scope, entries.clone(), operation_epoch().unwrap())
        .await
        .unwrap();
    let rank = |hits: Vec<origence::storage::FullTextHit>| {
        hits.into_iter()
            .map(|hit| (hit.id, hit.score.to_bits()))
            .collect::<Vec<_>>()
    };
    let before = rank(store.search(rank_query.clone(), &sources).await.unwrap());
    assert_eq!(before.len(), 2);
    let names = ["甲seed", "乙neighbor", "丙second", "丁incoming"];
    let mut graph = GraphExtraction {
        entities: names
            .iter()
            .map(|name| Entity {
                name: (*name).into(),
                entity_type: "node".into(),
                description: "real contract".into(),
            })
            .collect(),
        relations: [(0, 1), (1, 2), (3, 0)]
            .into_iter()
            .map(|(a, b)| Relation {
                source: names[a].into(),
                predicate: "points_to".into(),
                target: names[b].into(),
            })
            .collect(),
    };
    // More than ten graph edges in one attached payload must survive flattening.
    for index in 0..11 {
        let name = format!("extraendpoint{index}");
        graph.entities.push(Entity {
            name: name.clone(),
            entity_type: "node".into(),
            description: String::new(),
        });
        graph.relations.push(Relation {
            source: names[0].into(),
            predicate: "points_to".into(),
            target: name,
        });
    }
    // Reuse identical Record IDs and text via the same emptied group. A
    // nonempty publication is immutable, even if only metadata would change.
    store
        .delete_objects(scope, vec![row.id, competitor.id])
        .await
        .unwrap();
    store
        .upsert_with_graph(
            scope,
            entries.clone(),
            vec![],
            Some(GraphPublication {
                source: row.source,
                generation: row.generation,
                graph,
            }),
            operation_epoch().unwrap(),
        )
        .await
        .unwrap();
    let attached = rank(store.search(rank_query.clone(), &sources).await.unwrap());
    assert_eq!(
        attached, before,
        "adding graph metadata must preserve exact BM25 IDs, order and score bits"
    );
    let seeds = store
        .graph_seed_candidates(scope, "甲seed absent")
        .await
        .unwrap();
    assert_eq!(
        seeds.len(),
        1,
        "StringSet any-match and exact Unicode substring"
    );
    assert_eq!(seeds[0].id, entity_id(names[0]));
    assert_eq!(seeds[0].source, row.source);
    assert_eq!(seeds[0].generation, 7);
    let edges = store.graph_neighbors(scope, &[seeds[0].id]).await.unwrap();
    assert_eq!(edges.len(), 13);
    assert!(
        edges
            .iter()
            .all(|edge| edge.source == row.source && edge.generation == 7)
    );
    for index in 0..11 {
        assert!(
            edges
                .iter()
                .any(|edge| edge.to == entity_id(&format!("extraendpoint{index}")))
        );
    }
    assert!(
        edges
            .iter()
            .any(|e| e.from == entity_id(names[0]) && e.to == entity_id(names[1]))
    );
    assert!(
        edges
            .iter()
            .any(|e| e.from == entity_id(names[3]) && e.to == entity_id(names[0]))
    );
    assert!(
        !edges.iter().any(|e| e.to == entity_id(names[2])),
        "no second-hop edge"
    );
    let foreign = Scope {
        workspace_id: Uuid::new_v4(),
        ..scope
    };
    assert!(
        store
            .graph_seed_candidates(foreign, "甲seed")
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .graph_neighbors(foreign, &[seeds[0].id])
            .await
            .unwrap()
            .is_empty()
    );
    // Reuse identical Record IDs and text via the same emptied group. A
    // nonempty publication is immutable, even if only metadata would change.
    store
        .delete_objects(scope, vec![row.id, competitor.id])
        .await
        .unwrap();
    store
        .upsert_with_graph(
            scope,
            entries.clone(),
            vec![],
            Some(GraphPublication {
                source: row.source,
                generation: row.generation,
                graph: GraphExtraction {
                    entities: vec![],
                    relations: vec![],
                },
            }),
            operation_epoch().unwrap(),
        )
        .await
        .unwrap();
    assert!(
        store
            .graph_seed_candidates(scope, "甲seed")
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .graph_neighbors(scope, &[seeds[0].id])
            .await
            .unwrap()
            .is_empty()
    );
    let removed = rank(store.search(rank_query, &sources).await.unwrap());
    assert_eq!(
        removed, before,
        "removing graph metadata must preserve exact BM25 ranking"
    );
    store
        .delete_objects(scope, vec![row.id, competitor.id])
        .await
        .unwrap();
}

#[cfg(feature = "host")]
#[tokio::test]
async fn workspace_collections_are_automatic_isolated_and_never_repaired_after_loss() {
    let peer = support::AnySeekFixture::start(None);
    let store = AnySeekFullTextStore::new(peer.config()).unwrap();
    store.check().await.unwrap();
    assert_eq!(
        peer.creations(),
        0,
        "service health must not create a placeholder"
    );
    let a = Scope {
        tenant_id: Uuid::parse_str("12345678-1234-4234-8234-123456789012").unwrap(),
        workspace_id: Uuid::new_v4(),
    };
    let b = Scope {
        workspace_id: Uuid::new_v4(),
        ..a
    };
    let c = Scope {
        tenant_id: Uuid::new_v4(),
        ..a
    };
    peer.lose_next_create();
    let (first, concurrent) = tokio::join!(store.check_scope(a, true), store.check_scope(a, true));
    first.unwrap();
    concurrent.unwrap();
    assert_eq!(
        peer.creations(),
        1,
        "lost response/concurrent initialization must converge"
    );
    // Identical logical artifact/source IDs can coexist in distinct scopes.
    let row = entry("workspace scoped token");
    for target in [a, b, c] {
        let mut scoped_row = row.clone();
        scoped_row.payload.content =
            format!("content for {}/{}", target.tenant_id, target.workspace_id);
        store
            .upsert(target, vec![scoped_row.clone()], operation_epoch().unwrap())
            .await
            .unwrap();
        let payloads = store.read_payloads(target, &[row.id]).await.unwrap();
        assert_eq!(payloads.get(&row.id), Some(&scoped_row.payload));
        let (q, sources) = query(target, std::slice::from_ref(&row), "token");
        assert_eq!(store.search(q, &sources).await.unwrap()[0].id, row.id);
    }
    assert_eq!(peer.creations(), 3);
    for target in [a, b, c] {
        assert!(peer.routes().contains(&(
            target.tenant_id.to_string(),
            format!("ws_{}", target.workspace_id.simple())
        )));
    }
    peer.remove_collection(a);
    assert!(store.check().await.is_err());
    assert!(
        store
            .upsert(a, vec![row], operation_epoch().unwrap())
            .await
            .is_err()
    );
    let reopened = AnySeekFullTextStore::new(peer.config()).unwrap();
    assert!(reopened.check_scope(a, false).await.is_err());
    assert_eq!(
        peer.creations(),
        3,
        "missing committed collection must not be recreated"
    );
}

#[cfg(feature = "sqlite")]
#[path = "support/fixture_keys.rs"]
mod fixture_keys;
