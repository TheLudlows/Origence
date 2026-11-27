//! SQLite holds session coordination, used dependency proofs and withdrawal fences.
use super::*;

pub(super) const SCHEMA: &str = include_str!("../native-archive-schema.sql");

impl SqliteTx {
    pub(super) async fn archive_proof_live(
        &mut self,
        session: Uuid,
        turn: Uuid,
    ) -> StorageResult<bool> {
        let proof = self.native_archive_proof_metadata(session, turn).await?;
        if proof.is_none() {
            return Ok(false);
        }
        Ok(!self.archive_session_withdrawn(session).await?
            && !self.native_turn_withdrawn(session, turn).await?)
    }

    pub(super) async fn archive_body_metadata(
        &mut self,
        session: Uuid,
        turn: Uuid,
    ) -> StorageResult<(bool, usize)> {
        let (reported, bytes) = self
            .native_archive_proof_metadata(session, turn)
            .await?
            .ok_or(StorageError::NotFound)?;
        let bytes = usize::try_from(bytes)
            .map_err(|_| StorageError::Unavailable("invalid private archive body size".into()))?;
        Ok((reported, bytes))
    }

    async fn native_archive_proof_metadata(
        &mut self,
        session: Uuid,
        turn: Uuid,
    ) -> StorageResult<Option<(bool, i64)>> {
        sqlite_db::query_as("SELECT evidence_reported,body_bytes FROM oc_native_archive_proofs WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)
    }

    async fn archive_session_withdrawn(&mut self, session: Uuid) -> StorageResult<bool> {
        sqlite_db::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_withdrawn_private_sessions WHERE tenant_id=? AND workspace_id=? AND session_id=?)")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session)
            .fetch_one(&mut *self.tx).await.map_err(sqlite_err)
    }

    async fn native_turn_withdrawn(&mut self, session: Uuid, turn: Uuid) -> StorageResult<bool> {
        sqlite_db::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_native_archive_withdrawals WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=?)")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
            .fetch_one(&mut *self.tx).await.map_err(sqlite_err)
    }

    async fn fence_native_archive(&mut self, session: Uuid, turn: Uuid) -> StorageResult<()> {
        sqlite_db::query("INSERT INTO oc_native_archive_withdrawals(tenant_id,workspace_id,session_id,turn_id,cleanup_after) VALUES(?,?,?,?,?) ON CONFLICT DO NOTHING")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn).bind(now_ms())
            .execute(&mut *self.tx).await.map_err(sqlite_err)?;
        Ok(())
    }

    pub(crate) async fn native_archive_turn_for_source(
        &mut self,
        source: Uuid,
    ) -> StorageResult<Option<(Uuid, Uuid)>> {
        let turn: Option<(Uuid, Uuid)> = sqlite_db::query_as("SELECT session_id,turn_id FROM oc_session_turns WHERE tenant_id=? AND workspace_id=? AND source_event_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(source)
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        if let Some((session, turn)) = turn
            && self
                .native_archive_proof_metadata(session, turn)
                .await?
                .is_some()
        {
            self.require_private_session(session).await?;
            return Ok(Some((session, turn)));
        }
        Ok(None)
    }

    pub(super) async fn withdraw_archive_source(&mut self, source: Uuid) -> StorageResult<()> {
        if let Some((session, turn)) = self.native_archive_turn_for_source(source).await? {
            self.fence_native_archive(session, turn).await?;
        }
        Ok(())
    }
}

#[cfg(feature = "local-storage")]
mod host {
    use super::*;
    use crate::{session::ArchiveDomain, storage::anyseek::private_archive::NativeArchiveTurn};

    fn corrupt() -> StorageError {
        StorageError::Unavailable("invalid private archive metadata".into())
    }

    impl SqliteTx {
        pub(crate) async fn archive_domain(
            &mut self,
            session: Uuid,
        ) -> StorageResult<ArchiveDomain> {
            let owner = self.require_private_session_owner(session).await?;
            if self.archive_session_withdrawn(session).await? {
                return Err(StorageError::NotFound);
            }
            Ok(ArchiveDomain {
                scope: self.scope,
                owner,
            })
        }

        pub(crate) async fn native_archive_initialized(
            &mut self,
            session: Uuid,
        ) -> StorageResult<bool> {
            sqlite_db::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_native_archive_sessions WHERE tenant_id=? AND workspace_id=? AND session_id=?)")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session)
                .fetch_one(&mut *self.tx).await.map_err(sqlite_err)
        }

