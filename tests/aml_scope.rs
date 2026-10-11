use origence::storage::{
    AuthorizedScope, DomainTx, Lifecycle, Permission, RelationalStore, Scope, StorageError,
    sqlite::SqliteStore,
};
use uuid::Uuid;

async fn namespace(store: &SqliteStore, tenant: Uuid) -> (Scope, AuthorizedScope, String) {
    let scope = Scope {
        tenant_id: tenant,
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "evaluation")
        .await
        .unwrap();
    let key = store.issue_key(scope, "admin").await.unwrap();
    let admin = store.authenticate(&key.token).await.unwrap();
    (scope, admin, key.token)
}

#[tokio::test]
async fn exact_ids_concurrent_creation_namespace_isolation_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("context.db");
    let store = SqliteStore::open(&path).await.unwrap();
    let (scope, admin, token) = namespace(&store, Uuid::new_v4()).await;
    assert!(matches!(
        store.ensure_aml_user(admin.clone(), "user").await,
        Err(StorageError::Forbidden)
    ));
    store.enable_aml_namespace(admin.clone()).await.unwrap();
    store.enable_aml_namespace(admin.clone()).await.unwrap();
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM oc_workspaces")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert!(
        store
            .lookup_aml_user(admin.clone(), "unknown")
            .await
            .unwrap()
            .is_none()
    );
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM oc_workspaces")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(before, after);
    let user = "eval:run-A:中文:🙂:conv-0";
    let (a, b) = tokio::join!(
        store.ensure_aml_user(admin.clone(), user),
        store.ensure_aml_user(admin.clone(), user)
    );
    let a = a.unwrap();
    assert_eq!(a, b.unwrap());
    assert_ne!(a.scope, scope);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM oc_workspaces")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count, before + 1);
    for other in [
        "eval:run-B:中文:🙂:conv-0",
        "conv-0",
        "User",
        "user",
        " user",
        "user ",
        "é",
        "e\u{301}",
    ] {
        let next = store.ensure_aml_user(admin.clone(), other).await.unwrap();
        assert_ne!(a.scope, next.scope);
    }
    let ids: Vec<String> = sqlx::query_scalar("SELECT user_id FROM oc_aml_users")
        .fetch_all(store.pool())
        .await
        .unwrap();
    assert_eq!(ids.len(), 9);
    assert!(ids.contains(&user.to_string()));
    let (_, sibling, _) = namespace(&store, scope.tenant_id).await;
    let (_, foreign, _) = namespace(&store, Uuid::new_v4()).await;
    for other in [sibling, foreign] {
        store.enable_aml_namespace(other.clone()).await.unwrap();
        assert!(
            store
                .lookup_aml_user(other.clone(), user)
                .await
                .unwrap()
                .is_none()
        );
        let b = store.ensure_aml_user(other.clone(), user).await.unwrap();
        assert_ne!(a.scope, b.scope);
        let mut forged = other;
        forged.scope = a.scope;
        assert!(matches!(
            store.begin(forged).await,
            Err(StorageError::Forbidden)
        ));
    }
    store.shutdown().await.unwrap();
    drop(store);
    let store = SqliteStore::open(&path).await.unwrap();
    let auth = store.authenticate(&token).await.unwrap();
    assert_eq!(store.lookup_aml_user(auth, user).await.unwrap().unwrap(), a);
}

#[tokio::test]
async fn permissions_are_live_and_user_scopes_cannot_grant_access() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("context.db"))
        .await
        .unwrap();
    let (scope, admin, _) = namespace(&store, Uuid::new_v4()).await;
    let writer_key = store.issue_key(scope, "writer").await.unwrap();
    let writer = store.authenticate(&writer_key.token).await.unwrap();
    let reader_key = store.issue_key(scope, "reader").await.unwrap();
    let reader = store.authenticate(&reader_key.token).await.unwrap();
    assert!(matches!(
        store.enable_aml_namespace(writer.clone()).await,
        Err(StorageError::Forbidden)
    ));
    store.enable_aml_namespace(admin.clone()).await.unwrap();
    let user = store.ensure_aml_user(writer.clone(), "user").await.unwrap();
    assert_eq!(user.role, "writer");
    assert!(matches!(
        store.enable_aml_namespace(user.clone()).await,
        Err(StorageError::Forbidden)
    ));
    assert!(matches!(
        store.ensure_aml_user(user.clone(), "nested").await,
        Err(StorageError::Forbidden)
    ));
    assert!(matches!(
        store.ensure_aml_user(reader.clone(), "other").await,
        Err(StorageError::Forbidden)
    ));
    let mut read_user = store
        .lookup_aml_user(reader, "user")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(read_user.scope, user.scope);
    read_user.role = "admin".into(); // caller-supplied role is never authoritative
    let mut tx = store.begin(read_user.clone()).await.unwrap();
    tx.check_permission(Permission::Read).await.unwrap();
    assert!(matches!(
        tx.check_permission(Permission::Write).await,
        Err(StorageError::Forbidden)
    ));
    tx.rollback().await.unwrap();
    let mut forged = user.clone();
    forged.scope.workspace_id = Uuid::new_v4();
    assert!(matches!(
        store.begin(forged).await,
        Err(StorageError::Forbidden)
    ));
    store.revoke_key(writer_key.key_id).await.unwrap();
    assert!(matches!(
        store.begin(user).await,
        Err(StorageError::Forbidden)
    ));
    assert!(matches!(
        store.ensure_aml_user(writer, "user").await,
        Err(StorageError::Forbidden)
    ));
    assert!(store.begin(read_user).await.is_ok());
}

