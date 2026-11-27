//! SQLite holds session coordination, used dependency proofs and withdrawal fences.
use super::*;

pub(super) const SCHEMA: &str = include_str!("../private-archive-schema.sql");
pub(super) const NATIVE_SCHEMA: &str = include_str!("../native-archive-schema.sql");

impl SqliteTx {
    pub(super) async fn archive_state_visible(
        &mut self,
        session: Uuid,
        turn: Uuid,
    ) -> StorageResult<bool> {
        // Pre-conversion fixtures and the offline migration use the legacy
        // directory. Normal opens verify the complete current schema.
        if cfg!(test) && !archive_schema_present(&mut self.tx).await? {
            return Ok(true);
        }
        let state: Option<String> = sqlite_db::query_scalar(
            "SELECT state FROM oc_private_archives WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=?",
        ).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        if let Some(state) = state {
            return Ok(state == "archived");
        }
        let proof = self.native_archive_proof_metadata(session, turn).await?;
        if proof.is_none() {
            return Ok(cfg!(test));
        }
        Ok(!self.archive_session_withdrawn(session).await?
            && !self.native_turn_withdrawn(session, turn).await?)
    }

    pub(super) async fn archive_body_metadata(
        &mut self,
        session: Uuid,
        turn: Uuid,
    ) -> StorageResult<(bool, usize)> {
        let legacy: Option<(bool, i64)> = sqlite_db::query_as("SELECT evidence_reported,body_bytes FROM oc_private_archives WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=? AND state='archived'")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        let (reported, bytes) = match legacy {
            Some(metadata) => metadata,
            None => self
                .native_archive_proof_metadata(session, turn)
                .await?
                .ok_or(StorageError::NotFound)?,
        };
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

    pub(super) async fn withdraw_archive_source(&mut self, source: Uuid) -> StorageResult<()> {
        if cfg!(test) && !archive_schema_present(&mut self.tx).await? {
            return Ok(());
        }
        sqlite_db::query("UPDATE oc_private_archives SET state='withdrawn' WHERE tenant_id=? AND workspace_id=? AND source_event_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(source)
            .execute(&mut *self.tx).await.map_err(sqlite_err)?;
        let turn: Option<(Uuid, Uuid)> = sqlite_db::query_as("SELECT session_id,turn_id FROM oc_session_turns WHERE tenant_id=? AND workspace_id=? AND source_event_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(source)
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        if let Some((session, turn)) = turn
            && self
                .native_archive_proof_metadata(session, turn)
                .await?
                .is_some()
        {
            self.fence_native_archive(session, turn).await?;
        }
        Ok(())
    }
}

async fn archive_schema_present(conn: &mut Connection) -> StorageResult<bool> {
    sqlite_db::query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='oc_private_archives' AND type='table')")
        .fetch_one(conn).await.map_err(sqlite_err)
}

#[cfg(feature = "local-storage")]
mod host {
    use super::*;
    use crate::{
        context_retrieval::ContextSessionTurn,
        session::{ArchiveDomain, ArchiveLocator, ArchivedTurnBody, EvidenceUse, SessionTurnInput},
        storage::anyseek::private_archive::NativeArchiveTurn,
    };

    #[derive(Clone)]
    pub(crate) struct ArchiveEntry {
        pub locator: ArchiveLocator,
        pub state: String,
        pub sequence: i64,
        pub evidence_use: EvidenceUse,
    }

    type Row = (
        Uuid,
        Uuid,
        Uuid,
        Uuid,
        String,
        i64,
        bool,
        i64,
        String,
        String,
    );

    impl ArchiveEntry {
        fn from_row(scope: Scope, row: Row) -> StorageResult<Self> {
            let (owner, session, turn, source, body_hash, epoch, reported, sequence, state, group) =
                row;
            let locator = ArchiveLocator {
                domain: ArchiveDomain { scope, owner },
                session,
                turn,
                source,
                body_hash,
                epoch: epoch.try_into().map_err(|_| corrupt())?,
            };
            if locator.group() != group || !crate::retrieval_diagnostics::digest(&locator.body_hash)
            {
                return Err(corrupt());
            }
            Ok(Self {
                locator,
                state,
                sequence,
                evidence_use: if reported {
                    EvidenceUse::Reported
                } else {
                    EvidenceUse::Unknown
                },
            })
        }

        pub(crate) fn receipt(&self) -> Value {
            json!({"session_id":self.locator.session,"turn_id":self.locator.turn,"source_event_id":self.locator.source,"state":"archived"})
        }
    }

    fn corrupt() -> StorageError {
        StorageError::Unavailable("invalid private archive metadata".into())
    }

    impl SqliteTx {
        pub(crate) async fn archive_domain(
            &mut self,
            session: Uuid,
        ) -> StorageResult<ArchiveDomain> {
            self.require_private_session(session).await?;
            if self.archive_session_withdrawn(session).await? {
                return Err(StorageError::NotFound);
            }
            Ok(ArchiveDomain {
                scope: self.scope,
                owner: self.private_key_id().await?,
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
            let sequence = match current {
                Some(current) => current.checked_add(1).ok_or_else(corrupt)?,
                None => {
                    let legacy: i64 = sqlite_db::query_scalar("SELECT COALESCE(MAX(turn_sequence),0) FROM oc_private_archives WHERE tenant_id=? AND workspace_id=? AND session_id=?")
                        .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session)
                        .fetch_one(&mut *self.tx).await.map_err(sqlite_err)?;
                    legacy.checked_add(1).ok_or_else(corrupt)?
                }
            };
            sqlite_db::query("INSERT INTO oc_native_archive_sessions(tenant_id,workspace_id,session_id,last_sequence) VALUES(?,?,?,?) ON CONFLICT DO UPDATE SET last_sequence=excluded.last_sequence")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(sequence)
                .execute(&mut *self.tx).await.map_err(sqlite_err)?;
            Ok(sequence)
        }

        pub(crate) async fn native_archive_fenced(
            &mut self,
            session: Uuid,
            turn: Uuid,
        ) -> StorageResult<bool> {
            if self.archive_session_withdrawn(session).await?
                || self.native_turn_withdrawn(session, turn).await?
            {
                return Ok(true);
            }
            sqlite_db::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_private_archives WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=? AND state='withdrawn')")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
                .fetch_one(&mut *self.tx).await.map_err(sqlite_err)
        }

        pub(crate) async fn require_native_archive_live(
            &mut self,
            session: Uuid,
            turn: Uuid,
        ) -> StorageResult<()> {
            self.check_permission(Permission::Write).await?;
            self.require_private_session(session).await?;
            if self.native_archive_fenced(session, turn).await? {
                return Err(StorageError::NotFound);
            }
            Ok(())
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
            if !self.archive_session_withdrawn(session).await? {
                return Err(corrupt());
            }
            sqlite_db::query("UPDATE oc_native_archive_sessions SET cleanup_after=COALESCE(cleanup_after,?) WHERE tenant_id=? AND workspace_id=? AND session_id=?")
                .bind(now_ms()).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session)
                .execute(&mut *self.tx).await.map_err(sqlite_err)?;
            Ok(())
        }

        pub(crate) async fn archive_entry(
            &mut self,
            session: Uuid,
            turn: Uuid,
        ) -> StorageResult<ArchiveEntry> {
            let domain = self.archive_domain(session).await?;
            let row: Option<Row> = sqlite_db::query_as("SELECT logical_key_id,session_id,turn_id,source_event_id,body_hash,operation_epoch,evidence_reported,turn_sequence,state,group_id FROM oc_private_archives WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=?")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
                .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
            let entry = ArchiveEntry::from_row(self.scope, row.ok_or(StorageError::NotFound)?)?;
            if entry.locator.domain != domain || entry.state == "withdrawn" {
                return Err(StorageError::NotFound);
            }
            Ok(entry)
        }

        pub(crate) async fn archive_for_source(
            &mut self,
            source: Uuid,
        ) -> StorageResult<Option<(Uuid, Uuid)>> {
            self.require_private_source(source).await?;
            sqlite_db::query_as("SELECT session_id,turn_id FROM oc_private_archives WHERE tenant_id=? AND workspace_id=? AND source_event_id=?")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(source)
                .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)
        }

        pub(crate) async fn stage_archive(
            &mut self,
            session: Uuid,
            input: &SessionTurnInput,
            request_hash: &str,
            body: &ArchivedTurnBody,
            epoch: u64,
        ) -> StorageResult<ArchiveEntry> {
            let domain = self.archive_domain(session).await?;
            let body_hash = body.digest();
            let (source, inserted) = self.register_turn(session, input, request_hash).await?;
            if inserted {
                let sequence: i64 = sqlite_db::query_scalar("SELECT COALESCE(MAX(turn_sequence),0)+1 FROM oc_private_archives WHERE tenant_id=? AND workspace_id=? AND session_id=?")
                    .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session)
                    .fetch_one(&mut *self.tx).await.map_err(sqlite_err)?;
                self.insert_archive(
                    &ArchiveLocator {
                        domain,
                        session,
                        turn: input.turn_id,
                        source,
                        body_hash: body_hash.to_owned(),
                        epoch,
                    },
                    sequence,
                    input.evidence_use,
                    body.model_json_bytes(),
                )
                .await?;
                self.audit(
                    "session.turn_pending",
                    input.turn_id,
                    json!({"session_id":session,"source_event_id":source}),
                )
                .await?;
            }
            let entry = self.archive_entry(session, input.turn_id).await?;
            if entry.locator.source != source
                || entry.locator.body_hash != body_hash
                || entry.evidence_use != input.evidence_use
            {
                return Err(corrupt());
            }
            self.require_archive_support(&entry).await?;
            Ok(entry)
        }

