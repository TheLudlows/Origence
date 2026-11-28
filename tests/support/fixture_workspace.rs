use origence::storage::{Lifecycle, RelationalStore, Scope, SqliteStore};
use std::path::Path;
use uuid::Uuid;

/// Seed synthetic workspaces used by integration fixtures.
pub async fn seed_workspace(
    dir: &Path,
    name: &str,
    same_tenant_as: Option<Uuid>,
    single_key: bool,
) -> anyhow::Result<serde_json::Value> {
    let store = SqliteStore::open(dir.join("context.db")).await?;
    let scope = Scope {
        tenant_id: match same_tenant_as {
            Some(workspace) => store.workspace_scope(workspace).await?.tenant_id,
            None => Uuid::new_v4(),
        },
        workspace_id: Uuid::new_v4(),
    };
    let key = if single_key {
        store.create_key_workspace(scope, name).await?
    } else {
        store
            .create_workspace(scope.tenant_id, scope.workspace_id, name)
            .await?;
        store.issue_key(scope, "admin").await?
    };
    store.shutdown().await?;
    Ok(
        serde_json::json!({"tenant_id":scope.tenant_id,"workspace_id":scope.workspace_id,"key_id":key.key_id,"token":key.token}),
    )
}
