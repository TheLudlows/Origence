//! Opt-in contract tests against a real, separately provisioned AnySeek server.
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
    }
}
fn config() -> AnySeekConfig {
    AnySeekConfig::new(
        &std::env::var("OC_ANYSEEK_URL").expect("real AnySeek URL required"),
        &std::env::var("OC_ANYSEEK_API_KEY").expect("real AnySeek tenant key required"),
        &std::env::var("OC_ANYSEEK_COLLECTION")
            .expect("dedicated AnySeek test collection required"),
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

#[test]
fn connection_rejects_unsafe_origins_and_invalid_identifiers() {
    for base in [
        "http://example.com",
        "https://user:secret@example.com",
        "https://example.com/path",
        "https://example.com/?key=secret",
    ] {
        assert!(AnySeekConfig::new(base, "test", "origence").is_err());
    }
    for collection in ["", "../other", "Invalid", "a/b"] {
        assert!(AnySeekConfig::new("http://127.0.0.1:8080", "test", collection).is_err());
    }
    assert!(AnySeekConfig::new("https://example.com", "test", "origence-lexical").is_ok());
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
    let allowed = entry("鉴权 token policies");
    let forbidden = entry("鉴权 token token token");
    let epoch = operation_epoch().unwrap();
    let entries = vec![allowed.clone(), forbidden.clone()];
    store.upsert(scope, entries.clone(), epoch).await.unwrap();
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
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
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
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    let backend = std::env::var("OC_ANYSEEK_URL").unwrap();
    let lost = Arc::new(AtomicBool::new(true));
    let posts = Arc::new(AtomicUsize::new(0));
    let counted = posts.clone();
    let proxy = Router::new().fallback(any(move |request: Request| {
        let backend = backend.clone();
        let lost = lost.clone();
        let posts = counted.clone();
        async move {
            let method = request.method().clone();
            let path = request.uri().to_string();
            let headers = request.headers().clone();
            let body = to_bytes(request.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap();
            let response = reqwest::Client::new()
                .request(method, format!("{backend}{path}"))
                .headers(headers)
                .body(body)
                .send()
                .await
                .unwrap();
            let status = response.status();
            let body = response.bytes().await.unwrap();
            if path.ends_with("/group/replace") {
                posts.fetch_add(1, Ordering::SeqCst);
                if status.is_success() && lost.swap(false, Ordering::SeqCst) {
                    return Response::builder()
                        .status(503)
                        .body(Body::from("secret backend diagnostic"))
                        .unwrap();
                }
            }
            Response::builder()
                .status(status)
                .body(Body::from(body))
                .unwrap()
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, proxy).await.unwrap() });
    (base, posts, server)
}

#[tokio::test]
#[ignore = "requires real AnySeek; verifies a committed write with a lost HTTP response"]
async fn real_anyseek_lost_write_response_is_verified_without_resubmission() {
    use std::sync::atomic::Ordering;
    let (base, posts, server) = lost_write_proxy().await;
    let store = AnySeekFullTextStore::new(
        AnySeekConfig::new(
            &base,
            &std::env::var("OC_ANYSEEK_API_KEY").unwrap(),
            &std::env::var("OC_ANYSEEK_COLLECTION").unwrap(),
        )
        .unwrap(),
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
    assert_eq!(posts.load(Ordering::SeqCst), 1);
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

#[cfg(feature = "local-storage")]
mod service_contract {
    use super::*;
    use origence::{models::Models, service::Service, storage::RelationalStore, worker};
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
        let local = Service::open(dir.path(), Models::disabled()).await.unwrap();
        let (token, auth) = provision(&local).await;
        local
            .knowledge(
                &auth,
                "binding-first",
                serde_json::from_value(
                    json!({"title":"binding","format":"text","content":"pinned backend evidence"}),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert!(worker::process_next(&local).await.unwrap());
        local.engine.shutdown().await.unwrap();
        drop(local);
        let rejected = match Service::open_with_anyseek(
            dir.path(),
            Models::disabled(),
            Some(config()),
        )
        .await
        {
            Ok(service) => {
                service.engine.shutdown().await.unwrap();
                false
            }
            Err(error) => {
                assert_eq!(error.code(), "VERSION_OR_IDEMPOTENCY_CONFLICT");
                true
            }
        };
        assert!(
            rejected,
            "published LanceDB data must not silently select a different lexical backend"
        );
        let local = Service::open(dir.path(), Models::disabled()).await.unwrap();
        let auth = local.auth(&token).await.unwrap();
        let input =
            serde_json::from_value(json!({"query":"pinned backend","mode":"keyword"})).unwrap();
        assert!(
            !local.search(&auth, input).await.unwrap()["hits"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        local.engine.shutdown().await.unwrap();
        drop(local);

        let remote_dir = tempfile::tempdir().unwrap();
        let remote =
            Service::open_with_anyseek(remote_dir.path(), Models::disabled(), Some(config()))
                .await
                .unwrap();
        remote.engine.shutdown().await.unwrap();
        drop(remote);
        let binding = tokio::fs::read_to_string(remote_dir.path().join("full-text-backend.json"))
            .await
            .unwrap();
        assert!(!binding.contains(&std::env::var("OC_ANYSEEK_API_KEY").unwrap()));
        for selection in [
            None,
            Some(
                AnySeekConfig::new(
                    &std::env::var("OC_ANYSEEK_URL").unwrap(),
                    &std::env::var("OC_ANYSEEK_API_KEY").unwrap(),
                    "different-collection",
                )
                .unwrap(),
            ),
        ] {
            let rejected =
                match Service::open_with_anyseek(remote_dir.path(), Models::disabled(), selection)
                    .await
                {
                    Ok(service) => {
                        service.engine.shutdown().await.unwrap();
                        false
                    }
                    Err(error) => {
                        assert_eq!(error.code(), "VERSION_OR_IDEMPOTENCY_CONFLICT");
                        true
                    }
                };
            assert!(
                rejected,
                "a bound AnySeek directory must reject changed selection before backend IO"
            );
        }
        let remote =
            Service::open_with_anyseek(remote_dir.path(), Models::disabled(), Some(config()))
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
            Service::open_with_anyseek(remote_dir.path(), Models::disabled(), Some(config()))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    #[ignore = "requires real AnySeek; verifies pending publication recovery after a lost write response"]
    async fn anyseek_pending_publication_reopens_with_its_saved_epoch() {
        let (base, posts, proxy) = lost_write_proxy().await;
        let config = || {
            AnySeekConfig::new(
                &base,
                &std::env::var("OC_ANYSEEK_API_KEY").unwrap(),
                &std::env::var("OC_ANYSEEK_COLLECTION").unwrap(),
            )
            .unwrap()
        };
        let dir = tempfile::tempdir().unwrap();
        let service = Service::open_with_anyseek(dir.path(), Models::disabled(), Some(config()))
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
            origence::storage::native_turso::query_scalar("SELECT payload FROM oc_jobs WHERE id=?")
                .bind(job_id)
                .fetch_one(service.engine.relational().pool())
                .await
                .unwrap();
        let saved: Value = serde_json::from_str(&saved).unwrap();
        let epoch = saved["publication"]["full_text_operation_epoch"].clone();
        assert!(epoch.is_u64());
        service.engine.shutdown().await.unwrap();
        drop(service);
        let service = Service::open_with_anyseek(dir.path(), Models::disabled(), Some(config()))
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
            origence::storage::native_turso::query_scalar("SELECT payload FROM oc_jobs WHERE id=?")
                .bind(job_id)
                .fetch_one(service.engine.relational().pool())
                .await
                .unwrap();
        let after: Value = serde_json::from_str(&after).unwrap();
        assert_eq!(after["publication"]["full_text_operation_epoch"], epoch);
        let input =
            serde_json::from_value(json!({"query":"token recovery","mode":"keyword"})).unwrap();
        assert!(
            !service.search(&auth, input).await.unwrap()["hits"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(posts.load(std::sync::atomic::Ordering::SeqCst), 3);
        service.engine.shutdown().await.unwrap();
        proxy.abort();
    }

    #[tokio::test]
    #[ignore = "requires real AnySeek; run tools/anyseek_smoke.py with a built Origence binary"]
    async fn anyseek_public_service_ingest_search_get_delete_and_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let service = Service::open_with_anyseek(dir.path(), Models::disabled(), Some(config()))
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
        let input =
            || serde_json::from_value(json!({"query":"token 鉴权","mode":"keyword"})).unwrap();
        let hits = service.search(&auth, input()).await.unwrap();
        assert!(!hits["hits"].as_array().unwrap().is_empty());
        let id = serde_json::from_value(hits["hits"][0]["asset_id"].clone()).unwrap();
        assert_eq!(
            service.get(&auth, id, None).await.unwrap()["content"],
            "# Token 鉴权\n\n接口必须校验 token。"
        );
        assert_eq!(
            service.search(&other, input()).await.unwrap()["hits"],
            json!([])
        );
        assert!(service.get(&other, id, None).await.is_err());
        let saved: String =
            origence::storage::native_turso::query_scalar("SELECT payload FROM oc_jobs WHERE id=?")
                .bind(serde_json::from_value::<Uuid>(accepted["job_id"].clone()).unwrap())
                .fetch_one(service.engine.relational().pool())
                .await
                .unwrap();
        let plan: Value = serde_json::from_str(&saved).unwrap();
        assert!(
            plan["publication"]["full_text_operation_epoch"]
                .as_u64()
                .is_some()
        );
        service.engine.shutdown().await.unwrap();
        drop(service);
        let service = Service::open_with_anyseek(dir.path(), Models::disabled(), Some(config()))
            .await
            .unwrap();
        let auth = service.auth(&token).await.unwrap();
        assert!(
            !service.search(&auth, input()).await.unwrap()["hits"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        service
            .delete(&auth, "anyseek-delete", id, "asset")
            .await
            .unwrap();
        assert_eq!(
            service.search(&auth, input()).await.unwrap()["hits"],
            json!([])
        );
        while worker::process_next(&service).await.unwrap() {}
        service.engine.shutdown().await.unwrap();
    }
}
