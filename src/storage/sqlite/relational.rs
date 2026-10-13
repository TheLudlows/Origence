//! RelationalStore entry points: authentication, workspaces and API keys.
use super::*;

impl RelationalStore for TursoStore {
    type Tx = TursoTx;

    async fn begin(&self, authorized: AuthorizedScope) -> StorageResult<Self::Tx> {
        self.begin_scoped(authorized, true).await
    }

    async fn authenticate(&self, token: &str) -> StorageResult<AuthorizedScope> {
        if token.len() < 32 || token.len() > 256 {
            return Err(StorageError::Forbidden);
        }
        let row: Option<(Uuid, Uuid, Uuid, String)> = native_turso::query_as(
            "SELECT tenant_id, workspace_id, id, role FROM oc_api_keys WHERE token_hash = ? AND NOT revoked",
        )
        .bind(hash(token))
        .fetch_optional(&self.pool)
        .await
        .map_err(turso_err)?;
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
        native_turso::query(
            "INSERT INTO oc_workspaces(tenant_id, id, name, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(name)
        .bind(now_ms())
        .execute(&self.pool)
        .await
        .map_err(turso_err)?;
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
        if !ROLES.contains(&role) {
            return Err(StorageError::Conflict(format!("invalid role {role}")));
        }
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(turso_err)?;
        let owner: Option<Option<Uuid>> = native_turso::query_scalar(
            "SELECT owner_workspace_id FROM oc_workspaces WHERE tenant_id = ? AND id = ?",
        )
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(turso_err)?;
        let owner = owner.ok_or(StorageError::NotFound)?;
        if capabilities.allow_owned_workspaces && owner.is_some() {
            return Err(StorageError::Forbidden);
        }
        let key_id = Uuid::new_v4();
        let token = format!("oc_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        native_turso::query(
            "INSERT INTO oc_api_keys(id, tenant_id, workspace_id, token_hash, role, allow_owned_workspaces, revoked, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, 0, ?)",
        )
        .bind(key_id)
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(hash(&token))
        .bind(role)
        .bind(capabilities.allow_owned_workspaces)
        .bind(now_ms())
        .execute(&mut *tx)
        .await
        .map_err(turso_err)?;
        tx.commit().await.map_err(turso_err)?;
        Ok(IssuedKey { key_id, token })
    }

    async fn ensure_owned_workspace(
        &self,
        auth: AuthorizedScope,
        workspace_id: Uuid,
        name: &str,
    ) -> StorageResult<AuthorizedScope> {
        if name.trim().is_empty() || name.len() > 256 || name.contains('\0') {
            return Err(StorageError::InvalidArgument(
                "workspace name must contain 1..256 UTF-8 bytes without NUL",
            ));
        }
        self.owned_workspace(auth, workspace_id, Some(name))
            .await?
            .ok_or(StorageError::NotFound)
    }

    async fn lookup_owned_workspace(
        &self,
        auth: AuthorizedScope,
        workspace_id: Uuid,
    ) -> StorageResult<Option<AuthorizedScope>> {
        self.owned_workspace(auth, workspace_id, None).await
    }

    async fn revoke_key(&self, key_id: Uuid) -> StorageResult<()> {
        native_turso::query("UPDATE oc_api_keys SET revoked = 1 WHERE id = ?")
            .bind(key_id)
            .execute(&self.pool)
            .await
            .map_err(turso_err)?;
        Ok(())
    }

    async fn reconcile_ledger(&self) -> StorageResult<Value> {
        self.reconcile_ledger_inner().await
    }
}

impl TursoStore {
    async fn owned_workspace(
        &self,
        auth: AuthorizedScope,
        workspace_id: Uuid,
        create: Option<&str>,
    ) -> StorageResult<Option<AuthorizedScope>> {
        let mut tx = self.begin_scoped(auth, create.is_some()).await?;
        let permission = if create.is_some() {
            Permission::Write
        } else {
            Permission::Read
        };
        // A forged/remapped AuthContext is insufficient. The live key must be
        // bound directly to this root, explicitly capable, and not revoked.
        let role = tx.require_owned_workspace_root(permission).await?;
        if workspace_id == tx.scope.workspace_id {
            return Err(StorageError::Conflict("workspace cannot own itself".into()));
        }
        // Lookup by global identity so a cross-tenant collision also fails;
        // never overwrite or silently select a different identity.
        let existing: Option<(Uuid, Option<Uuid>)> =
            native_turso::query_as("SELECT tenant_id,owner_workspace_id FROM oc_workspaces WHERE id=?")
                .bind(workspace_id)
                .fetch_optional(&mut *tx.tx)
                .await
                .map_err(turso_err)?;
        let found = match existing {
            Some((tenant, owner)) => {
                if tenant != tx.scope.tenant_id || owner != Some(tx.scope.workspace_id) {
                    return Err(StorageError::Conflict(
                        "workspace ownership does not match".into(),
                    ));
                }
                true
            }
            None => match create {
                Some(name) => {
                    native_turso::query(
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
                    .map_err(turso_err)?;
                    tx.audit("workspace.create_owned", workspace_id, json!({}))
                        .await?;
                    true
                }
                None => false,
            },
        };
        let authorized = found.then(|| AuthorizedScope {
            scope: Scope {
                tenant_id: tx.scope.tenant_id,
                workspace_id,
            },
            principal_id: tx.principal_id,
            role: if Permission::Write.granted_by(&role) {
                "writer"
            } else {
                "reader"
            }
            .into(),
        });
        tx.commit().await?;
        Ok(authorized)
    }
}

impl TursoTx {
    async fn require_owned_workspace_root(
        &mut self,
        permission: Permission,
    ) -> StorageResult<String> {
        let role: Option<String> = native_turso::query_scalar(
            "SELECT k.role FROM oc_api_keys k \
             JOIN oc_workspaces w ON w.tenant_id=k.tenant_id AND w.id=k.workspace_id \
             WHERE k.id=? AND k.tenant_id=? AND k.workspace_id=? AND NOT k.revoked \
               AND k.allow_owned_workspaces=1 AND w.owner_workspace_id IS NULL",
        )
        .bind(self.principal_id)
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        let role = role.ok_or(StorageError::Forbidden)?;
        if !permission.granted_by(&role) {
            return Err(StorageError::Forbidden);
        }
        self.required.push(permission);
        Ok(role)
    }
}
