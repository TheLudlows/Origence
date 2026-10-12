//! Generic workspace ownership, live key capability and command-domain tests.
//! These deliberately use no external protocol DTOs, mappings or routes.
use origence::storage::{
    AuthorizedScope, DomainTx, JobQueue, KeyCapabilities, Lifecycle, Permission, RelationalStore,
    Scope, StorageError, WorkItem, sqlite::SqliteStore,
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
    allow_owned_workspaces: bool,
) -> (AuthorizedScope, String) {
    let mut tx = store.begin(admin.clone()).await.unwrap();
    let key = tx
        .issue_scoped_key_with_capabilities(
            role,
            KeyCapabilities {
                allow_owned_workspaces,
            },
        )
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let auth = store.authenticate(&key.token).await.unwrap();
    (auth, key.token)
}

#[tokio::test]
async fn explicit_capability_concurrent_ensure_and_same_version_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("owned.db");
    let store = SqliteStore::open(&path).await.unwrap();
    let (admin, _) = root(&store, Uuid::new_v4()).await;
    let target = Uuid::new_v4();
    assert!(matches!(
        store
            .ensure_owned_workspace(admin.clone(), target, "Child")
            .await,
        Err(StorageError::Forbidden)
    ));
    assert!(matches!(
        store.lookup_owned_workspace(admin.clone(), target).await,
        Err(StorageError::Forbidden)
    ));
    let (capable, token) = issue(&store, &admin, "admin", true).await;
    assert!(
        store
            .lookup_owned_workspace(capable.clone(), target)
            .await
            .unwrap()
            .is_none()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM oc_workspaces")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
    let (left, right) = tokio::join!(
        store.ensure_owned_workspace(capable.clone(), target, "Child"),
        store.ensure_owned_workspace(capable.clone(), target, "Other display name")
    );
    let child = left.unwrap();
    assert_eq!(child, right.unwrap());
    assert_eq!(child.principal_id, capable.principal_id);
    assert_eq!(child.role, "writer");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM oc_workspaces")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count, 2);
    let owner: Uuid = sqlx::query_scalar("SELECT owner_workspace_id FROM oc_workspaces WHERE id=?")
        .bind(target)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(owner, admin.scope.workspace_id);
    store.shutdown().await.unwrap();
    drop(store);
    let store = SqliteStore::open(&path).await.unwrap();
    let capable = store.authenticate(&token).await.unwrap();
    assert_eq!(
        store.lookup_owned_workspace(capable, target).await.unwrap(),
        Some(child)
    );
}

#[tokio::test]
async fn reader_lookup_is_read_only_and_missing_targets_still_require_authority() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("reader.db"))
        .await
        .unwrap();
    let (admin, _) = root(&store, Uuid::new_v4()).await;
    let (writer, _) = issue(&store, &admin, "writer", true).await;
    let target = Uuid::new_v4();
    store
        .ensure_owned_workspace(writer, target, "Child")
        .await
        .unwrap();
    let (reader, _) = issue(&store, &admin, "reader", true).await;
    let child = store
        .lookup_owned_workspace(reader.clone(), target)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(child.role, "reader");
    let mut tx = store.begin(child).await.unwrap();
    tx.check_permission(Permission::Read).await.unwrap();
    assert!(matches!(
        tx.check_permission(Permission::Write).await,
        Err(StorageError::Forbidden)
    ));
    tx.rollback().await.unwrap();
    assert!(matches!(
        store
            .ensure_owned_workspace(reader.clone(), Uuid::new_v4(), "Denied")
            .await,
        Err(StorageError::Forbidden)
    ));
    assert!(
        store
            .lookup_owned_workspace(reader.clone(), Uuid::new_v4())
            .await
            .unwrap()
            .is_none()
    );
    store.revoke_key(reader.principal_id).await.unwrap();
    assert!(matches!(
        store.lookup_owned_workspace(reader, Uuid::new_v4()).await,
        Err(StorageError::Forbidden)
    ));
}

