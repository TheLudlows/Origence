//! Commit-time permission contract against the real SQLite/Turso adapter.
//!
//! Scope, idempotency, audit, rollback and queue contracts live in sqlite_store.
//! No mock adapter is maintained alongside the production implementation.

use origence::storage::native_turso as db;
use origence::storage::{
    DomainTx, JobQueue, Permission, RelationalStore, Scope, StorageError, WorkItem,
    sqlite::SqliteStore,
};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn commit_rechecks_permission_and_rolls_back_after_live_role_changes() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("contract.db"))
        .await
        .unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "contract")
        .await
        .unwrap();
    let key = store.issue_key(scope, "writer").await.unwrap();
    let auth = store.authenticate(&key.token).await.unwrap();

    // External revocation serializes behind BEGIN IMMEDIATE (covered in
    // sqlite_store). This test-only trigger changes the actual authority row
    // inside the write transaction, isolating commit's permission recheck.
    db::raw_sql(
        "CREATE TRIGGER contract_downgrade_key AFTER INSERT ON oc_audit \
         WHEN NEW.action = 'contract.downgrade' BEGIN \
         UPDATE oc_api_keys SET role = 'reader' \
         WHERE id = NEW.actor AND tenant_id = NEW.tenant_id AND workspace_id = NEW.workspace_id; \
         END;",
    )
    .execute(store.pool())
    .await
    .unwrap();

    let mut tx = store.begin(auth.clone()).await.unwrap();
    tx.check_permission(Permission::Write).await.unwrap();
    tx.begin_command("memory.create", "commit-key", "hash")
        .await
        .unwrap();
    let asset = tx.insert_knowledge_asset("Must roll back").await.unwrap();
    tx.enqueue(WorkItem {
        job_id: Uuid::new_v4(),
        kind: "publish".into(),
        payload: json!(null),
        asset: Some(asset),
        source: None,
    })
    .await
    .unwrap();
    tx.audit("contract.downgrade", asset, json!({}))
        .await
        .unwrap();
    tx.finish_command(json!({"asset_id": asset})).await.unwrap();
    assert!(matches!(tx.commit().await, Err(StorageError::Forbidden)));

    // A denied commit must roll back all writes, including the role change.
    assert_eq!(store.authenticate(&key.token).await.unwrap(), auth);
    assert!(store.claim_next().await.unwrap().is_none());
    let assets: i64 = db::query_scalar("SELECT count(*) FROM oc_assets")
        .fetch_one(store.pool())
        .await
        .unwrap();
    let audits: i64 = db::query_scalar("SELECT count(*) FROM oc_audit")
        .fetch_one(store.pool())
        .await
        .unwrap();
    let commands: i64 = db::query_scalar("SELECT count(*) FROM oc_commands")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!((assets, audits, commands), (0, 0, 0));
}
