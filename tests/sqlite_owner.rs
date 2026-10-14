//! Provenance owners through the two-phase cleanup path.

use origence::storage::{
    AuthorizedScope, DomainTx, Lifecycle, Permission, RelationalStore, Scope, SourceVersion,
    sqlite::SqliteStore,
};
use uuid::Uuid;

async fn provision(store: &SqliteStore) -> AuthorizedScope {
    let tenant = Uuid::new_v4();
    let workspace = Uuid::new_v4();
    store
        .create_workspace(tenant, workspace, "acme")
        .await
        .unwrap();
    let scope = Scope {
        tenant_id: tenant,
        workspace_id: workspace,
    };
    let key = store.issue_key(scope, "admin").await.unwrap();
    store.authenticate(&key.token).await.unwrap()
}

async fn begin_write(
    store: &SqliteStore,
    auth: &AuthorizedScope,
) -> origence::storage::sqlite::SqliteTx {
    let mut tx = store.begin(auth.clone()).await.unwrap();
    tx.check_permission(Permission::Write).await.unwrap();
    tx
}

#[tokio::test]
async fn cleanup_plan_retains_shared_owners_after_source_retraction() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("oc.db")).await.unwrap();
    let auth = provision(&store).await;
    let shared = Uuid::new_v4();
    let sole = Uuid::new_v4();
    let mut tx = begin_write(&store, &auth).await;
    let mut sources = Vec::new();
    for title in ["first", "second"] {
        let source_id = tx.create_event("knowledge", title, None).await.unwrap();
        let asset = tx.insert_knowledge_asset(title).await.unwrap();
        tx.insert_version(asset, 1, title, "hash", source_id, None, None)
            .await
            .unwrap();
        let source = SourceVersion {
            source_id,
            version: 1,
        };
        tx.register_owner(source, "entity", shared, None)
            .await
            .unwrap();
        sources.push(source);
    }
    tx.register_owner(sources[0], "entity", sole, None)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (_, mut entities, relations) = store.cleanup_plan(auth.scope).await.unwrap();
    entities.sort();
    let mut expected = vec![shared, sole];
    expected.sort();
    assert_eq!(entities, expected);
    assert!(relations.is_empty());
    let mut tx = begin_write(&store, &auth).await;
    tx.retract_event(sources[0].source_id).await.unwrap();
    tx.commit().await.unwrap();
    for _ in 0..2 {
        assert_eq!(
            store.cleanup_plan(auth.scope).await.unwrap(),
            (Vec::new(), vec![shared], Vec::new())
        );
    }
    let owners: Vec<Uuid> = origence::storage::native_turso::query_scalar(
        "SELECT source_id FROM oc_artifact_owners WHERE tenant_id=? AND workspace_id=?",
    )
    .bind(auth.scope.tenant_id)
    .bind(auth.scope.workspace_id)
    .fetch_all(store.pool())
    .await
    .unwrap();
    assert_eq!(owners, vec![sources[1].source_id]);
    store.shutdown().await.unwrap();
}