#[tokio::test]
async fn ownership_conflicts_and_forged_scopes_fail_without_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("conflict.db"))
        .await
        .unwrap();
    let (admin, _) = root(&store, Uuid::new_v4()).await;
    let (capable, _) = issue(&store, &admin, "admin", true).await;
    let target = Uuid::new_v4();
    let child = store
        .ensure_owned_workspace(capable.clone(), target, "Child")
        .await
        .unwrap();
    let (other, _) = root(&store, admin.scope.tenant_id).await;
    let (foreign, _) = root(&store, Uuid::new_v4()).await;
    for root in [other, foreign] {
        let (intruder, _) = issue(&store, &root, "admin", true).await;
        assert!(matches!(
            store
                .ensure_owned_workspace(intruder.clone(), target, "Denied")
                .await,
            Err(StorageError::Conflict(_))
        ));
        assert!(matches!(
            store.lookup_owned_workspace(intruder.clone(), target).await,
            Err(StorageError::Conflict(_))
        ));
        let forged = AuthorizedScope {
            scope: child.scope,
            ..intruder
        };
        assert!(matches!(
            store.begin(forged).await,
            Err(StorageError::Forbidden)
        ));
    }
    // An ordinary root admin cannot gain access by changing AuthContext.scope.
    let forged = AuthorizedScope {
        scope: child.scope,
        ..admin.clone()
    };
    assert!(matches!(
        store.begin(forged).await,
        Err(StorageError::Forbidden)
    ));
    let forged = AuthorizedScope {
        role: "admin".into(),
        ..child.clone()
    };
    let mut tx = store.begin(forged).await.unwrap();
    assert!(matches!(
        tx.check_permission(Permission::Delete).await,
        Err(StorageError::Forbidden)
    ));
    tx.rollback().await.unwrap();
    assert!(matches!(
        store
            .ensure_owned_workspace(capable.clone(), admin.scope.workspace_id, "Self")
            .await,
        Err(StorageError::Conflict(_))
    ));
    let actual: (Uuid, Option<Uuid>, String) =
        sqlx::query_as("SELECT tenant_id,owner_workspace_id,name FROM oc_workspaces WHERE id=?")
            .bind(target)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(
        actual,
        (
            admin.scope.tenant_id,
            Some(admin.scope.workspace_id),
            "Child".into()
        )
    );
}

#[tokio::test]
async fn owned_access_has_no_admin_or_second_level_capability() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("depth.db"))
        .await
        .unwrap();
    let (admin, _) = root(&store, Uuid::new_v4()).await;
    let (capable, _) = issue(&store, &admin, "admin", true).await;
    let child = store
        .ensure_owned_workspace(capable, Uuid::new_v4(), "Child")
        .await
        .unwrap();
    let mut tx = store.begin(child.clone()).await.unwrap();
    for permission in [Permission::Review, Permission::Publish, Permission::Delete] {
        assert!(matches!(
            tx.check_permission(permission).await,
            Err(StorageError::Forbidden)
        ));
    }
    assert!(matches!(
        tx.issue_scoped_key("admin").await,
        Err(StorageError::Forbidden)
    ));
    tx.rollback().await.unwrap();
    assert!(matches!(
        store
            .ensure_owned_workspace(child.clone(), Uuid::new_v4(), "Grandchild")
            .await,
        Err(StorageError::Forbidden)
    ));
    // Direct child administrators are valid for that child only. Even manually
    // restoring a capability bit cannot authorize a second ownership level.
    let key = store.issue_key(child.scope, "admin").await.unwrap();
    let direct = store.authenticate(&key.token).await.unwrap();
    assert!(matches!(
        store
            .issue_key_with_capabilities(
                child.scope,
                "admin",
                KeyCapabilities {
                    allow_owned_workspaces: true
                }
            )
            .await,
        Err(StorageError::Forbidden)
    ));
    let mut tx = store.begin(direct.clone()).await.unwrap();
    tx.check_permission(Permission::Delete).await.unwrap();
    assert!(matches!(
        tx.issue_scoped_key_with_capabilities(
            "admin",
            KeyCapabilities {
                allow_owned_workspaces: true
            }
        )
        .await,
        Err(StorageError::Forbidden)
    ));
    tx.rollback().await.unwrap();
    sqlx::query("UPDATE oc_api_keys SET allow_owned_workspaces=1 WHERE id=?")
        .bind(key.key_id)
        .execute(store.pool())
        .await
        .unwrap();
    assert!(matches!(
        store
            .ensure_owned_workspace(direct.clone(), Uuid::new_v4(), "Grandchild")
            .await,
        Err(StorageError::Forbidden)
    ));
    assert!(matches!(
        store.lookup_owned_workspace(direct, Uuid::new_v4()).await,
        Err(StorageError::Forbidden)
    ));
}

