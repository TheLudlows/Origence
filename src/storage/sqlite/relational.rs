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
        if tenant_id.is_nil() || workspace_id.is_nil() {
            return Err(StorageError::InvalidArgument("invalid workspace scope"));
        }
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
        self.issue_key_with_capabilities(scope, role, KeyCapabilities::default())
            .await
    }

    async fn issue_key_with_capabilities(
        &self,
        scope: Scope,
        role: &str,
        capabilities: KeyCapabilities,
    ) -> StorageResult<IssuedKey> {
        if role == "workspace" || !Permission::Read.granted_by(role) {
            return Err(StorageError::Conflict(format!("invalid role {role}")));
        }
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(sqlite_err)?;
        let owner: Option<Option<Uuid>> = sqlite_db::query_scalar(
            "SELECT owner_workspace_id FROM oc_workspaces WHERE tenant_id = ? AND id = ?",
        )
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(sqlite_err)?;
        let owner = owner.ok_or(StorageError::NotFound)?;
        if super::workspace_key::has_workspace_key(&mut tx, scope).await? {
            return Err(StorageError::Forbidden);
        }
        if capabilities.allow_owned_workspaces && owner.is_some() {
            return Err(StorageError::Forbidden);
        }
        let key_id = Uuid::new_v4();
        let token = format!("oc_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        sqlite_db::query(
            "INSERT INTO oc_api_keys(id, logical_key_id, tenant_id, workspace_id, token_hash, role, allow_owned_workspaces, revoked, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?)",
        )
        .bind(key_id)
        .bind(key_id)
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(hash(&token))
        .bind(role)
        .bind(capabilities.allow_owned_workspaces)
        .bind(now_ms())
        .execute(&mut *tx)
        .await
        .map_err(sqlite_err)?;
        tx.commit().await.map_err(sqlite_err)?;
        Ok(IssuedKey { key_id, token })
    }

    async fn ensure_owned_workspace(
        &self,
        auth: AuthorizedScope,
        workspace_id: Uuid,
        name: &str,
    ) -> StorageResult<AuthorizedScope> {
        self.owned_workspace(auth, workspace_id, Some(name), Permission::Write)
            .await?
            .ok_or(StorageError::NotFound)
    }

    async fn lookup_owned_workspace(
        &self,
        auth: AuthorizedScope,
        workspace_id: Uuid,
    ) -> StorageResult<Option<AuthorizedScope>> {
        self.owned_workspace(auth, workspace_id, None, Permission::Read)
            .await
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

impl SqliteStore {
    #[cfg(feature = "local-storage")]
    pub(crate) async fn list_managed_workspaces(
        &self,
        auth: AuthorizedScope,
        after: Uuid,
        limit: u32,
    ) -> StorageResult<Vec<(Uuid, String, String)>> {
        let mut tx = self
            .begin_scoped(auth, false, Some(Permission::WorkspaceAdmin))
            .await?;
        tx.require_root_workspace().await?;
        let (logical_id, home, home_role, default_access, capable) =
            tx.managed_workspace_key().await?;
        let rows = if capable && home == tx.scope.workspace_id && default_access {
            sqlite_db::query_as(
                "SELECT id,name,'writer' FROM oc_workspaces WHERE tenant_id=? AND owner_workspace_id=? \
                 AND id>? ORDER BY id LIMIT ?",
            )
            .bind(tx.scope.tenant_id)
            .bind(tx.scope.workspace_id)
            .bind(after)
            .bind(limit)
            .fetch_all(&mut *tx.tx)
            .await
            .map_err(sqlite_err)?
        } else {
            let mut grants = super::workspace_grants::additional_grants(
                &mut tx.tx,
                tx.scope.tenant_id,
                logical_id,
            )
            .await?;
            if default_access && home != tx.scope.workspace_id {
                grants.push((home, home_role));
                grants.sort_unstable_by_key(|(id, _)| *id);
            }
            let mut rows = Vec::new();
            for (id, role) in grants.into_iter().filter(|(id, _)| *id > after) {
                let name: Option<String> = sqlite_db::query_scalar(
                    "SELECT name FROM oc_workspaces WHERE tenant_id=? AND owner_workspace_id=? AND id=?",
                )
                .bind(tx.scope.tenant_id)
                .bind(tx.scope.workspace_id)
                .bind(id)
                .fetch_optional(&mut *tx.tx)
                .await
                .map_err(sqlite_err)?;
                if let Some(name) = name {
                    rows.push((id, name, role));
                    if rows.len() == limit as usize {
                        break;
                    }
                }
            }
            rows
        };
        tx.commit().await?;
        Ok(rows)
    }

    async fn owned_workspace(
        &self,
        auth: AuthorizedScope,
        workspace_id: Uuid,
        create: Option<&str>,
        permission: Permission,
    ) -> StorageResult<Option<AuthorizedScope>> {
        if create
            .is_some_and(|name| name.trim().is_empty() || name.len() > 256 || name.contains('\0'))
        {
            return Err(StorageError::InvalidArgument(
                "workspace name must contain 1..256 UTF-8 bytes without NUL",
            ));
        }
        let mut tx = self.begin_scoped(auth, create.is_some(), None).await?;
        let root_role = tx.require_owned_workspace_root(permission).await?;
        let role = if Permission::Write.granted_by(&root_role) {
            "writer"
        } else {
            "reader"
        };
        if workspace_id == tx.scope.workspace_id {
            return Err(StorageError::Conflict("workspace cannot own itself".into()));
        }
        // Lookup by global identity so a cross-tenant collision also fails;
        // never overwrite or silently select a different identity.
        let existing: Option<(Uuid, Option<Uuid>)> = sqlite_db::query_as(
            "SELECT tenant_id,owner_workspace_id FROM oc_workspaces WHERE id=?",
        )
        .bind(workspace_id)
        .fetch_optional(&mut *tx.tx)
        .await
        .map_err(sqlite_err)?;
        let scope = Scope {
            workspace_id,
            ..tx.scope
        };
        let target_role = match existing {
            Some((tenant, owner)) => {
                if tenant != tx.scope.tenant_id || owner != Some(tx.scope.workspace_id) {
                    return Err(StorageError::Conflict(
                        "workspace ownership does not match".into(),
                    ));
                }
                Some(role.into())
            }
            None => match create {
                Some(name) => {
                    sqlite_db::query(
                        "INSERT INTO oc_workspaces(tenant_id,id,name,owner_workspace_id,created_at) \
                         VALUES(?,?,?,?,?)",
                    )
                    .bind(tx.scope.tenant_id)
                    .bind(workspace_id)
                    .bind(name)
                    .bind(tx.scope.workspace_id)
                    .bind(now_ms())
                    .execute(&mut *tx.tx)
                    .await
                    .map_err(sqlite_err)?;
                    tx.audit("workspace.create_owned", workspace_id, json!({}))
                        .await?;
                    Some(role.into())
                }
                None => None,
            },
        };
        let authorized = target_role.map(|role| AuthorizedScope {
            scope,
            principal_id: tx.principal_id,
            role,
        });
        tx.commit().await?;
        Ok(authorized)
    }
}

impl SqliteTx {
    #[cfg(feature = "local-storage")]
    async fn managed_workspace_key(&mut self) -> StorageResult<(Uuid, Uuid, String, bool, bool)> {
        sqlite_db::query_as(
            "SELECT logical_key_id,workspace_id,role,default_access,allow_owned_workspaces=1
             FROM oc_api_keys WHERE id=? AND tenant_id=? AND NOT revoked",
        )
        .bind(self.principal_id)
        .bind(self.scope.tenant_id)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(sqlite_err)?
        .ok_or(StorageError::Forbidden)
    }

    async fn require_owned_workspace_root(
        &mut self,
        permission: Permission,
    ) -> StorageResult<String> {
        let role: Option<String> = sqlite_db::query_scalar(
            "SELECT role FROM oc_api_keys \
             WHERE id=? AND tenant_id=? AND workspace_id=? AND NOT revoked \
               AND default_access AND allow_owned_workspaces=1",
        )
        .bind(self.principal_id)
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        let role = role.ok_or(StorageError::Forbidden)?;
        if !permission.granted_by(&role) {
            return Err(StorageError::Forbidden);
        }
        self.require_root_workspace().await?;
        self.required.push(permission);
        Ok(role)
    }

    async fn require_root_workspace(&mut self) -> StorageResult<()> {
        let root: Option<Uuid> = sqlite_db::query_scalar(
            "SELECT id FROM oc_workspaces \
             WHERE tenant_id=? AND id=? AND owner_workspace_id IS NULL",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        if root.is_none() {
            return Err(StorageError::Forbidden);
        }
        Ok(())
    }
}
