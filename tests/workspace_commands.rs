//! Generic workspace ownership, live key capability and command-domain tests.
//! These deliberately use no external protocol DTOs, mappings or routes.
use origence::storage::sqlite_db as db;
use origence::storage::{
    AuthorizedScope, DomainTx, JobQueue, RelationalStore, Scope, StorageError, WorkItem,
    sqlite::SqliteStore,
};
use serde_json::json;
use uuid::Uuid;

async fn root(store: &SqliteStore, tenant_id: Uuid) -> (AuthorizedScope, String) {
    let scope = Scope {
        tenant_id,
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(tenant_id, scope.workspace_id, "Root")
        .await
        .unwrap();
    let key = store.issue_key(scope, "admin").await.unwrap();
    let auth = store.authenticate(&key.token).await.unwrap();
    (auth, key.token)
}

async fn issue(
    store: &SqliteStore,
    admin: &AuthorizedScope,
    role: &str,
) -> (AuthorizedScope, String) {
    let mut tx = store.begin(admin.clone()).await.unwrap();
    let key = tx.issue_scoped_key(role).await.unwrap();
    tx.commit().await.unwrap();
    let auth = store.authenticate(&key.token).await.unwrap();
    (auth, key.token)
}

#[tokio::test]
async fn workspace_command_replay_preserves_original_creator_after_revocation() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("dedup.db"))
        .await
        .unwrap();
    let (admin, _) = root(&store, Uuid::new_v4()).await;
    let (first, _) = issue(&store, &admin, "writer").await;
    let (second, _) = issue(&store, &admin, "writer").await;
    let job_id = Uuid::new_v4();
    let receipt = json!({"job_id":job_id});
    let mut tx = store.begin(first.clone()).await.unwrap();
    assert!(
        tx.begin_command("message_batch.ingest", "same-key", "same-input")
            .await
            .unwrap()
            .is_none()
    );
    tx.enqueue(WorkItem {
        job_id,
        kind: "message_batch_ingest".into(),
        payload: json!({}),
        asset: None,
        source: None,
    })
    .await
    .unwrap();
    tx.finish_command(receipt.clone()).await.unwrap();
    tx.commit().await.unwrap();
    store.revoke_key(first.principal_id).await.unwrap();
    let mut tx = store.begin(second.clone()).await.unwrap();
    assert_eq!(
        tx.begin_command("message_batch.ingest", "same-key", "same-input")
            .await
            .unwrap(),
        Some(receipt)
    );
    assert!(matches!(
        tx.begin_command("message_batch.ingest", "same-key", "changed-input")
            .await,
        Err(StorageError::Conflict(_))
    ));
    tx.commit().await.unwrap();
    let (creator, domain): (Uuid, String) = db::query_as(
        "SELECT principal_id,dedup_scope FROM oc_commands WHERE operation='message_batch.ingest'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(creator, first.principal_id);
    assert_eq!(domain, "workspace");
    let claim = store.claim_next().await.unwrap().unwrap();
    assert_eq!(claim.job_id, job_id);
    assert_eq!(claim.created_by, first.principal_id);
    // Worker authorization uses this original principal, never the replay key.
    assert!(matches!(
        store
            .begin(AuthorizedScope {
                scope: claim.scope,
                principal_id: claim.created_by,
                role: "admin".into()
            })
            .await,
        Err(StorageError::Forbidden)
    ));
    assert_eq!(
        db::query_scalar::<i64>("SELECT count(*) FROM oc_jobs")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn ordinary_commands_keep_principal_isolation_and_receipts_require_writer() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("commands.db"))
        .await
        .unwrap();
    let (admin, _) = root(&store, Uuid::new_v4()).await;
    let (other, _) = issue(&store, &admin, "writer").await;
    for auth in [&admin, &other] {
        let mut tx = store.begin(auth.clone()).await.unwrap();
        assert!(
            tx.begin_command("memory.create", "same-key", "same-input")
                .await
                .unwrap()
                .is_none()
        );
        tx.finish_command(json!({"creator":auth.principal_id}))
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    let rows: Vec<(Uuid, String)> =
        db::query_as("SELECT principal_id,dedup_scope FROM oc_commands")
            .fetch_all(store.pool())
            .await
            .unwrap();
    assert_eq!(rows.len(), 2);
    for (principal, domain) in rows {
        assert_eq!(domain, format!("principal:{principal}"));
    }
    let (reader, _) = issue(&store, &admin, "reader").await;
    let mut tx = store.begin(reader).await.unwrap();
    assert!(matches!(
        tx.begin_command("message_batch.ingest", "same-key", "same-input")
            .await,
        Err(StorageError::Forbidden)
    ));
    tx.rollback().await.unwrap();
}

#[cfg(feature = "host")]
mod retry_tests {
    use super::*;
    use origence::{error::AppError, models::Models, types::KnowledgeInput};

    fn input() -> KnowledgeInput {
        KnowledgeInput {
            title: "Retry authority".into(),
            content: Some("Original creator remains responsible for publication.".into()),
            file_id: None,
            format: "text".into(),
            asset_id: None,
            expected_version: None,
        }
    }

    #[tokio::test]
    async fn live_creator_allows_another_writer_to_retry_without_transferring_identity() {
        let dir = tempfile::tempdir().unwrap();
        let service = crate::support::open_service(dir.path(), Models::disabled())
            .await
            .unwrap();
        let store = service.engine.relational();
        let (admin, _) = root(store, Uuid::new_v4()).await;
        let (writer, writer_token) = issue(store, &admin, "writer").await;
        let (_, other_writer_token) = issue(store, &admin, "writer").await;
        let writer_auth = service.auth(&writer_token).await.unwrap();
        let other_writer = service.auth(&other_writer_token).await.unwrap();
        let accepted = service
            .knowledge(&writer_auth, "accept", input())
            .await
            .unwrap();
        let job: Uuid = serde_json::from_value(accepted["job_id"].clone()).unwrap();
        db::query("UPDATE oc_jobs SET state='failed' WHERE id=?")
            .bind(job)
            .execute(store.pool())
            .await
            .unwrap();
        let retried = service
            .job_action(&other_writer, "retry", job, "retry")
            .await
            .unwrap();
        assert_eq!(retried["state"], "pending");
        let creator: Uuid = db::query_scalar("SELECT created_by FROM oc_jobs WHERE id=?")
            .bind(job)
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(creator, writer.principal_id);
        assert!(origence::worker::process_next(&service).await.unwrap());
        assert_eq!(
            service.job(&other_writer, job).await.unwrap()["state"],
            "completed"
        );
        service.engine.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn cleanup_retry_requires_current_admin_but_not_the_revoked_deleting_key() {
        let dir = tempfile::tempdir().unwrap();
        let service = crate::support::open_service(dir.path(), Models::disabled())
            .await
            .unwrap();
        let store = service.engine.relational();
        let (creator, creator_token) = root(store, Uuid::new_v4()).await;
        let (_, admin_token) = issue(store, &creator, "admin").await;
        let (_, other_writer_token) = issue(store, &creator, "writer").await;
        let original = service.auth(&creator_token).await.unwrap();
        let admin = service.auth(&admin_token).await.unwrap();
        let other_writer = service.auth(&other_writer_token).await.unwrap();
        let accepted = service
            .knowledge(&original, "accept", input())
            .await
            .unwrap();
        let asset: Uuid = serde_json::from_value(accepted["asset_id"].clone()).unwrap();
        let source: Uuid = serde_json::from_value(accepted["source_event_id"].clone()).unwrap();
        assert!(origence::worker::process_next(&service).await.unwrap());
        assert!(service.get(&original, asset, None).await.is_ok());
        let deleted = service
            .delete(&original, "delete", source, "event")
            .await
            .unwrap();
        let job: Uuid = serde_json::from_value(deleted["cleanup_job_id"].clone()).unwrap();
        // Simulate a failed native reclamation after the durable tombstone.
        db::query("UPDATE oc_jobs SET state='failed' WHERE id=?")
            .bind(job)
            .execute(store.pool())
            .await
            .unwrap();
        service
            .revoke_key(&admin, creator.principal_id)
            .await
            .unwrap();
        assert!(matches!(
            service.get(&admin, asset, None).await,
            Err(AppError::NotFound)
        ));
        assert!(matches!(
            service
                .job_action(&other_writer, "retry", job, "retry")
                .await,
            Err(AppError::Forbidden)
        ));
        assert!(matches!(
            service.job_action(&admin, "cancel", job, "cancel").await,
            Err(AppError::Conflict(_))
        ));
        let retried = service
            .job_action(&admin, "retry", job, "retry")
            .await
            .unwrap();
        assert_eq!(retried["state"], "pending");
        assert_eq!(retried["generation"], 2);
        assert!(origence::worker::process_next(&service).await.unwrap());
        let (saved_creator, state): (Uuid, String) =
            db::query_as("SELECT created_by,state FROM oc_jobs WHERE id=?")
                .bind(job)
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(saved_creator, creator.principal_id);
        assert_eq!(state, "completed");
        let remaining: i64 = db::query_scalar("SELECT count(*) FROM oc_chunks WHERE asset_id=?")
            .bind(asset)
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(remaining, 0);
        assert!(matches!(
            service.get(&admin, asset, None).await,
            Err(AppError::NotFound)
        ));
        service.engine.shutdown().await.unwrap();
    }
}