        async fn insert_archive(
            &mut self,
            locator: &ArchiveLocator,
            sequence: i64,
            evidence_use: EvidenceUse,
            body_bytes: usize,
        ) -> StorageResult<()> {
            sqlite_db::query("INSERT INTO oc_private_archives(tenant_id,workspace_id,logical_key_id,session_id,turn_id,source_event_id,group_id,body_hash,body_bytes,operation_epoch,evidence_reported,turn_sequence,state) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,'pending')")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(locator.domain.owner)
                .bind(locator.session).bind(locator.turn).bind(locator.source).bind(locator.group())
                .bind(&locator.body_hash).bind(body_bytes as i64).bind(locator.epoch as i64).bind(evidence_use.is_reported()).bind(sequence)
                .execute(&mut *self.tx).await.map_err(sqlite_err)?;
            Ok(())
        }

        async fn require_archive_support(
            &mut self,
            entry: &ArchiveEntry,
        ) -> StorageResult<Vec<crate::session::ArchivedEvidence>> {
            let (source, evidence) = self
                .turn_support_local(entry.locator.session, entry.locator.turn)
                .await?;
            if source != entry.locator.source {
                return Err(corrupt());
            }
            self.require_archived_evidence_set_visible(&evidence)
                .await?;
            Ok(evidence)
        }

