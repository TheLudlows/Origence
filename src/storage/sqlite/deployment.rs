//! Explicit deployment-wide inspection and moderation authority.
use super::*;

#[cfg(feature = "host")]
#[derive(Debug)]
pub enum DeploymentKeyOperation {
    List {
        after: Uuid,
        limit: u32,
    },
    ListGrants {
        after: Uuid,
        limit: u32,
    },
    Issue {
        role: String,
    },
    Rotate(Uuid),
    RotateSingle,
    Revoke(Uuid),
    SetGrant {
        logical_id: Uuid,
        role: Option<String>,
    },
}

#[cfg(feature = "host")]
pub(super) async fn require_admin(conn: &mut Connection, token: &str) -> StorageResult<()> {
    if token.len() != 73 || !token.starts_with("oc_admin_") {
        return Err(StorageError::Unauthorized);
    }
    let exists: bool = sqlite_db::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM oc_deployment_admin WHERE singleton=1 AND token_hash=?)",
    )
    .bind(hash(token))
    .fetch_one(conn)
    .await
    .map_err(sqlite_err)?;
    if !exists {
        return Err(StorageError::Unauthorized);
    }
    Ok(())
}

impl SqliteStore {
    /// Offline operator provisioning. Replacement immediately revokes the previous token.
    pub async fn rotate_deployment_admin(&self) -> StorageResult<String> {
        let token = format!(
            "oc_admin_{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        sqlite_db::query(
            "INSERT INTO oc_deployment_admin(singleton,token_hash) VALUES(1,?)
             ON CONFLICT(singleton) DO UPDATE SET token_hash=excluded.token_hash",
        )
        .bind(hash(&token))
        .execute(&self.pool)
        .await
        .map_err(sqlite_err)?;
        Ok(token)
    }

    #[cfg(feature = "host")]
    pub(crate) async fn authenticate_deployment_admin(&self, token: &str) -> StorageResult<()> {
        let mut conn = self.pool.acquire().await.map_err(sqlite_err)?;
        require_admin(&mut conn, token).await
    }

    #[cfg(feature = "host")]
    pub(crate) async fn deployment_workspaces(
        &self,
        token: &str,
        after: Uuid,
        limit: usize,
    ) -> StorageResult<Value> {
        let mut tx = self.pool.begin().await.map_err(sqlite_err)?;
        require_admin(&mut tx, token).await?;
        if !(1..=100).contains(&limit) {
            return Err(StorageError::InvalidArgument("invalid workspace page"));
        }
        let mut rows: Vec<(Uuid, String)> =
            sqlite_db::query_as("SELECT id,name FROM oc_workspaces WHERE id>? ORDER BY id LIMIT ?")
                .bind(after)
                .bind((limit + 1) as i64)
                .fetch_all(&mut *tx)
                .await
                .map_err(sqlite_err)?;
        tx.commit().await.map_err(sqlite_err)?;
        let more = rows.len() > limit;
        rows.truncate(limit);
        let next_after = more.then(|| rows.last().expect("nonempty page").0);
        let items: Vec<_> = rows
            .into_iter()
            .map(|(id, name)| json!({"workspace_id":id,"name":name}))
            .collect();
        Ok(json!({"items":items,"next_after":next_after}))
    }

    /// Internal inspection context. The nil principal has no regular write or ownership rights.
    #[cfg(feature = "host")]
    pub(crate) async fn begin_deployment_inspection(
        &self,
        token: &str,
        workspace: Uuid,
    ) -> StorageResult<SqliteTx> {
        self.deployment_workspace_tx(token, workspace, false).await
    }

    #[cfg(feature = "host")]
    async fn deployment_workspace_tx(
        &self,
        token: &str,
        workspace: Uuid,
        write: bool,
    ) -> StorageResult<SqliteTx> {
        let mut tx = self
            .pool
            .begin_with(if write { "BEGIN IMMEDIATE" } else { "BEGIN" })
            .await
            .map_err(sqlite_err)?;
        require_admin(&mut tx, token).await?;
        let tenant = sqlite_db::query_scalar("SELECT tenant_id FROM oc_workspaces WHERE id=?")
            .bind(workspace)
            .fetch_optional(&mut *tx)
            .await
            .map_err(sqlite_err)?
            .ok_or(StorageError::NotFound)?;
        Ok(SqliteTx {
            tx,
            scope: Scope {
                tenant_id: tenant,
                workspace_id: workspace,
            },
            principal_id: Uuid::nil(),
            required: Vec::new(),
            command: None,
        })
    }

