use origence::storage::native_turso as db;
use origence::{
    memory_semantics::MemoryProperty,
    storage::{
        AuthorizedScope, DomainTx, RelationalStore, Scope, StorageError, sqlite::SqliteStore,
    },
};
use serde_json::json;
use uuid::Uuid;
async fn auth(store: &SqliteStore) -> AuthorizedScope {
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "catalog")
        .await
        .unwrap();
    let key = store.issue_key(scope, "admin").await.unwrap();
    store.authenticate(&key.token).await.unwrap()
}
fn property() -> MemoryProperty {
    serde_json::from_value(
        json!({"predicate":"database.engine","cardinality":"single","required_context":["env"]}),
    )
    .unwrap()
}
#[tokio::test]
async fn catalog_is_scoped_immutable_and_admin_only() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("test.db")).await.unwrap();
    let a = auth(&store).await;
    let b = auth(&store).await;
    let mut tx = store.begin(a.clone()).await.unwrap();
    tx.register_memory_property(&property()).await.unwrap();
    tx.register_memory_property(&property()).await.unwrap();
    tx.commit().await.unwrap();
    let mut tx = store.begin(b).await.unwrap();
    assert!(
        tx.memory_property("database.engine")
            .await
            .unwrap()
            .is_none()
    );
    tx.rollback().await.unwrap();
    let mut tx = store.begin(a.clone()).await.unwrap();
    let mut changed = property();
    changed.cardinality = origence::memory_semantics::Cardinality::Multi;
    assert!(matches!(
        tx.register_memory_property(&changed).await,
        Err(StorageError::Conflict(_))
    ));
    tx.rollback().await.unwrap();
    let key = store.issue_key(a.scope, "writer").await.unwrap();
    let mut tx = store
        .begin(store.authenticate(&key.token).await.unwrap())
        .await
        .unwrap();
    assert!(matches!(
        tx.register_memory_property(&property()).await,
        Err(StorageError::Forbidden)
    ));
    tx.rollback().await.unwrap();
}
#[tokio::test]
async fn catalog_never_reinterprets_existing_identity_or_legacy_keys() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("test.db")).await.unwrap();
    let a = auth(&store).await;
    let mut tx = store.begin(a).await.unwrap();
    let identity=serde_json::from_value(json!({"subject":{"kind":"project","stable_id":"Atlas"},"predicate":"database.engine","context":{"env":"production"}})).unwrap();
    tx.identity_slot(&identity).await.unwrap();
    assert!(matches!(
        tx.register_memory_property(&property()).await,
        Err(StorageError::Conflict(_))
    ));
    tx.rollback().await.unwrap();
}
#[tokio::test]
async fn old_database_does_not_get_catalog_on_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    {
        let store = SqliteStore::open(&path).await.unwrap();
        db::query("DROP TABLE oc_memory_properties")
            .execute(store.pool())
            .await
            .unwrap();
        store.pool().close().await;
    }
    let store = SqliteStore::open(&path).await.unwrap();
    let a = auth(&store).await;
    let mut tx = store.begin(a).await.unwrap();
    assert!(
        tx.memory_property("database.engine")
            .await
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        tx.register_memory_property(&property()).await,
        Err(StorageError::Unavailable(_))
    ));
    tx.rollback().await.unwrap();
}