#[tokio::test]
async fn live_revocation_role_and_capability_override_stale_contexts() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("live.db")).await.unwrap();
    let (admin, _) = root(&store, Uuid::new_v4()).await;
    let (capable, _) = issue(&store, &admin, "admin", true).await;
    let target = Uuid::new_v4();
    let child = store
        .ensure_owned_workspace(capable.clone(), target, "Child")
        .await
        .unwrap();
    let mut tx = store.begin(child.clone()).await.unwrap();
    tx.check_permission(Permission::Write).await.unwrap();
    tx.commit().await.unwrap();
    // This models the gap occupied by model/native I/O before a fresh read or
    // publish transaction. No cached AuthContext role can restore permission.
    sqlx::query("UPDATE oc_api_keys SET role='reader' WHERE id=?")
        .bind(capable.principal_id)
        .execute(store.pool())
        .await
        .unwrap();
    let mut tx = store.begin(child.clone()).await.unwrap();
    assert!(matches!(
        tx.check_permission(Permission::Write).await,
        Err(StorageError::Forbidden)
    ));
    tx.check_permission(Permission::Read).await.unwrap();
    tx.commit().await.unwrap();
    sqlx::query("UPDATE oc_api_keys SET role='admin',allow_owned_workspaces=0 WHERE id=?")
        .bind(capable.principal_id)
        .execute(store.pool())
        .await
        .unwrap();
    assert!(matches!(
        store.begin(child.clone()).await,
        Err(StorageError::Forbidden)
    ));
    assert!(matches!(
        store
            .lookup_owned_workspace(capable.clone(), Uuid::new_v4())
            .await,
        Err(StorageError::Forbidden)
    ));
    sqlx::query("UPDATE oc_api_keys SET allow_owned_workspaces=1 WHERE id=?")
        .bind(capable.principal_id)
        .execute(store.pool())
        .await
        .unwrap();
    store.revoke_key(capable.principal_id).await.unwrap();
    assert!(matches!(
        store.begin(child).await,
        Err(StorageError::Forbidden)
    ));
    assert!(matches!(
        store.lookup_owned_workspace(capable, target).await,
        Err(StorageError::Forbidden)
    ));
}

#[tokio::test]
async fn replacement_keys_never_inherit_capability() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("rotation.db"))
        .await
        .unwrap();
    let (admin, _) = root(&store, Uuid::new_v4()).await;
    let (capable, _) = issue(&store, &admin, "admin", true).await;
    let target = Uuid::new_v4();
    store
        .ensure_owned_workspace(capable.clone(), target, "Child")
        .await
        .unwrap();
    let mut tx = store.begin(capable).await.unwrap();
    let replacement = tx.issue_scoped_key("admin").await.unwrap();
    tx.commit().await.unwrap();
    let replacement = store.authenticate(&replacement.token).await.unwrap();
    assert!(matches!(
        store
            .lookup_owned_workspace(replacement.clone(), target)
            .await,
        Err(StorageError::Forbidden)
    ));
    let (granted, _) = issue(&store, &replacement, "writer", true).await;
    assert!(
        store
            .lookup_owned_workspace(granted, target)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        KeyCapabilities::default(),
        serde_json::from_value::<KeyCapabilities>(json!({})).unwrap()
    );
}