        pub(crate) async fn confirm_archive(
            &mut self,
            entry: &ArchiveEntry,
            confirmed_seq: u64,
        ) -> StorageResult<()> {
            let live = self
                .archive_entry(entry.locator.session, entry.locator.turn)
                .await?;
            if entry.locator != live.locator {
                return Err(corrupt());
            }
            self.require_archive_support(&live).await?;
            if live.state == "pending" {
                sqlite_db::query("UPDATE oc_private_archives SET state='archived',confirmed_seq=? WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=? AND state='pending'")
                    .bind(i64::try_from(confirmed_seq).map_err(|_| corrupt())?).bind(self.scope.tenant_id).bind(self.scope.workspace_id)
                    .bind(live.locator.session).bind(live.locator.turn).execute(&mut *self.tx).await.map_err(sqlite_err)?;
                self.audit("session.turn_archived", live.locator.turn, json!({"session_id":live.locator.session,"source_event_id":live.locator.source})).await?;
            }
            Ok(())
        }

        pub(crate) async fn archive_delivery(
            &mut self,
            entry: &ArchiveEntry,
            body: ArchivedTurnBody,
        ) -> StorageResult<ContextSessionTurn> {
            let live = self
                .archive_entry(entry.locator.session, entry.locator.turn)
                .await?;
            if live.state != "archived" {
                return Err(StorageError::NotFound);
            }
            if live.locator != entry.locator || live.evidence_use != body.evidence_use {
                return Err(corrupt());
            }
            let evidence = self.require_archive_support(&live).await?;
            let created_at: i64 = sqlite_db::query_scalar("SELECT created_at FROM oc_session_turns WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=?")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(entry.locator.session).bind(entry.locator.turn)
                .fetch_one(&mut *self.tx).await.map_err(sqlite_err)?;
            Ok(ContextSessionTurn {
                session_id: entry.locator.session,
                turn_id: entry.locator.turn,
                source_event_id: entry.locator.source,
                question: body.question,
                answer: body.answer,
                evidence,
                evidence_use: body.evidence_use,
                created_at,
                score: 0.0,
                retrieval_routes: Vec::new(),
            })
        }

