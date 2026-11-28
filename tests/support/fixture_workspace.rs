use origence::storage::{Lifecycle, RelationalStore, Scope, SqliteStore};
use std::path::Path;
use uuid::Uuid;

/// Seed member-key workspaces used by HTTP integration fixtures.
pub async fn seed_member_workspace(
    dir: &Path,
    name: &str,
    same_tenant_as: Option<Uuid>,
) -> anyhow::Result<serde_json::Value> {
    let store = SqliteStore::open(dir.join("context.db")).await?;
    let scope = Scope {
        tenant_id: match same_tenant_as {
            Some(workspace) => store.workspace_scope(workspace).await?.tenant_id,
            None => Uuid::new_v4(),
        },
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, name)
        .await?;
    let key = store.issue_key(scope, "admin").await?;
    store.shutdown().await?;
    Ok(
        serde_json::json!({"tenant_id":scope.tenant_id,"workspace_id":scope.workspace_id,"key_id":key.key_id,"token":key.token}),
    )
}
