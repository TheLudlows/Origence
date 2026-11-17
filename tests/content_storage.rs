//! Stored bytes and retrieval indexes share one AnySeek chunk Record.
use origence::{
    error::AppError,
    models::Models,
    service::Service,
    storage::{RelationalStore, Scope, sqlite_db as db},
    types::{AuthContext, MemoryInput, RestoreInput},
    worker,
};
use serde_json::{Value, json};
use uuid::Uuid;

async fn setup() -> (tempfile::TempDir, Service, AuthContext, Value) {
    let dir = tempfile::tempdir().unwrap();
    let service = crate::support::open_service(dir.path(), Models::disabled())
        .await
        .unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    service
        .engine
        .relational()
        .create_workspace(scope.tenant_id, scope.workspace_id, "content")
        .await
        .unwrap();
    let key = service
        .engine
        .relational()
        .issue_key(scope, "admin")
        .await
        .unwrap();
    let auth = service.auth(&key.token).await.unwrap();
    let receipt = publish(&service, &auth, "v1", "Atlas 正文🙂\r\n第二行 <literal>").await;
    (dir, service, auth, receipt)
}

async fn publish(service: &Service, auth: &AuthContext, key: &str, text: &str) -> Value {
    let receipt = service
        .memory(
            auth,
            key,
            MemoryInput {
                fact_key: "policy".into(),
                content: text.into(),
                publish_if_authorized: true,
            },
        )
        .await
        .unwrap();
    assert!(worker::process_next(service).await.unwrap());
    receipt
}

fn id(value: &Value, field: &str) -> Uuid {
    serde_json::from_value(value[field].clone()).unwrap()
}

#[tokio::test]
async fn stored_content_shares_index_records_and_restores_history_after_reopen() {
    let (dir, service, auth, first) = setup().await;
    let fixture = crate::support::fixture(dir.path(), None);
    let records = fixture.snapshot_records();
    assert_eq!(records.len(), 1);
    assert!(
        records[0]["document"]["terms"]
            .as_str()
            .is_some_and(|terms| !terms.is_empty())
    );
    let original = records[0]["document"]["payload"]["content"]
        .as_str()
        .unwrap();
    assert_eq!(original, "Atlas 正文🙂\r\n第二行 <literal>");
    for (table, removed) in [
        ("oc_chunks", "content"),
        ("oc_versions", "content"),
        ("oc_summaries", "text"),
        ("oc_chunks", "search_terms"),
        ("oc_summaries", "model_revision"),
    ] {
        let rows = db::query(&format!("PRAGMA table_info({table})"))
            .fetch_all(service.engine.relational().pool())
            .await
            .unwrap();
        assert!(rows.iter().all(|row| row.get::<String>("name") != removed));
    }
    let asset = id(&first, "asset_id");
    publish(&service, &auth, "v2", "Atlas current version").await;
    assert_eq!(
        service.get(&auth, asset, Some(1)).await.unwrap()["content"],
        original
    );
    service.engine.shutdown().await.unwrap();
    drop(service);
    let service = crate::support::open_service(dir.path(), Models::disabled())
        .await
        .unwrap();
    service
        .restore(
            &auth,
            "restore",
            asset,
            RestoreInput {
                target_version: 1,
                expected_version: 2,
                reason: "restore immutable bytes".into(),
            },
        )
        .await
        .unwrap();
    assert!(worker::process_next(&service).await.unwrap());
    let restored = service.get(&auth, asset, None).await.unwrap();
    assert_eq!(restored["version"], 3);
    assert_eq!(restored["restored_from"], 1);
    assert_eq!(restored["content"], original);
}

#[tokio::test]
async fn damaged_frozen_terms_fail_without_retokenizing_or_republishing() {
    for field in ["search_terms", "summary_terms"] {
        let (dir, service, auth, first) = setup().await;
        let fixture = crate::support::fixture(dir.path(), None);
        fixture.reject_writes(true);
        let second = publish(&service, &auth, "retry", "Atlas changed source").await;
        let job = id(&second, "job_id");
        assert_eq!(
            service.job(&auth, job).await.unwrap()["state"],
            "retry_wait"
        );
        let attempts = fixture.posts();
        let path = format!("$.publication.memories[0].chunks[0].{field}");
        db::query("UPDATE oc_jobs SET payload=json_remove(payload,?),next_retry_at=0 WHERE tenant_id=? AND workspace_id=? AND id=?")
            .bind(path).bind(auth.tenant_id).bind(auth.workspace_id).bind(job)
            .execute(service.engine.relational().pool()).await.unwrap();
        fixture.reject_writes(false);
        assert!(worker::process_next(&service).await.unwrap());
        assert_eq!(service.job(&auth, job).await.unwrap()["state"], "failed");
        assert_eq!(fixture.posts(), attempts);
        assert_eq!(fixture.snapshot_records().len(), 1);
        assert_eq!(
            service
                .get(&auth, id(&first, "asset_id"), None)
                .await
                .unwrap()["version"],
            1
        );
    }
}

#[tokio::test]
async fn missing_and_changed_remote_content_fail_without_fallback() {
    for replacement in [
        Value::Null,
        json!({"content":"tampered","locator":{},"summary":null}),
    ] {
        let (dir, service, auth, receipt) = setup().await;
        let fixture = crate::support::fixture(dir.path(), None);
        let record = fixture.snapshot_records().pop().unwrap();
        fixture.mutate_payload(id(&record["document"], "chunk_id"), |payload| {
            *payload = replacement
        });
        assert!(
            service
                .get(&auth, id(&receipt, "asset_id"), None)
                .await
                .is_err()
        );
        assert!(
            service
                .search(
                    &auth,
                    serde_json::from_value(json!({"query":"Atlas","mode":"keyword"})).unwrap()
                )
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn authority_changes_during_content_read_block_get_search_and_context() {
    for (route, revoke) in [
        ("get", true),
        ("search", true),
        ("context", true),
        ("get", false),
        ("search", false),
        ("context", false),
    ] {
        let (dir, service, auth, receipt) = setup().await;
        let source = id(&receipt, "source_event_id");
        let gate = crate::support::fixture(dir.path(), None).pause_next_content_read();
        let reader = service.clone();
        let principal = auth.clone();
        let request = tokio::spawn(async move {
            match route {
                "get" => reader.get(&principal, id(&receipt, "asset_id"), None).await.map(|_| ()),
                "search" => reader.search(&principal, serde_json::from_value(json!({"query":"Atlas","mode":"keyword"})).unwrap()).await.map(|_| ()),
                _ => reader.evidence_bundle(&principal, serde_json::from_value(json!({"query":"Atlas","mode":"keyword","routes":[{"route":"memory","limit":1,"max_bytes":4000}]})).unwrap()).await.map(|_| ()),
            }
        });
        tokio::time::timeout(std::time::Duration::from_secs(10), gate.entered.notified())
            .await
            .unwrap();
        if revoke {
            service
                .engine
                .relational()
                .revoke_key(auth.id)
                .await
                .unwrap();
        } else {
            service
                .delete(&auth, "retract", source, "event")
                .await
                .unwrap();
        }
        gate.resume.notify_one();
        let result = request.await.unwrap();
        if revoke {
            assert!(matches!(result, Err(AppError::Forbidden)));
        } else {
            assert!(result.is_err());
        }
    }
}