#[tokio::test]
async fn invalid_ids_are_rejected_without_partial_workspaces() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("context.db"))
        .await
        .unwrap();
    let (_, admin, _) = namespace(&store, Uuid::new_v4()).await;
    store.enable_aml_namespace(admin.clone()).await.unwrap();
    for invalid in [String::new(), "a\0b".into(), "界".repeat(1366)] {
        assert!(matches!(
            store.ensure_aml_user(admin.clone(), &invalid).await,
            Err(StorageError::InvalidArgument(_))
        ));
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM oc_workspaces")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
    store
        .ensure_aml_user(admin.clone(), &"a".repeat(4096))
        .await
        .unwrap();
    // Fail after the workspace insert: the transaction must roll back both writes.
    sqlx::raw_sql("CREATE TRIGGER reject_aml BEFORE INSERT ON oc_aml_users BEGIN SELECT RAISE(ABORT, 'injected'); END;")
        .execute(store.pool()).await.unwrap();
    assert!(
        store
            .ensure_aml_user(admin.clone(), "failed")
            .await
            .is_err()
    );
    assert!(
        store
            .lookup_aml_user(admin, "failed")
            .await
            .unwrap()
            .is_none()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM oc_workspaces")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn old_databases_are_not_upgraded_and_partial_schemas_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("context.db");
    let store = SqliteStore::open(&path).await.unwrap();
    let (_, admin, _) = namespace(&store, Uuid::new_v4()).await;
    sqlx::raw_sql("DROP TABLE oc_aml_adds; DROP TABLE oc_aml_users; DROP TABLE oc_aml_namespaces;")
        .execute(store.pool())
        .await
        .unwrap();
    store.shutdown().await.unwrap();
    drop(store);
    let store = SqliteStore::open(&path).await.unwrap();
    assert!(matches!(
        store.enable_aml_namespace(admin.clone()).await,
        Err(StorageError::Unavailable(_))
    ));
    let mut tx = store.begin(admin).await.unwrap();
    tx.check_permission(Permission::Write).await.unwrap();
    tx.rollback().await.unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE name LIKE 'oc_aml_%'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(count, 0);
    sqlx::query("CREATE VIEW oc_aml_users AS SELECT 1 AS sentinel")
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
}

#[tokio::test]
async fn scope_only_database_keeps_mapping_but_does_not_silently_install_add_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("context.db");
    let store = SqliteStore::open(&path).await.unwrap();
    let (_, admin, _) = namespace(&store, Uuid::new_v4()).await;
    store.enable_aml_namespace(admin.clone()).await.unwrap();
    let user = store.ensure_aml_user(admin.clone(), "u").await.unwrap();
    sqlx::query("DROP TABLE oc_aml_adds")
        .execute(store.pool())
        .await
        .unwrap();
    store.shutdown().await.unwrap();
    drop(store);
    let store = SqliteStore::open(&path).await.unwrap();
    assert_eq!(
        store.lookup_aml_user(admin, "u").await.unwrap().unwrap(),
        user
    );
    let mut tx = store.begin(user).await.unwrap();
    assert!(matches!(
        tx.aml_add_record("r").await,
        Err(StorageError::Unavailable(_))
    ));
    tx.rollback().await.unwrap();
    sqlx::query("CREATE TABLE oc_aml_adds (request_id TEXT)")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(matches!(
        store.check().await,
        Err(StorageError::Unavailable(_))
    ));
    store.shutdown().await.unwrap();
}