        pub(crate) async fn archive_view(
            &mut self,
            entry: &ArchiveEntry,
            body: ArchivedTurnBody,
        ) -> StorageResult<Value> {
            let item = self.archive_delivery(entry, body).await?;
            let created_by: Uuid = sqlite_db::query_scalar("SELECT created_by FROM oc_session_turns WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=?")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(item.session_id).bind(item.turn_id)
                .fetch_one(&mut *self.tx).await.map_err(sqlite_err)?;
            let feedback = self.feedback_rows(item.session_id, item.turn_id).await?;
            let mut value = json!({
                "session_id":item.session_id,"turn_id":item.turn_id,"source_event_id":item.source_event_id,
                "question":item.question,"answer":item.answer,"used_evidence":item.evidence,
                "feedback":feedback,"created_by":created_by,"created_at":item.created_at,"sequence":entry.sequence,
            });
            if !item.evidence_use.is_reported() {
                value["evidence_use"] = json!(item.evidence_use);
            }
            Ok(value)
        }

        /// Full directory rows define the page cursor. Only entries with
        /// currently valid support are eligible for native body reads or search.
        pub(crate) async fn archive_page(
            &mut self,
            session: Uuid,
            after: i64,
            limit: usize,
        ) -> StorageResult<(ArchiveDomain, Option<i64>, usize, Vec<ArchiveEntry>)> {
            let domain = self.archive_domain(session).await?;
            let rows: Vec<Row> = sqlite_db::query_as("SELECT logical_key_id,session_id,turn_id,source_event_id,body_hash,operation_epoch,evidence_reported,turn_sequence,state,group_id FROM oc_private_archives WHERE tenant_id=? AND workspace_id=? AND logical_key_id=? AND session_id=? AND state='archived' AND turn_sequence>? ORDER BY turn_sequence LIMIT ?")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(domain.owner).bind(session).bind(after).bind(limit as i64)
                .fetch_all(&mut *self.tx).await.map_err(sqlite_err)?;
            let next = rows.last().map(|row| row.7);
            let scanned = rows.len();
            let mut selected = Vec::new();
            for row in rows {
                let entry = ArchiveEntry::from_row(self.scope, row)?;
                match self.require_archive_support(&entry).await {
                    Ok(_) => selected.push(entry),
                    Err(StorageError::NotFound) => {}
                    Err(error) => return Err(error),
                }
            }
            Ok((domain, next, scanned, selected))
        }

