//! Explicit deployment-wide inspection and moderation authority.
use super::*;

fn admin_actor_id(token_hash: &str) -> Uuid {
    // Administrator receipts use a distinct UUID version from workspace keys.
    // Derive the actor from the verified fingerprint without storing the token.
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    digest.update(b"origence:administrator:v1:");
    digest.update(token_hash);
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest.finalize()[..16]);
    uuid::Builder::from_custom_bytes(bytes).into_uuid()
}

pub(super) async fn require_admin(conn: &mut Connection, token: &str) -> StorageResult<Uuid> {
    if token.len() != 73 || !token.starts_with("oc_admin_") {
        return Err(StorageError::Unauthorized);
    }
    let fingerprint = hash(token);
    let exists: bool = sqlite_db::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM oc_deployment_admin WHERE singleton=1 AND token_hash=?)",
    )
    .bind(&fingerprint)
    .fetch_one(conn)
    .await
    .map_err(sqlite_err)?;
    if !exists {
        return Err(StorageError::Unauthorized);
    }
    Ok(admin_actor_id(&fingerprint))
}

pub(super) async fn live_admin_in_scope(
    conn: &mut Connection,
    actor: Uuid,
    scope: Scope,
) -> StorageResult<bool> {
    if actor.get_version_num() != 8 {
        return Ok(false);
    }
    let fingerprint: Option<String> =
        sqlite_db::query_scalar("SELECT token_hash FROM oc_deployment_admin WHERE singleton=1")
            .fetch_optional(&mut *conn)
            .await
            .map_err(sqlite_err)?;
    if !fingerprint.is_some_and(|hash| admin_actor_id(&hash) == actor) {
        return Ok(false);
    }
    sqlite_db::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_workspaces WHERE tenant_id=? AND id=?)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .fetch_one(conn)
        .await
        .map_err(sqlite_err)
}

impl SqliteStore {
    #[cfg(feature = "host")]
    pub(crate) async fn has_deployment_admin(&self) -> StorageResult<bool> {
        sqlite_db::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM oc_deployment_admin WHERE singleton=1)",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(sqlite_err)
    }

    #[cfg(feature = "host")]
    pub(crate) async fn initialize_deployment_admin(&self, token: &str) -> StorageResult<()> {
        sqlite_db::query(
            "INSERT INTO oc_deployment_admin(singleton,token_hash) VALUES(1,?)
             ON CONFLICT(singleton) DO NOTHING",
        )
        .bind(hash(token))
        .execute(&self.pool)
        .await
        .map_err(sqlite_err)?;
        self.authenticate_deployment_admin(token).await
    }

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
        require_admin(&mut conn, token).await.map(|_| ())
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

    /// Begin a read transaction for the selected workspace.
    pub(crate) async fn begin_deployment_inspection(
        &self,
        token: &str,
        workspace: Uuid,
    ) -> StorageResult<SqliteTx> {
        self.deployment_workspace_tx(token, workspace, false).await
    }

    /// Authenticate the instance administrator for the selected workspace.
    pub async fn authorize_deployment_workspace(
        &self,
        token: &str,
        workspace: Uuid,
    ) -> StorageResult<AuthorizedScope> {
        let tx = self.begin_deployment_inspection(token, workspace).await?;
        let auth = AuthorizedScope {
            scope: tx.scope,
            principal_id: tx.principal_id,
            role: "admin".into(),
        };
        tx.commit().await?;
        Ok(auth)
    }

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
        let actor = require_admin(&mut tx, token).await?;
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
            principal_id: actor,
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
        let actor = require_admin(&mut tx, token).await?;
        let previous: Option<(Uuid, String)> =
            sqlite_db::query_as("SELECT tenant_id,name FROM oc_workspaces WHERE id=?")
                .bind(workspace)
                .fetch_optional(&mut *tx)
                .await
                .map_err(sqlite_err)?;
        if let Some((tenant, saved_name)) = previous {
            if tenant != workspace || saved_name != name.trim() {
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
            principal_id: actor,
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
    pub(crate) async fn deployment_workspace_key(
        &self,
        token: &str,
        workspace: Uuid,
    ) -> StorageResult<Value> {
        let mut tx = self.begin_deployment_inspection(token, workspace).await?;
        let row: Option<(Uuid, i64)> = sqlite_db::query_as(
            "SELECT id,created_at FROM oc_api_keys
             WHERE tenant_id=? AND workspace_id=? AND role='workspace' AND NOT revoked",
        )
        .bind(tx.scope.tenant_id)
        .bind(tx.scope.workspace_id)
        .fetch_optional(&mut *tx.tx)
        .await
        .map_err(sqlite_err)?;
        tx.commit().await?;
        let key = row
            .map(|(key_id, created_at_ms)| json!({"key_id":key_id,"created_at_ms":created_at_ms}));
        Ok(json!({"key":key}))
    }

    #[cfg(feature = "host")]
    pub(crate) async fn rotate_deployment_workspace_key(
        &self,
        token: &str,
        workspace: Uuid,
    ) -> StorageResult<IssuedKey> {
        let mut tx = self.deployment_workspace_tx(token, workspace, true).await?;
        let key = super::workspace_key::rotate_single_workspace_key(&mut tx.tx, tx.scope).await?;
        tx.audit("deployment.workspace.key.rotate", key.key_id, json!({}))
            .await?;
        tx.commit().await?;
        Ok(key)
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
