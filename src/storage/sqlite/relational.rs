//! RelationalStore entry points: authentication, workspaces and API keys.
use super::*;

impl RelationalStore for SqliteStore {
    type Tx = SqliteTx;

    async fn begin(&self, authorized: AuthorizedScope) -> StorageResult<Self::Tx> {
        self.begin_scoped(authorized, true, None).await
    }

    async fn authenticate(&self, token: &str) -> StorageResult<AuthorizedScope> {
        self.authenticate_workspace(token, None)
            .await?
            .ok_or(StorageError::Forbidden)
    }

    async fn create_workspace(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
        name: &str,
    ) -> StorageResult<()> {
        sqlite_db::query(
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
        if role == "workspace" || !Permission::Read.granted_by(role) {
            return Err(StorageError::Conflict(format!("invalid role {role}")));
        }
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(sqlite_err)?;
        let exists: bool = sqlite_db::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM oc_workspaces WHERE tenant_id = ? AND id = ?)",
        )
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlite_err)?;
        if !exists {
            return Err(StorageError::NotFound);
        }
        if super::workspace_key::has_workspace_key(&mut tx, scope).await? {
            return Err(StorageError::Forbidden);
        }
        let key_id = Uuid::new_v4();
        let token = format!("oc_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        sqlite_db::query(
            "INSERT INTO oc_api_keys(id, logical_key_id, tenant_id, workspace_id, token_hash, role, revoked, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, 0, ?)",
        )
        .bind(key_id)
        .bind(key_id)
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(hash(&token))
        .bind(role)
        .bind(now_ms())
        .execute(&mut *tx)
        .await
        .map_err(sqlite_err)?;
        tx.commit().await.map_err(sqlite_err)?;
        Ok(IssuedKey { key_id, token })
    }

    async fn revoke_key(&self, key_id: Uuid) -> StorageResult<()> {
        sqlite_db::query("UPDATE oc_api_keys SET revoked = 1 WHERE id = ?")
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