        pub(crate) async fn withdraw_archives(
            &mut self,
            session: Uuid,
            turn: Option<Uuid>,
        ) -> StorageResult<()> {
            self.require_private_session(session).await?;
            if turn.is_none() {
                sqlite_db::query("INSERT INTO oc_withdrawn_private_sessions(tenant_id,workspace_id,session_id) VALUES(?,?,?) ON CONFLICT DO NOTHING")
                    .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session)
                    .execute(&mut *self.tx).await.map_err(sqlite_err)?;
            }
            let count = sqlite_db::query("UPDATE oc_private_archives SET state='withdrawn' WHERE tenant_id=? AND workspace_id=? AND session_id=? AND (? IS NULL OR turn_id=?)")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn).bind(turn)
                .execute(&mut *self.tx).await.map_err(sqlite_err)?.rows_affected();
            if turn.is_some() && count == 0 {
                return Err(StorageError::NotFound);
            }
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

        pub(crate) async fn native_archive_sessions(
            &self,
        ) -> StorageResult<Vec<(ArchiveDomain, Uuid)>> {
            let rows: Vec<(Uuid, Uuid, Uuid)> = sqlite_db::query_as("SELECT tenant_id,workspace_id,session_id FROM oc_native_archive_sessions WHERE cleanup_after IS NULL ORDER BY tenant_id,workspace_id,session_id")
                .fetch_all(&self.pool).await.map_err(sqlite_err)?;
            let mut sessions = Vec::with_capacity(rows.len());
            for (tenant_id, workspace_id, session) in rows {
                let scope = Scope {
                    tenant_id,
                    workspace_id,
                };
                let domain = self.native_archive_cleanup_domain(scope, session).await?;
                sessions.push((domain, session));
            }
            Ok(sessions)
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

        pub(crate) async fn withdrawn_archives(
            &self,
            limit: usize,
        ) -> StorageResult<Vec<ArchiveEntry>> {
            type ScopedRow = (
                Uuid,
                Uuid,
                Uuid,
                Uuid,
                Uuid,
                Uuid,
                String,
                i64,
                bool,
                i64,
                String,
                String,
            );
            let rows: Vec<ScopedRow> = sqlite_db::query_as("SELECT tenant_id,workspace_id,logical_key_id,session_id,turn_id,source_event_id,body_hash,operation_epoch,evidence_reported,turn_sequence,state,group_id FROM oc_private_archives WHERE state='withdrawn' AND cleanup_done=0 AND cleanup_after<=? ORDER BY cleanup_after,tenant_id,workspace_id,session_id,turn_sequence LIMIT ?")
                .bind(now_ms()).bind(limit as i64).fetch_all(&self.pool).await.map_err(sqlite_err)?;
            rows.into_iter()
                .map(
                    |(
                        tenant_id,
                        workspace_id,
                        owner,
                        session,
                        turn,
                        source,
                        body_hash,
                        epoch,
                        reported,
                        sequence,
                        state,
                        group,
                    )| {
                        ArchiveEntry::from_row(
                            Scope {
                                tenant_id,
                                workspace_id,
                            },
                            (
                                owner, session, turn, source, body_hash, epoch, reported, sequence,
                                state, group,
                            ),
                        )
                    },
                )
                .collect()
        }

        pub(crate) async fn defer_archive_cleanup(
            &self,
            locator: &ArchiveLocator,
        ) -> StorageResult<()> {
            sqlite_db::query("UPDATE oc_private_archives SET cleanup_after=? WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=? AND state='withdrawn' AND cleanup_done=0")
                .bind(now_ms() + 30_000).bind(locator.domain.scope.tenant_id).bind(locator.domain.scope.workspace_id)
                .bind(locator.session).bind(locator.turn).execute(&self.pool).await.map_err(sqlite_err)?;
            Ok(())
        }

        pub(crate) async fn finish_archive_cleanup(
            &self,
            locator: &ArchiveLocator,
        ) -> StorageResult<()> {
            sqlite_db::query("UPDATE oc_private_archives SET cleanup_done=1 WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=? AND state='withdrawn'")
                .bind(locator.domain.scope.tenant_id).bind(locator.domain.scope.workspace_id)
                .bind(locator.session).bind(locator.turn).execute(&self.pool).await.map_err(sqlite_err)?;
            Ok(())
        }
    }
}
#[cfg(feature = "local-storage")]
pub(crate) use host::ArchiveEntry;

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
    assert!(tx.native_archive_fenced(session, turn).await.unwrap());
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
