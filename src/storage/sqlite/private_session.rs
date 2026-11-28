//! Private archives are bound to logical keys, not caller-selected session IDs.
use super::*;

impl SqliteTx {
    /// Stable identity survives only an explicitly authorized credential rotation.
    pub async fn private_key_id(&mut self) -> StorageResult<Uuid> {
        self.check_permission(Permission::Read).await?;
        sqlite_db::query_scalar(
            "SELECT logical_key_id FROM oc_api_keys WHERE tenant_id=? AND id=? AND NOT revoked",
        )
        .bind(self.scope.tenant_id)
        .bind(self.principal_id)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(sqlite_err)?
        .ok_or(StorageError::Forbidden)
    }
    /// Read the authenticated key's binding without creating a session.
    pub async fn bound_private_session(&mut self) -> StorageResult<Option<Uuid>> {
        let owner = self.private_key_id().await?;
        self.private_session_for(owner).await
    }
    pub(super) async fn private_session_for(&mut self, owner: Uuid) -> StorageResult<Option<Uuid>> {
        sqlite_db::query_scalar("SELECT session_id FROM oc_key_sessions WHERE tenant_id=? AND workspace_id=? AND logical_key_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(owner).fetch_optional(&mut *self.tx).await.map_err(sqlite_err)
    }
    /// Resolve or create the server-assigned session in a write transaction.
    pub async fn private_session_id(&mut self) -> StorageResult<Uuid> {
        let owner = self.private_key_id().await?;
        if let Some(id) = self.private_session_for(owner).await? {
            return Ok(id);
        }
        let id = Uuid::new_v4();
        sqlite_db::query("INSERT INTO oc_key_sessions(tenant_id,workspace_id,logical_key_id,session_id) VALUES(?,?,?,?)")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(owner).bind(id).execute(&mut *self.tx).await.map_err(sqlite_err)?;
        Ok(id)
    }
    /// Workspace membership never confers another key's private session access.
    pub async fn require_private_session(&mut self, session: Uuid) -> StorageResult<()> {
        self.require_private_session_owner(session)
            .await
            .map(|_| ())
    }
    pub(super) async fn require_private_session_owner(
        &mut self,
        session: Uuid,
    ) -> StorageResult<Uuid> {
        let owner = self.private_key_id().await?;
        if self.private_session_for(owner).await? != Some(session) {
            return Err(StorageError::NotFound);
        }
        Ok(owner)
    }
}

impl SqliteTx {
    /// Guard indirect raw-source access before content or locators leave storage.
    pub(super) async fn require_private_source(&mut self, source: Uuid) -> StorageResult<()> {
        let session:Option<Uuid>=sqlite_db::query_scalar("SELECT session_id FROM oc_session_turns WHERE tenant_id=? AND workspace_id=? AND source_event_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(source).fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        if let Some(session) = session {
            self.require_private_session(session).await?;
        }
        let learning_session: Option<Uuid> = sqlite_db::query_scalar("SELECT session_id FROM oc_learnings WHERE tenant_id=? AND workspace_id=? AND source_event_id=? AND session_id IS NOT NULL")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(source).fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        if let Some(session) = learning_session {
            self.require_private_session(session).await?;
        }
        Ok(())
    }
    /// Improve job details include private frozen turns and must retain their scope.
    pub(super) async fn require_private_job(&mut self, job: Uuid) -> StorageResult<()> {
        let session:Option<Uuid>=sqlite_db::query_scalar("SELECT session_id FROM oc_improve_runs WHERE tenant_id=? AND workspace_id=? AND job_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(job).fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        if let Some(session) = session {
            self.require_private_session(session).await?;
        }
        let session:Option<Uuid>=sqlite_db::query_scalar("SELECT session_id FROM oc_learnings WHERE tenant_id=? AND workspace_id=? AND ingest_job_id=? AND session_id IS NOT NULL")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(job).fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        if let Some(session) = session {
            self.require_private_session(session).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionTurnInput;
    use crate::storage::{DomainTx, RelationalStore};

    #[tokio::test]
    async fn private_archive_isolation_and_authorized_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(dir.path().join("private.db"))
            .await
            .unwrap();
        let scope = Scope {
            tenant_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
        };
        let first = store.create_key_workspace(scope, "private").await.unwrap();
        let auth = store.authenticate(&first.token).await.unwrap();
        let mut tx = store.begin(auth.clone()).await.unwrap();
        let session = tx.private_session_id().await.unwrap();
        let logical = tx.private_key_id().await.unwrap();
        assert_eq!(session, tx.private_session_id().await.unwrap());
        let turn = SessionTurnInput {
            turn_id: Uuid::new_v4(),
            question: "private question".into(),
            answer: "private answer".into(),
            used_evidence: vec![],
            evidence_use: Default::default(),
        };
        let (source, _) = tx.register_turn(session, &turn, "hash").await.unwrap();
        tx.commit().await.unwrap();
        let foreign_scope = Scope {
            tenant_id: scope.tenant_id,
            workspace_id: Uuid::new_v4(),
        };
        let foreign = store
            .create_key_workspace(foreign_scope, "other")
            .await
            .unwrap();
        let other_auth = store.authenticate(&foreign.token).await.unwrap();
        let mut forged = auth.clone();
        forged.scope = foreign_scope;
        assert!(matches!(
            store.begin(forged).await,
            Err(StorageError::Forbidden)
        ));
        let mut tx = store.begin(other_auth).await.unwrap();
        assert_ne!(session, tx.private_session_id().await.unwrap());
        assert!(matches!(
            tx.require_turn_visible(session, turn.turn_id).await,
            Err(StorageError::NotFound)
        ));
        assert!(matches!(
            tx.register_turn(session, &turn, "hash").await,
            Err(StorageError::NotFound)
        ));
        assert_eq!(tx.event_content_inner(source, true).await.unwrap(), None);
        assert!(matches!(
            tx.event_file_inner(source).await,
            Err(StorageError::NotFound)
        ));
        assert_eq!(tx.retract_event_inner(source).await.unwrap(), 0);
        tx.commit().await.unwrap();
        let admin = store.rotate_deployment_admin().await.unwrap();
        let admin_auth = store
            .authorize_deployment_workspace(&admin, scope.workspace_id)
            .await
            .unwrap();
        let mut tx = store.begin(admin_auth).await.unwrap();
        assert!(matches!(
            tx.require_private_session(session).await,
            Err(StorageError::Forbidden)
        ));
        tx.rollback().await.unwrap();
        let replacement = store.rotate_workspace_key(scope).await.unwrap();
        assert!(store.authenticate(&first.token).await.is_err());
        assert!(matches!(
            store.begin(auth.clone()).await,
            Err(StorageError::Forbidden)
        ));
        let new_auth = store.authenticate(&replacement.token).await.unwrap();
        let mut tx = store.begin(new_auth).await.unwrap();
        assert_eq!(logical, tx.private_key_id().await.unwrap());
        assert_eq!(session, tx.private_session_id().await.unwrap());
        assert!(tx.require_turn_visible(session, turn.turn_id).await.is_ok());
        tx.commit().await.unwrap();
        assert_eq!(
            store.authenticate(&foreign.token).await.unwrap().scope,
            foreign_scope
        );
    }
}