        pub(crate) async fn native_archive_proof_digest(
            &mut self,
            session: Uuid,
            turn: Uuid,
        ) -> StorageResult<Option<(Uuid, String)>> {
            if self
                .native_archive_proof_metadata(session, turn)
                .await?
                .is_none()
            {
                return Ok(None);
            }
            let proof = sqlite_db::query_as("SELECT source_event_id,request_hash FROM oc_session_turns WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=?")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
                .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?
                .ok_or_else(corrupt)?;
            Ok(Some(proof))
        }

        pub(crate) async fn native_archive_request_exists(
            &mut self,
            session: Uuid,
            turn: Uuid,
        ) -> StorageResult<bool> {
            self.check_permission(Permission::Write).await?;
            self.require_private_session(session).await?;
            // The session is bound to the logical owner, so a replacement
            // credential must also be able to withdraw its predecessor's turn.
            sqlite_db::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_commands WHERE tenant_id=? AND workspace_id=? AND operation='session.turn.archive' AND CASE WHEN json_valid(response) THEN (json_extract(response,'$.session_id')=? AND json_extract(response,'$.turn_id')=?) ELSE 0 END)")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id)
                .bind(session.to_string()).bind(turn.to_string())
                .fetch_one(&mut *self.tx).await.map_err(sqlite_err)
        }

        /// Preserve the integer paging cursor without storing a row for each turn.
        pub(crate) async fn allocate_native_archive_sequence(
            &mut self,
            session: Uuid,
        ) -> StorageResult<i64> {
            self.check_permission(Permission::Write).await?;
            self.archive_domain(session).await?;
            let current: Option<i64> = sqlite_db::query_scalar("SELECT last_sequence FROM oc_native_archive_sessions WHERE tenant_id=? AND workspace_id=? AND session_id=?")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session)
                .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
            let sequence = current.unwrap_or(0).checked_add(1).ok_or_else(corrupt)?;
            sqlite_db::query("INSERT INTO oc_native_archive_sessions(tenant_id,workspace_id,session_id,last_sequence) VALUES(?,?,?,?) ON CONFLICT DO UPDATE SET last_sequence=excluded.last_sequence")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(sequence)
                .execute(&mut *self.tx).await.map_err(sqlite_err)?;
            Ok(sequence)
        }

        pub(crate) async fn require_native_archive_live(
            &mut self,
            session: Uuid,
            turn: Uuid,
        ) -> StorageResult<ArchiveDomain> {
            self.check_permission(Permission::Write).await?;
            let domain = self.archive_domain(session).await?;
            if self.native_turn_withdrawn(session, turn).await? {
                return Err(StorageError::NotFound);
            }
            Ok(domain)
        }

        /// A durable consumer may keep the exact support it used for transactional
        /// rechecks. Plain saves, reads, and searches never create such proof.
        pub(crate) async fn attach_native_archive_proof(
            &mut self,
            record: &NativeArchiveTurn,
        ) -> StorageResult<()> {
            let (session, turn) = (record.session, record.turn);
            self.require_native_archive_live(session, turn).await?;
            let metadata = (
                record.body.evidence_use.is_reported(),
                i64::try_from(record.body.model_json_bytes()).map_err(|_| corrupt())?,
            );
            let existing: Option<(Uuid, String, Uuid, i64)> = sqlite_db::query_as("SELECT source_event_id,request_hash,created_by,created_at FROM oc_session_turns WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=?")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
                .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
            if let Some((source, digest, creator, created_at)) = existing {
                if source != record.source
                    || digest != record.request_hash
                    || creator != record.created_by
                    || created_at != record.created_at
                    || self.native_archive_proof_metadata(session, turn).await? != Some(metadata)
                    || self.turn_evidence(session, turn).await? != record.evidence
                {
                    return Err(corrupt());
                }
                return Ok(());
            }
            sqlite_db::query("INSERT INTO oc_events(tenant_id,workspace_id,id,content,kind,created_by,created_at) VALUES(?,?,?,'','session_turn',?,?)")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(record.source)
                .bind(record.created_by).bind(record.created_at)
                .execute(&mut *self.tx).await.map_err(sqlite_err)?;
            sqlite_db::query("INSERT INTO oc_session_turns(tenant_id,workspace_id,session_id,turn_id,source_event_id,request_hash,created_by,created_at) VALUES(?,?,?,?,?,?,?,?)")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
                .bind(record.source).bind(&record.request_hash).bind(record.created_by).bind(record.created_at)
                .execute(&mut *self.tx).await.map_err(sqlite_err)?;
            for (ordinal, evidence) in record.evidence.iter().enumerate() {
                let proof = serde_json::to_string(evidence).map_err(|_| corrupt())?;
                sqlite_db::query("INSERT INTO oc_session_turn_evidence(tenant_id,workspace_id,session_id,turn_id,evidence_ref_id,ordinal,evidence) VALUES(?,?,?,?,?,?,?)")
                    .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
                    .bind(evidence.evidence_ref_id).bind(ordinal as i64).bind(proof)
                    .execute(&mut *self.tx).await.map_err(sqlite_err)?;
            }
            sqlite_db::query("INSERT INTO oc_native_archive_proofs(tenant_id,workspace_id,session_id,turn_id,evidence_reported,body_bytes) VALUES(?,?,?,?,?,?)")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
                .bind(metadata.0).bind(metadata.1).execute(&mut *self.tx).await.map_err(sqlite_err)?;
            Ok(())
        }

        pub(crate) async fn withdraw_native_archive(
            &mut self,
            session: Uuid,
            turn: Uuid,
        ) -> StorageResult<()> {
            self.check_permission(Permission::Write).await?;
            self.require_private_session(session).await?;
            self.fence_native_archive(session, turn).await
        }

        pub(crate) async fn withdraw_session_native(&mut self, session: Uuid) -> StorageResult<()> {
            self.check_permission(Permission::Write).await?;
            self.require_private_session(session).await?;
            sqlite_db::query("INSERT INTO oc_withdrawn_private_sessions(tenant_id,workspace_id,session_id) VALUES(?,?,?) ON CONFLICT DO NOTHING")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session)
                .execute(&mut *self.tx).await.map_err(sqlite_err)?;
            sqlite_db::query("UPDATE oc_native_archive_sessions SET cleanup_after=COALESCE(cleanup_after,?) WHERE tenant_id=? AND workspace_id=? AND session_id=?")
                .bind(now_ms()).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session)
                .execute(&mut *self.tx).await.map_err(sqlite_err)?;
            Ok(())
        }
    }

    impl SqliteStore {
        async fn native_archive_cleanup_domain(
            &self,
            scope: Scope,
            session: Uuid,
        ) -> StorageResult<ArchiveDomain> {
            let owner = sqlite_db::query_scalar("SELECT logical_key_id FROM oc_key_sessions WHERE tenant_id=? AND workspace_id=? AND session_id=?")
                .bind(scope.tenant_id).bind(scope.workspace_id).bind(session)
                .fetch_optional(&self.pool).await.map_err(sqlite_err)?
                .ok_or_else(corrupt)?;
            Ok(ArchiveDomain { scope, owner })
        }

        pub(crate) async fn withdrawn_native_archive_turns(
            &self,
            limit: usize,
        ) -> StorageResult<Vec<(ArchiveDomain, Uuid, Uuid)>> {
            let due: Vec<(Uuid, Uuid, Uuid, Uuid)> = sqlite_db::query_as("SELECT tenant_id,workspace_id,session_id,turn_id FROM oc_native_archive_withdrawals WHERE cleanup_after<=? ORDER BY cleanup_after,tenant_id,workspace_id,session_id,turn_id LIMIT ?")
                .bind(now_ms()).bind(limit as i64).fetch_all(&self.pool).await.map_err(sqlite_err)?;
            let mut targets = Vec::with_capacity(due.len());
            for (tenant_id, workspace_id, session, turn) in due {
                let scope = Scope {
                    tenant_id,
                    workspace_id,
                };
                let domain = self.native_archive_cleanup_domain(scope, session).await?;
                targets.push((domain, session, turn));
            }
            Ok(targets)
        }

        pub(crate) async fn withdrawn_native_archive_sessions(
            &self,
            limit: usize,
        ) -> StorageResult<Vec<(ArchiveDomain, Uuid)>> {
            let due: Vec<(Uuid, Uuid, Uuid)> = sqlite_db::query_as("SELECT tenant_id,workspace_id,session_id FROM oc_native_archive_sessions WHERE cleanup_after<=? ORDER BY cleanup_after,tenant_id,workspace_id,session_id LIMIT ?")
                .bind(now_ms()).bind(limit as i64).fetch_all(&self.pool).await.map_err(sqlite_err)?;
            let mut targets = Vec::with_capacity(due.len());
            for (tenant_id, workspace_id, session) in due {
                let scope = Scope {
                    tenant_id,
                    workspace_id,
                };
                let domain = self.native_archive_cleanup_domain(scope, session).await?;
                targets.push((domain, session));
            }
            Ok(targets)
        }

        pub(crate) async fn settle_native_turn_cleanup(
            &self,
            scope: Scope,
            session: Uuid,
            turn: Uuid,
            cleaned: bool,
        ) -> StorageResult<()> {
            let retry_at = (!cleaned).then(|| now_ms() + 30_000);
            sqlite_db::query("UPDATE oc_native_archive_withdrawals SET cleanup_after=? WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=?")
                .bind(retry_at).bind(scope.tenant_id).bind(scope.workspace_id).bind(session).bind(turn)
                .execute(&self.pool).await.map_err(sqlite_err)?;
            Ok(())
        }

        pub(crate) async fn reschedule_native_session_cleanup(
            &self,
            scope: Scope,
            session: Uuid,
            emptied_page: bool,
        ) -> StorageResult<()> {
            let retry_at = now_ms() + if emptied_page { 1_000 } else { 30_000 };
            sqlite_db::query("UPDATE oc_native_archive_sessions SET cleanup_after=? WHERE tenant_id=? AND workspace_id=? AND session_id=? AND cleanup_after IS NOT NULL")
                .bind(retry_at).bind(scope.tenant_id).bind(scope.workspace_id).bind(session)
                .execute(&self.pool).await.map_err(sqlite_err)?;
            Ok(())
        }
    }
}

