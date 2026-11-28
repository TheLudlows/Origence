//! Authentication and key inventory within one workspace.
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

impl SqliteTx {
    /// Enumerate this workspace's current credentials without exposing token hashes.
    pub async fn list_issued_keys(&mut self, after: Uuid, limit: u32) -> StorageResult<Value> {
        self.check_permission(Permission::WorkspaceAdmin).await?;
        self.list_issued_keys_inner(after, limit).await
    }

    pub(super) async fn list_issued_keys_inner(
        &mut self,
        after: Uuid,
        limit: u32,
    ) -> StorageResult<Value> {
        let count = key_page_size(limit)?;
        let rows: Vec<(Uuid, Uuid, String, i64)> = sqlite_db::query_as(
            "SELECT id,logical_key_id,role,created_at
             FROM oc_api_keys WHERE tenant_id=? AND workspace_id=? AND NOT revoked AND id>?
             ORDER BY id LIMIT ?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(after)
        .bind(i64::from(limit) + 1)
        .fetch_all(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        let next_after = (rows.len() > count).then(|| rows[count - 1].0);
        let items: Vec<_> = rows.into_iter().take(count).map(|(key_id, logical_key_id, role, created_at_ms)| {
            json!({"key_id":key_id,"logical_key_id":logical_key_id,"role":role,"created_at_ms":created_at_ms})
        }).collect();
        Ok(json!({"items":items,"next_after":next_after}))
    }
}

fn key_page_size(limit: u32) -> StorageResult<usize> {
    if !(1..=100).contains(&limit) {
        return Err(StorageError::InvalidArgument(
            "key page limit must be 1–100",
        ));
    }
    Ok(limit as usize)
}
