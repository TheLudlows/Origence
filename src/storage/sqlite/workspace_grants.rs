//! Explicit grants for API credentials. Default access stays in the credential
//! row; additional grants belong to its stable logical identity.
use super::*;
use crate::storage::WorkspaceAccess;
use std::collections::HashSet;

struct LiveCredential {
    tenant_id: Uuid,
    default_workspace: Uuid,
    principal_id: Uuid,
    logical_id: Uuid,
    default_role: String,
    default_access: bool,
}

impl LiveCredential {
    async fn from_token(conn: &mut Connection, token: &str) -> StorageResult<Self> {
        if token.len() < 32 || token.len() > 256 {
            return Err(StorageError::Forbidden);
        }
        let (tenant_id, default_workspace, principal_id, logical_id, default_role, default_access) =
            sqlite_db::query_as(
                "SELECT tenant_id,workspace_id,id,logical_key_id,role,default_access
                 FROM oc_api_keys WHERE token_hash=? AND NOT revoked",
            )
            .bind(hash(token))
            .fetch_optional(conn)
            .await
            .map_err(sqlite_err)?
            .ok_or(StorageError::Forbidden)?;
        Ok(Self {
            tenant_id,
            default_workspace,
            principal_id,
            logical_id,
            default_role,
            default_access,
        })
    }
}

pub(super) async fn grant_role(
    conn: &mut Connection,
    scope: Scope,
    logical_id: Uuid,
) -> StorageResult<Option<String>> {
    sqlite_db::query_scalar(
        "SELECT role FROM oc_workspace_key_grants
         WHERE tenant_id=? AND logical_key_id=? AND workspace_id=?",
    )
    .bind(scope.tenant_id)
    .bind(logical_id)
    .bind(scope.workspace_id)
    .fetch_optional(conn)
    .await
    .map_err(sqlite_err)
}

async fn additional_grants(
    conn: &mut Connection,
    tenant_id: Uuid,
    logical_id: Uuid,
) -> StorageResult<Vec<(Uuid, String)>> {
    sqlite_db::query_as(
        "SELECT workspace_id,role FROM oc_workspace_key_grants
         WHERE tenant_id=? AND logical_key_id=? ORDER BY workspace_id",
    )
    .bind(tenant_id)
    .bind(logical_id)
    .fetch_all(conn)
    .await
    .map_err(sqlite_err)
}

impl SqliteStore {
    /// None means the credential is valid but the selected scope is unavailable.
    pub async fn authenticate_workspace(
        &self,
        token: &str,
        requested: Option<Uuid>,
    ) -> StorageResult<Option<AuthorizedScope>> {
        let mut tx = self.pool.begin_with("BEGIN").await.map_err(sqlite_err)?;
        let key = LiveCredential::from_token(&mut tx, token).await?;
        let scope = Scope {
            tenant_id: key.tenant_id,
            workspace_id: requested.unwrap_or(key.default_workspace),
        };
        let role = if scope.workspace_id == key.default_workspace {
            key.default_access.then_some(key.default_role)
        } else {
            grant_role(&mut tx, scope, key.logical_id).await?
        };
        tx.commit().await.map_err(sqlite_err)?;
        Ok(role.map(|role| AuthorizedScope {
            scope,
            principal_id: key.principal_id,
            role,
        }))
    }

    /// List only the active default and named grants, within the key's tenant.
    pub async fn workspace_access(&self, token: &str) -> StorageResult<Vec<WorkspaceAccess>> {
        let mut tx = self.pool.begin_with("BEGIN").await.map_err(sqlite_err)?;
        let key = LiveCredential::from_token(&mut tx, token).await?;
        let mut grants = additional_grants(&mut tx, key.tenant_id, key.logical_id).await?;
        if key.default_access {
            grants.insert(0, (key.default_workspace, key.default_role));
        }
        let mut access = Vec::with_capacity(grants.len());
        for batch in grants.chunks(256) {
            let placeholders = vec!["?"; batch.len()].join(",");
            let sql = format!(
                "SELECT id,name FROM oc_workspaces WHERE tenant_id=? AND id IN ({placeholders})"
            );
            let mut query = sqlite_db::query_as(&sql).bind(key.tenant_id);
            for (id, _) in batch {
                query = query.bind(id);
            }
            let names: HashMap<Uuid, String> = query
                .fetch_all(&mut *tx)
                .await
                .map_err(sqlite_err)?
                .into_iter()
                .collect();
            for (id, role) in batch {
                let name = names.get(id).ok_or(StorageError::Forbidden)?;
                access.push(WorkspaceAccess {
                    workspace_id: *id,
                    name: name.clone(),
                    role: role.clone(),
                    is_default: *id == key.default_workspace,
                });
            }
        }
        tx.commit().await.map_err(sqlite_err)?;
        Ok(access)
    }
}