#[cfg(all(test, feature = "local-storage"))]
#[tokio::test]
async fn native_archive_authority_keeps_only_used_proof_and_fences_withdrawal() {
    use crate::{
        session::{ArchivedTurnBody, EvidenceUse},
        storage::anyseek::private_archive::NativeArchiveTurn,
    };

    let directory = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(directory.path().join("authority.db"))
        .await
        .unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "workspace")
        .await
        .unwrap();
    let key = store.issue_key(scope, "admin").await.unwrap();
    let auth = store.authenticate(&key.token).await.unwrap();
    let mut tx = store.begin(auth.clone()).await.unwrap();
    let session = tx.private_session_id().await.unwrap();
    let turn = Uuid::new_v4();
    let owner = tx.archive_domain(session).await.unwrap();
    assert!(!tx.native_archive_initialized(session).await.unwrap());
    let first = tx.allocate_native_archive_sequence(session).await.unwrap();
    assert_eq!(first, 1);
    assert!(tx.native_archive_initialized(session).await.unwrap());
    assert!(
        tx.native_archive_proof_metadata(session, turn)
            .await
            .unwrap()
            .is_none()
    );
    let digest = hash("private archive request");
    tx.begin_command("session.turn.archive", "fixture-turn", &digest)
        .await
        .unwrap();
    tx.finish_command(json!({"session_id":session,"turn_id":turn}))
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let mut tx = store.begin(auth.clone()).await.unwrap();
    assert!(
        tx.native_archive_request_exists(session, turn)
            .await
            .unwrap()
    );
    assert_eq!(
        tx.allocate_native_archive_sequence(session).await.unwrap(),
        2
    );
    let record = NativeArchiveTurn {
        session,
        turn,
        source: Uuid::new_v4(),
        created_by: auth.principal_id,
        created_at: now_ms(),
        sequence: first,
        request_hash: digest,
        evidence: vec![],
        body: ArchivedTurnBody {
            question: "private source question".into(),
            answer: "private source answer".into(),
            evidence_use: EvidenceUse::Reported,
        },
    };
    tx.attach_native_archive_proof(&record).await.unwrap();
    tx.attach_native_archive_proof(&record).await.unwrap();
    assert_eq!(
        tx.require_turn_visible(session, turn).await.unwrap(),
        record.source
    );
    assert_eq!(
        tx.event_content(record.source, true)
            .await
            .unwrap()
            .as_deref(),
        Some("")
    );
    tx.retract_event(record.source).await.unwrap();
    assert!(tx.native_turn_withdrawn(session, turn).await.unwrap());
    tx.commit().await.unwrap();
    assert_eq!(
        store.withdrawn_native_archive_turns(2).await.unwrap(),
        [(owner, session, turn)]
    );
    store
        .settle_native_turn_cleanup(scope, session, turn, true)
        .await
        .unwrap();
    assert!(
        store
            .withdrawn_native_archive_turns(2)
            .await
            .unwrap()
            .is_empty()
    );
    let mut tx = store.begin(auth).await.unwrap();
    assert!(matches!(
        tx.require_native_archive_live(session, turn).await,
        Err(StorageError::NotFound)
    ));
    tx.commit().await.unwrap();
}
