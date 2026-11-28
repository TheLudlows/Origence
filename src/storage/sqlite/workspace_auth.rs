//! Credential authentication within one workspace.
use super::*;
use crate::storage::WorkspaceAccess;

impl SqliteStore {
    /// An explicit workspace is a consistency check against the key's home.
    pub async fn authenticate_workspace(
        &self,
        token: &str,
        requested: Option<Uuid>,
    ) -> StorageResult<Option<AuthorizedScope>> {
        let mut tx = self.pool.begin().await.map_err(sqlite_err)?;
        let auth = credential(&mut tx, token).await?;
        tx.commit().await.map_err(sqlite_err)?;
        Ok(requested
            .is_none_or(|id| id == auth.scope.workspace_id)
            .then_some(auth))
    }

    pub async fn workspace_access(&self, token: &str) -> StorageResult<Vec<WorkspaceAccess>> {
        let mut tx = self.pool.begin().await.map_err(sqlite_err)?;
        let auth = credential(&mut tx, token).await?;
        let name =
            sqlite_db::query_scalar("SELECT name FROM oc_workspaces WHERE tenant_id=? AND id=?")
                .bind(auth.scope.tenant_id)
                .bind(auth.scope.workspace_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(sqlite_err)?;
        tx.commit().await.map_err(sqlite_err)?;
        Ok(vec![WorkspaceAccess {
            workspace_id: auth.scope.workspace_id,
            name,
            role: auth.role,
            is_default: true,
        }])
    }
}

async fn credential(conn: &mut Connection, token: &str) -> StorageResult<AuthorizedScope> {
    if token.len() < 32 || token.len() > 256 {
        return Err(StorageError::Forbidden);
    }
    let (tenant_id, workspace_id, principal_id, role) = sqlite_db::query_as(
        "SELECT tenant_id,workspace_id,id,role FROM oc_api_keys WHERE token_hash=? AND NOT revoked",
    )
    .bind(hash(token))
    .fetch_optional(conn)
    .await
    .map_err(sqlite_err)?
    .ok_or(StorageError::Forbidden)?;
    Ok(AuthorizedScope {
        scope: Scope {
            tenant_id,
            workspace_id,
        },
        principal_id,
        role,
    })
}
