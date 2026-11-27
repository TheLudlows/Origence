//! Explicit deployment-wide inspection and moderation authority.
use super::*;

#[cfg(feature = "local-storage")]
pub(super) async fn require_admin(conn: &mut Connection, token: &str) -> StorageResult<()> {
    if token.len() != 73 || !token.starts_with("oc_admin_") {
        return Err(StorageError::Forbidden);
    }
    let exists: bool = sqlite_db::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM oc_deployment_admin WHERE singleton=1 AND token_hash=?)",
    )
    .bind(hash(token))
    .fetch_one(conn)
    .await
    .map_err(sqlite_err)?;
    if !exists {
        return Err(StorageError::Forbidden);
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

    #[cfg(feature = "local-storage")]
    pub(crate) async fn authenticate_deployment_admin(&self, token: &str) -> StorageResult<()> {
        let mut conn = self.pool.acquire().await.map_err(sqlite_err)?;
        require_admin(&mut conn, token).await
    }

    #[cfg(feature = "local-storage")]
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
    #[cfg(feature = "local-storage")]
    pub(crate) async fn begin_deployment_inspection(
        &self,
        token: &str,
        workspace: Uuid,
    ) -> StorageResult<SqliteTx> {
        let mut tx = self.pool.begin().await.map_err(sqlite_err)?;
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
}

#[cfg(feature = "local-storage")]
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