#[tokio::test]
async fn workspace_command_replay_preserves_original_creator_after_revocation() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("dedup.db"))
        .await
        .unwrap();
    let (admin, _) = root(&store, Uuid::new_v4()).await;
    let (first, _) = issue(&store, &admin, "writer", true).await;
    let (second, _) = issue(&store, &admin, "writer", true).await;
    let target = Uuid::new_v4();
    let first = store
        .ensure_owned_workspace(first, target, "Child")
        .await
        .unwrap();
    let second = store
        .lookup_owned_workspace(second, target)
        .await
        .unwrap()
        .unwrap();
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
    let (creator, domain): (Uuid, String) = sqlx::query_as(
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
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM oc_jobs")
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
    let (other, _) = issue(&store, &admin, "writer", false).await;
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
        sqlx::query_as("SELECT principal_id,dedup_scope FROM oc_commands")
            .fetch_all(store.pool())
            .await
            .unwrap();
    assert_eq!(rows.len(), 2);
    for (principal, domain) in rows {
        assert_eq!(domain, format!("principal:{principal}"));
    }
    let (reader, _) = issue(&store, &admin, "reader", false).await;
    let mut tx = store.begin(reader).await.unwrap();
    assert!(matches!(
        tx.begin_command("message_batch.ingest", "same-key", "same-input")
            .await,
        Err(StorageError::Forbidden)
    ));
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn schema_enforces_same_tenant_one_level_and_immutable_ownership() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("schema.db"))
        .await
        .unwrap();
    let (admin, _) = root(&store, Uuid::new_v4()).await;
    let (capable, _) = issue(&store, &admin, "admin", true).await;
    let child = store
        .ensure_owned_workspace(capable, Uuid::new_v4(), "Child")
        .await
        .unwrap();
    for (tenant, id, owner) in [
        (Uuid::new_v4(), Uuid::new_v4(), admin.scope.workspace_id),
        (
            admin.scope.tenant_id,
            Uuid::new_v4(),
            child.scope.workspace_id,
        ),
        (
            admin.scope.tenant_id,
            child.scope.workspace_id,
            child.scope.workspace_id,
        ),
        (admin.scope.tenant_id, Uuid::new_v4(), Uuid::new_v4()),
    ] {
        assert!(sqlx::query("INSERT INTO oc_workspaces(tenant_id,id,name,owner_workspace_id,created_at) VALUES(?,?,'Invalid',?,0)")
            .bind(tenant).bind(id).bind(owner).execute(store.pool()).await.is_err());
    }
    assert!(
        sqlx::query("UPDATE oc_workspaces SET owner_workspace_id=NULL WHERE id=?")
            .bind(child.scope.workspace_id)
            .execute(store.pool())
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE oc_workspaces SET owner_workspace_id=? WHERE id=?")
            .bind(child.scope.workspace_id)
            .bind(admin.scope.workspace_id)
            .execute(store.pool())
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE oc_api_keys SET allow_owned_workspaces=2 WHERE id=?")
            .bind(admin.principal_id)
            .execute(store.pool())
            .await
            .is_err()
    );
    store.check().await.unwrap();
}

#[tokio::test]
async fn incompatible_owner_constraints_are_rejected_without_repair() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("incompatible.db");
    let store = SqliteStore::open(&path).await.unwrap();
    let (admin, _) = root(&store, Uuid::new_v4()).await;
    sqlx::query("DROP TRIGGER oc_workspace_owner_immutable")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(matches!(
        store.check().await,
        Err(StorageError::Unavailable(_))
    ));
    store.shutdown().await.unwrap();
    drop(store);
    assert!(matches!(
        SqliteStore::open(&path).await,
        Err(StorageError::Unavailable(_))
    ));
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(false),
        )
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM oc_workspaces")
            .fetch_one(&pool)
            .await
            .unwrap(),
        admin.scope.workspace_id
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM sqlite_master WHERE name='oc_workspace_owner_immutable'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        0
    );
    pool.close().await;
}

