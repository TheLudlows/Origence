//! SQLite holds session coordination and used dependency proofs.
use super::*;
#[cfg(all(test, feature = "host"))]
use crate::fixture_keys::FixtureKeys;

pub(super) const SCHEMA: &str = include_str!("../native-archive-schema.sql");

impl SqliteTx {
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

    pub(super) async fn archive_source_active(&mut self, source: Uuid) -> StorageResult<bool> {
        sqlite_db::query_scalar(
            "SELECT state='active' FROM oc_events WHERE tenant_id=? AND workspace_id=? AND id=?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(source)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(sqlite_err)?
        .ok_or(StorageError::NotFound)
    }

    pub(crate) async fn native_archive_turn_for_source(
        &mut self,
        source: Uuid,
    ) -> StorageResult<Option<(Uuid, Uuid)>> {
        let turn = self.native_archive_turn_for_source_local(source).await?;
        if let Some((session, _)) = turn {
            self.require_private_session(session).await?;
        }
        Ok(turn)
    }

    /// Resolve stored provenance in this scope without reading the archive body.
    pub(super) async fn native_archive_turn_for_source_local(
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
            return Ok(Some((session, turn)));
        }
        Ok(None)
    }
}

#[cfg(feature = "host")]
mod host {
    use super::*;
    use crate::{session::ArchiveDomain, storage::anyseek::private_archive::NativeArchiveTurn};

    fn corrupt() -> StorageError {
        StorageError::Unavailable("invalid private archive metadata".into())
    }

    impl SqliteTx {
        /// Admission runs in the archive's final write transaction. Completed
        /// and failed jobs are also bindings, so client replays never enqueue
        /// a second extraction after a memory has been deleted.
        pub(crate) async fn archive_extraction_job(
            &mut self,
            source: Uuid,
        ) -> StorageResult<Option<Uuid>> {
            sqlite_db::query_scalar(
                "SELECT id FROM oc_jobs
                 WHERE tenant_id=? AND workspace_id=? AND source_event_id=?
                 AND operation='extract' AND json_type(payload,'$.archive_turn')='text'",
            )
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .bind(source)
            .fetch_optional(&mut *self.tx)
            .await
            .map_err(sqlite_err)
        }

        pub(crate) async fn archive_domain(
            &mut self,
            session: Uuid,
        ) -> StorageResult<ArchiveDomain> {
            self.check_permission(Permission::Write).await?;
            let owner = self.require_private_session_owner(session).await?;
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

        pub(crate) async fn native_archive_live_proof_digest(
            &mut self,
            session: Uuid,
            turn: Uuid,
        ) -> StorageResult<Option<(Uuid, String)>> {
            let Some((source, digest)) = self.native_archive_proof_digest(session, turn).await?
            else {
                return Ok(None);
            };
            Ok(self
                .archive_source_active(source)
                .await?
                .then_some((source, digest)))
        }

        pub(crate) async fn event_exists(&mut self, source: Uuid) -> StorageResult<bool> {
            sqlite_db::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_events WHERE tenant_id=? AND workspace_id=? AND id=?)")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(source)
                .fetch_one(&mut *self.tx).await.map_err(sqlite_err)
        }

        pub(crate) async fn archive_turn_sequence(
            &mut self,
            session: Uuid,
            turn: Uuid,
        ) -> StorageResult<Option<i64>> {
            sqlite_db::query_scalar("SELECT MIN(json_extract(response,'$.sequence')) FROM oc_commands WHERE tenant_id=? AND workspace_id=? AND operation='session.turn.archive' AND json_extract(response,'$.session_id')=? AND json_extract(response,'$.turn_id')=?")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id)
                .bind(session.to_string()).bind(turn.to_string())
                .fetch_one(&mut *self.tx).await.map_err(sqlite_err)
        }

        pub(crate) async fn archive_last_sequence(&mut self, session: Uuid) -> StorageResult<i64> {
            let sequence = sqlite_db::query_scalar("SELECT last_sequence FROM oc_native_archive_sessions WHERE tenant_id=? AND workspace_id=? AND session_id=?")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session)
                .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
            Ok(sequence.unwrap_or(0))
        }

        pub(crate) async fn archive_delete_page(
            &mut self,
            session: Uuid,
            after: i64,
            through: i64,
        ) -> StorageResult<Vec<(i64, String)>> {
            sqlite_db::query_as("SELECT DISTINCT json_extract(response,'$.sequence') AS sequence,json_extract(response,'$.turn_id') FROM oc_commands WHERE tenant_id=? AND workspace_id=? AND operation='session.turn.archive' AND json_extract(response,'$.session_id')=? AND sequence>? AND sequence<=? ORDER BY sequence LIMIT 16")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id)
                .bind(session.to_string()).bind(after).bind(through)
                .fetch_all(&mut *self.tx).await.map_err(sqlite_err)
        }

        // The existing source state protects SQLite-only evidence consumers.
        // Plain archived turns never acquire a relational source row.
        pub(crate) async fn retract_archive_support(
            &mut self,
            session: Uuid,
            turn: Uuid,
        ) -> StorageResult<()> {
            if let Some((source, _)) = self.native_archive_proof_digest(session, turn).await? {
                self.retract_event_inner(source).await?;
            }
            Ok(())
        }

        /// Preserve the integer paging cursor without storing a row for each turn.
        pub(crate) async fn allocate_native_archive_sequence(
            &mut self,
            session: Uuid,
        ) -> StorageResult<i64> {
            self.archive_domain(session).await?;
            let sequence = self
                .archive_last_sequence(session)
                .await?
                .checked_add(1)
                .ok_or_else(corrupt)?;
            sqlite_db::query("INSERT INTO oc_native_archive_sessions(tenant_id,workspace_id,session_id,last_sequence) VALUES(?,?,?,?) ON CONFLICT DO UPDATE SET last_sequence=excluded.last_sequence")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(sequence)
                .execute(&mut *self.tx).await.map_err(sqlite_err)?;
            Ok(sequence)
        }

        /// A durable consumer may keep the exact support it used for transactional
        /// rechecks. Plain saves, reads, and searches never create such proof.
        pub(crate) async fn attach_native_archive_proof(
            &mut self,
            record: &NativeArchiveTurn,
        ) -> StorageResult<()> {
            let (session, turn) = (record.session, record.turn);
            self.archive_domain(session).await?;
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
    }
}

#[cfg(all(test, feature = "host"))]
#[tokio::test]
async fn native_archive_authority_keeps_only_used_proof() {
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
    tx.finish_command(json!({"session_id":session,"turn_id":turn,"sequence":first}))
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let mut tx = store.begin(auth.clone()).await.unwrap();
    assert!(tx.archive_turn_sequence(session, turn).await.unwrap() == Some(first));
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
    tx.commit().await.unwrap();
    let mut tx = store.begin(auth).await.unwrap();
    assert!(matches!(
        tx.require_turn_visible(session, turn).await,
        Err(StorageError::NotFound)
    ));
    tx.commit().await.unwrap();
}