    #[cfg(feature = "host")]
    pub(crate) async fn provision_deployment_workspace(
        &self,
        token: &str,
        workspace: Uuid,
        name: &str,
    ) -> StorageResult<Value> {
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(sqlite_err)?;
        require_admin(&mut tx, token).await?;
        let previous: Option<(Uuid, String, Option<Uuid>)> = sqlite_db::query_as(
            "SELECT tenant_id,name,owner_workspace_id FROM oc_workspaces WHERE id=?",
        )
        .bind(workspace)
        .fetch_optional(&mut *tx)
        .await
        .map_err(sqlite_err)?;
        if let Some((tenant, saved_name, parent)) = previous {
            if tenant != workspace || parent.is_some() || saved_name != name.trim() {
                return Err(StorageError::Conflict("workspace ID already exists".into()));
            }
            tx.commit().await.map_err(sqlite_err)?;
            return Ok(json!({"workspace_id":workspace,"name":saved_name,"created":false}));
        }
        let scope = Scope {
            tenant_id: workspace,
            workspace_id: workspace,
        };
        let key = super::workspace_key::create_single_key_workspace(&mut tx, scope, name).await?;
        let mut scoped = SqliteTx {
            tx,
            scope,
            principal_id: Uuid::nil(),
            required: Vec::new(),
            command: None,
        };
        scoped
            .audit(
                "deployment.workspace.create",
                workspace,
                json!({"key_id":key.key_id}),
            )
            .await?;
        scoped.commit().await?;
        Ok(json!({"workspace_id":workspace,"name":name.trim(),"created":true,"key":key}))
    }

    #[cfg(feature = "host")]
    pub(crate) async fn administer_deployment_keys(
        &self,
        token: &str,
        workspace: Uuid,
        operation: DeploymentKeyOperation,
    ) -> StorageResult<Value> {
        use DeploymentKeyOperation::*;
        let write = !matches!(operation, List { .. } | ListGrants { .. });
        let mut tx = self
            .deployment_workspace_tx(token, workspace, write)
            .await?;
        let scope = tx.scope;
        let single_key = super::workspace_key::has_workspace_key(&mut tx.tx, scope).await?;
        if single_key && !matches!(operation, List { .. } | RotateSingle) {
            return Err(StorageError::Conflict(
                "this workspace uses a single key".into(),
            ));
        }
        let result = match operation {
            List { after, limit } => {
                let mut page = tx.list_issued_keys_inner(after, limit).await?;
                page["single_key"] = json!(single_key);
                page
            }
            ListGrants { after, limit } => tx.list_workspace_key_grants_inner(after, limit).await?,
            Issue { role } => json!(
                tx.issue_scoped_key_inner(&role, KeyCapabilities::default())
                    .await?
            ),
            Rotate(id) => json!(tx.rotate_scoped_key_inner(id, false).await?),
            RotateSingle => {
                let key =
                    super::workspace_key::rotate_single_workspace_key(&mut tx.tx, scope).await?;
                tx.audit("deployment.workspace.key.rotate", key.key_id, json!({}))
                    .await?;
                json!(key)
            }
            Revoke(id) => {
                tx.revoke_scoped_key_inner(id).await?;
                json!({"key_id":id,"revoked":true})
            }
            SetGrant { logical_id, role } => {
                tx.set_workspace_key_grant_inner(logical_id, role.as_deref())
                    .await?;
                json!({"workspace_id":workspace,"logical_key_id":logical_id,"role":role})
            }
        };
        tx.commit().await?;
        Ok(result)
    }
}

#[cfg(feature = "host")]
impl SqliteTx {
    pub(crate) async fn inspection_sessions(
        &mut self,
        after: Uuid,
        limit: usize,
    ) -> StorageResult<Value> {
        if !(1..=100).contains(&limit) {
            return Err(StorageError::InvalidArgument(
                "invalid archive session page",
            ));
        }
        let mut sessions: Vec<Uuid> = sqlite_db::query_scalar(
            "SELECT session_id FROM oc_key_sessions WHERE tenant_id=? AND workspace_id=? AND session_id>? ORDER BY session_id LIMIT ?",
        ).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(after).bind((limit+1) as i64)
        .fetch_all(&mut *self.tx).await.map_err(sqlite_err)?;
        let more = sessions.len() > limit;
        sessions.truncate(limit);
        let next_after = more.then(|| *sessions.last().expect("nonempty page"));
        Ok(json!({"items":sessions,"next_after":next_after}))
    }

    pub(crate) async fn inspection_archive_domain(
        &mut self,
        session: Uuid,
    ) -> StorageResult<crate::session::ArchiveDomain> {
        let owner = sqlite_db::query_scalar(
            "SELECT logical_key_id FROM oc_key_sessions WHERE tenant_id=? AND workspace_id=? AND session_id=?",
        ).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session)
        .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?.ok_or(StorageError::NotFound)?;
        Ok(crate::session::ArchiveDomain {
            scope: self.scope,
            owner,
        })
    }
}