impl SqliteTx {
    /// Enumerate this workspace's current credentials without exposing token hashes.
    pub async fn list_issued_keys(&mut self, after: Uuid, limit: u32) -> StorageResult<Value> {
        self.check_permission(Permission::Delete).await?;
        let count = key_page_size(limit)?;
        let rows: Vec<(Uuid, Uuid, String, bool, bool, i64)> = sqlite_db::query_as(
            "SELECT id,logical_key_id,role,default_access,allow_owned_workspaces,created_at
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
        let items: Vec<_> = rows.into_iter().take(count).map(|(key_id, logical_key_id, role, default_access, allow_owned_workspaces, created_at_ms)| {
            json!({"key_id":key_id,"logical_key_id":logical_key_id,"role":role,"default_access":default_access,
                   "allow_owned_workspaces":allow_owned_workspaces,"created_at_ms":created_at_ms})
        }).collect();
        Ok(json!({"items":items,"next_after":next_after}))
    }

    /// List explicit incoming grants. A revoked key's grant remains visible as inactive.
    pub async fn list_workspace_key_grants(
        &mut self,
        after: Uuid,
        limit: u32,
    ) -> StorageResult<Value> {
        self.check_permission(Permission::Delete).await?;
        let count = key_page_size(limit)?;
        let rows: Vec<(Uuid, String)> = sqlite_db::query_as(
            "SELECT logical_key_id,role FROM oc_workspace_key_grants
             WHERE tenant_id=? AND workspace_id=? AND logical_key_id>?
             ORDER BY logical_key_id LIMIT ?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(after)
        .bind(i64::from(limit) + 1)
        .fetch_all(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        let next_after = (rows.len() > count).then(|| rows[count - 1].0);
        let shown = &rows[..rows.len().min(count)];
        let active: HashSet<Uuid> = if shown.is_empty() {
            HashSet::new()
        } else {
            let placeholders = vec!["?"; shown.len()].join(",");
            let sql = format!(
                "SELECT logical_key_id FROM oc_api_keys WHERE tenant_id=? AND NOT revoked AND logical_key_id IN ({placeholders})"
            );
            let mut query = sqlite_db::query_scalar(&sql).bind(self.scope.tenant_id);
            for (id, _) in shown {
                query = query.bind(*id);
            }
            query
                .fetch_all(&mut *self.tx)
                .await
                .map_err(sqlite_err)?
                .into_iter()
                .collect()
        };
        let items: Vec<_> = shown
            .iter()
            .map(|(id, role)| json!({"logical_key_id":id,"role":role,"active":active.contains(id)}))
            .collect();
        Ok(json!({"items":items,"next_after":next_after}))
    }

    /// Manage the logical key's access to this workspace under its current admin.
    pub async fn set_workspace_key_grant(
        &mut self,
        logical_id: Uuid,
        role: Option<&str>,
    ) -> StorageResult<()> {
        self.check_permission(Permission::Delete).await?;
        if role.is_some_and(|role| !ROLES.contains(&role)) {
            return Err(StorageError::InvalidArgument("invalid workspace role"));
        }
        let (credential, default_workspace, owned_capability): (Uuid, Uuid, bool) =
            sqlite_db::query_as(
                "SELECT id,workspace_id,allow_owned_workspaces FROM oc_api_keys
                 WHERE tenant_id=? AND logical_key_id=? AND NOT revoked",
            )
            .bind(self.scope.tenant_id)
            .bind(logical_id)
            .fetch_optional(&mut *self.tx)
            .await
            .map_err(sqlite_err)?
            .ok_or(StorageError::NotFound)?;
        if self.scope.workspace_id == default_workspace {
            sqlite_db::query(
                "UPDATE oc_api_keys SET role=COALESCE(?,role),default_access=?
                 WHERE tenant_id=? AND id=?",
            )
            .bind(role)
            .bind(role.is_some())
            .bind(self.scope.tenant_id)
            .bind(credential)
            .execute(&mut *self.tx)
            .await
            .map_err(sqlite_err)?;
        } else if owned_capability {
            return Err(StorageError::Forbidden);
        } else if let Some(role) = role {
            sqlite_db::query(
                "INSERT INTO oc_workspace_key_grants(tenant_id,workspace_id,logical_key_id,role)
                 VALUES(?,?,?,?) ON CONFLICT(tenant_id,logical_key_id,workspace_id)
                 DO UPDATE SET role=excluded.role",
            )
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .bind(logical_id)
            .bind(role)
            .execute(&mut *self.tx)
            .await
            .map_err(sqlite_err)?;
        } else {
            sqlite_db::query(
                "DELETE FROM oc_workspace_key_grants
                 WHERE tenant_id=? AND logical_key_id=? AND workspace_id=?",
            )
            .bind(self.scope.tenant_id)
            .bind(logical_id)
            .bind(self.scope.workspace_id)
            .execute(&mut *self.tx)
            .await
            .map_err(sqlite_err)?;
        }
        self.audit("key.workspace_grant", logical_id, json!({"role":role}))
            .await
    }

    /// Rotating another key discloses a replacement token. Home admin rights
    /// alone cannot disclose a credential with grants in other workspaces.
    pub(super) async fn authorize_granted_key_rotation(
        &mut self,
        credential_id: Uuid,
        logical_id: Uuid,
    ) -> StorageResult<()> {
        if credential_id == self.principal_id {
            return Ok(());
        }
        let grants = additional_grants(&mut self.tx, self.scope.tenant_id, logical_id).await?;
        for (workspace_id, _) in grants {
            let scope = Scope {
                tenant_id: self.scope.tenant_id,
                workspace_id,
            };
            let role = self.live_role_in_scope(self.principal_id, scope).await?;
            if !Permission::Delete.granted_by(&role) {
                return Err(StorageError::Forbidden);
            }
        }
        Ok(())
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
