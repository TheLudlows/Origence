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
    async fn private_session_for(&mut self, owner: Uuid) -> StorageResult<Option<Uuid>> {
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
    /// Rotate one live credential, retaining its logical identity and all archives.
    pub async fn rotate_scoped_key(&mut self, id: Uuid) -> StorageResult<IssuedKey> {
        self.check_permission(Permission::Delete).await?;
        let old:Option<(Uuid,String,bool,bool)>=sqlite_db::query_as("SELECT logical_key_id,role,allow_owned_workspaces,default_access FROM oc_api_keys WHERE tenant_id=? AND workspace_id=? AND id=? AND NOT revoked")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(id).fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        let (logical, role, capability, default_access) = old.ok_or(StorageError::NotFound)?;
        self.authorize_granted_key_rotation(id, logical).await?;
        // A key may rotate itself only under this transaction's already-verified admin role.
        let key_id = Uuid::new_v4();
        let token = format!("oc_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        sqlite_db::query("INSERT INTO oc_api_keys(id,logical_key_id,tenant_id,workspace_id,token_hash,role,allow_owned_workspaces,default_access,created_at) VALUES(?,?,?,?,?,?,?,?,?)")
            .bind(key_id).bind(logical).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(hash(&token)).bind(role).bind(capability).bind(default_access).bind(now_ms()).execute(&mut *self.tx).await.map_err(sqlite_err)?;
        self.revoke_scoped_key(id).await?;
        self.audit(
            "key.rotate",
            id,
            json!({"replacement_key_id":key_id,"logical_key_id":logical}),
        )
        .await?;
        Ok(IssuedKey { key_id, token })
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
        store
            .create_workspace(scope.tenant_id, scope.workspace_id, "private")
            .await
            .unwrap();
        let admin = store.issue_key(scope, "admin").await.unwrap();
        let first = store.issue_key(scope, "writer").await.unwrap();
        let other = store.issue_key(scope, "writer").await.unwrap();
        let auth = store.authenticate(&first.token).await.unwrap();
        let other_auth = store.authenticate(&other.token).await.unwrap();
        let admin_auth = store.authenticate(&admin.token).await.unwrap();
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
        store
            .create_workspace(foreign_scope.tenant_id, foreign_scope.workspace_id, "other")
            .await
            .unwrap();
        let foreign_key = store.issue_key(foreign_scope, "admin").await.unwrap();
        let foreign_auth = store.authenticate(&foreign_key.token).await.unwrap();
        let mut foreign_tx = store.begin(foreign_auth).await.unwrap();
        assert!(matches!(
            foreign_tx.require_turn_visible(session, turn.turn_id).await,
            Err(StorageError::NotFound)
        ));
        foreign_tx.commit().await.unwrap();
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
        assert!(matches!(
            tx.event_content_inner(source, true).await,
            Err(StorageError::NotFound)
        ));
        assert!(matches!(
            tx.event_file_inner(source).await,
            Err(StorageError::NotFound)
        ));
        assert!(matches!(
            tx.retract_event_inner(source).await,
            Err(StorageError::NotFound)
        ));
        assert!(matches!(
            tx.rotate_scoped_key(first.key_id).await,
            Err(StorageError::Forbidden)
        ));
        tx.commit().await.unwrap();
        let mut tx = store.begin(admin_auth).await.unwrap();
        assert!(matches!(
            tx.require_private_session(session).await,
            Err(StorageError::NotFound)
        ));
        let replacement = tx.rotate_scoped_key(first.key_id).await.unwrap();
        tx.commit().await.unwrap();
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
        let mut tx = store
            .begin(store.authenticate(&admin.token).await.unwrap())
            .await
            .unwrap();
        let admin_session = tx.private_session_id().await.unwrap();
        let rotated_admin = tx.rotate_scoped_key(admin.key_id).await.unwrap();
        tx.commit().await.unwrap();
        assert!(store.authenticate(&admin.token).await.is_err());
        let mut tx = store
            .begin(store.authenticate(&rotated_admin.token).await.unwrap())
            .await
            .unwrap();
        assert_eq!(tx.private_session_id().await.unwrap(), admin_session);
        tx.revoke_scoped_key(replacement.key_id).await.unwrap();
        tx.commit().await.unwrap();
        assert!(store.authenticate(&replacement.token).await.is_err());
    }
}
