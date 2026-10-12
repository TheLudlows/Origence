//! RelationalStore entry points: authentication, workspaces and API keys.
use super::*;

impl RelationalStore for SqliteStore {
    type Tx = SqliteTx;

    async fn begin(&self, authorized: AuthorizedScope) -> StorageResult<Self::Tx> {
        self.begin_scoped(authorized, true).await
    }

    async fn authenticate(&self, token: &str) -> StorageResult<AuthorizedScope> {
        if token.len() < 32 || token.len() > 256 {
            return Err(StorageError::Forbidden);
        }
        let row: Option<(Uuid, Uuid, Uuid, String)> = sqlx::query_as(
            "SELECT tenant_id, workspace_id, id, role FROM oc_api_keys WHERE token_hash = ? AND NOT revoked",
        )
        .bind(hash(token))
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlite_err)?;
        let (tenant_id, workspace_id, principal_id, role) = row.ok_or(StorageError::Forbidden)?;
        Ok(AuthorizedScope {
            scope: Scope {
                tenant_id,
                workspace_id,
            },
            principal_id,
            role,
        })
    }

    async fn create_workspace(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
        name: &str,
    ) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO oc_workspaces(tenant_id, id, name, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(name)
        .bind(now_ms())
        .execute(&self.pool)
        .await
        .map_err(sqlite_err)?;
        Ok(())
    }

    async fn issue_key(&self, scope: Scope, role: &str) -> StorageResult<IssuedKey> {
        if !ROLES.contains(&role) {
            return Err(StorageError::Conflict(format!("invalid role {role}")));
        }
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM oc_workspaces WHERE tenant_id = ? AND id = ?)",
        )
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlite_err)?;
        if !exists {
            return Err(StorageError::NotFound);
        }
        let key_id = Uuid::new_v4();
        let token = format!("oc_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        sqlx::query(
            "INSERT INTO oc_api_keys(id, tenant_id, workspace_id, token_hash, role, revoked, created_at) \
             VALUES (?, ?, ?, ?, ?, 0, ?)",
        )
        .bind(key_id)
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(hash(&token))
        .bind(role)
        .bind(now_ms())
        .execute(&self.pool)
        .await
        .map_err(sqlite_err)?;
        Ok(IssuedKey { key_id, token })
    }

    async fn revoke_key(&self, key_id: Uuid) -> StorageResult<()> {
        sqlx::query("UPDATE oc_api_keys SET revoked = 1 WHERE id = ?")
            .bind(key_id)
            .execute(&self.pool)
            .await
            .map_err(sqlite_err)?;
        Ok(())
    }

    async fn reconcile_ledger(&self) -> StorageResult<Value> {
        self.reconcile_ledger_inner().await
    }
}