#[cfg(feature = "local-storage")]
mod retry_tests {
    use super::*;
    use origence::{error::AppError, models::Models, service::Service, types::KnowledgeInput};

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
    async fn live_creator_allows_reviewer_retry_without_transferring_identity() {
        let dir = tempfile::tempdir().unwrap();
        let service = Service::open(dir.path(), Models::disabled()).await.unwrap();
        let store = service.engine.relational();
        let (admin, _) = root(store, Uuid::new_v4()).await;
        let (writer, writer_token) = issue(store, &admin, "writer", false).await;
        let (_, reviewer_token) = issue(store, &admin, "reviewer", false).await;
        let writer_auth = service.auth(&writer_token).await.unwrap();
        let reviewer = service.auth(&reviewer_token).await.unwrap();
        let accepted = service
            .knowledge(&writer_auth, "accept", input())
            .await
            .unwrap();
        let job: Uuid = serde_json::from_value(accepted["job_id"].clone()).unwrap();
        sqlx::query("UPDATE oc_jobs SET state='failed' WHERE id=?")
            .bind(job)
            .execute(store.pool())
            .await
            .unwrap();
        let retried = service
            .job_action(&reviewer, "retry", job, "retry")
            .await
            .unwrap();
        assert_eq!(retried["state"], "pending");
        let creator: Uuid = sqlx::query_scalar("SELECT created_by FROM oc_jobs WHERE id=?")
            .bind(job)
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(creator, writer.principal_id);
        assert!(origence::worker::process_next(&service).await.unwrap());
        assert_eq!(
            service.job(&reviewer, job).await.unwrap()["state"],
            "completed"
        );
        service.engine.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn direct_child_admin_cannot_retry_a_revoked_root_creator() {
        let dir = tempfile::tempdir().unwrap();
        let service = Service::open(dir.path(), Models::disabled()).await.unwrap();
        let store = service.engine.relational();
        let (admin, _) = root(store, Uuid::new_v4()).await;
        let (writer, writer_token) = issue(store, &admin, "writer", true).await;
        let writer_auth = service.auth(&writer_token).await.unwrap();
        let child = service
            .ensure_owned_workspace(&writer_auth, Uuid::new_v4(), "Child")
            .await
            .unwrap();
        let accepted = service.knowledge(&child, "accept", input()).await.unwrap();
        let job: Uuid = serde_json::from_value(accepted["job_id"].clone()).unwrap();
        sqlx::query("UPDATE oc_jobs SET state='failed' WHERE id=?")
            .bind(job)
            .execute(store.pool())
            .await
            .unwrap();
        let direct = store
            .issue_key(
                Scope {
                    tenant_id: child.tenant_id,
                    workspace_id: child.workspace_id,
                },
                "admin",
            )
            .await
            .unwrap();
        let direct = service.auth(&direct.token).await.unwrap();
        store.revoke_key(writer.principal_id).await.unwrap();
        assert!(matches!(
            service.job_action(&direct, "retry", job, "retry").await,
            Err(AppError::Forbidden)
        ));
        let (creator, state, generation): (Uuid, String, i64) =
            sqlx::query_as("SELECT created_by,state,generation FROM oc_jobs WHERE id=?")
                .bind(job)
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(creator, writer.principal_id);
        assert_eq!(state, "failed");
        assert_eq!(generation, 1);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM oc_commands WHERE operation=?")
                .bind(format!("job/{job}/retry"))
                .fetch_one(store.pool())
                .await
                .unwrap(),
            0
        );
        service.engine.shutdown().await.unwrap();
    }
}
