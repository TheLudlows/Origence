//! SQLite holds the immutable turn directory and delivery/cleanup decisions.
use super::*;

pub(super) const SCHEMA: &str = include_str!("../private-archive-schema.sql");

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
        Ok(state.map_or(cfg!(test), |s| s == "archived"))
    }

    pub(super) async fn archive_body_metadata(
        &mut self,
        session: Uuid,
        turn: Uuid,
    ) -> StorageResult<Option<(bool, usize)>> {
        if !archive_schema_present(&mut self.tx).await? {
            return Ok(None);
        }
        let row: Option<(bool, i64)> = sqlite_db::query_as("SELECT evidence_reported,body_bytes FROM oc_private_archives WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        row.map(|(reported, bytes)| {
            usize::try_from(bytes)
                .map(|bytes| (reported, bytes))
                .map_err(|_| StorageError::Unavailable("invalid private archive body size".into()))
        })
        .transpose()
    }

    pub(super) async fn withdraw_archive_source(&mut self, source: Uuid) -> StorageResult<()> {
        if archive_schema_present(&mut self.tx).await? {
            sqlite_db::query("UPDATE oc_private_archives SET state='withdrawn' WHERE tenant_id=? AND workspace_id=? AND source_event_id=?")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(source)
                .execute(&mut *self.tx).await.map_err(sqlite_err)?;
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
    };

    #[derive(Clone)]
    pub(crate) struct ArchiveEntry {
        pub locator: ArchiveLocator,
        pub state: String,
        pub sequence: i64,
        pub evidence_use: EvidenceUse,
    }

    pub(crate) type Row = (
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
        pub(crate) fn from_row(scope: Scope, row: Row) -> StorageResult<Self> {
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
            let withdrawn: bool = sqlite_db::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_withdrawn_private_sessions WHERE tenant_id=? AND workspace_id=? AND session_id=?)")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session)
                .fetch_one(&mut *self.tx).await.map_err(sqlite_err)?;
            if withdrawn {
                return Err(StorageError::NotFound);
            }
            Ok(ArchiveDomain {
                scope: self.scope,
                owner: self.private_key_id().await?,
            })
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

        pub(crate) async fn archive_list(
            &mut self,
            session: Uuid,
            after: i64,
            limit: usize,
        ) -> StorageResult<Vec<ArchiveEntry>> {
            let domain = self.archive_domain(session).await?;
            let rows: Vec<Row> = sqlite_db::query_as("SELECT logical_key_id,session_id,turn_id,source_event_id,body_hash,operation_epoch,evidence_reported,turn_sequence,state,group_id FROM oc_private_archives WHERE tenant_id=? AND workspace_id=? AND logical_key_id=? AND session_id=? AND state='archived' AND turn_sequence>? ORDER BY turn_sequence LIMIT ?")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(domain.owner).bind(session).bind(after).bind(limit as i64)
                .fetch_all(&mut *self.tx).await.map_err(sqlite_err)?;
            rows.into_iter()
                .map(|row| ArchiveEntry::from_row(self.scope, row))
                .collect()
        }

        pub(crate) async fn archive_candidates(
            &mut self,
            session: Uuid,
            groups: &[String],
        ) -> StorageResult<HashMap<String, ArchiveEntry>> {
            let domain = self.archive_domain(session).await?;
            let mut selected = HashMap::new();
            for batch in groups.chunks(16) {
                let mut query = sqlite_db::query_as::<Row>("SELECT logical_key_id,session_id,turn_id,source_event_id,body_hash,operation_epoch,evidence_reported,turn_sequence,state,group_id FROM oc_private_archives WHERE tenant_id=? AND workspace_id=? AND logical_key_id=? AND session_id=? AND state='archived' AND group_id IN (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
                    .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(domain.owner).bind(session);
                for i in 0..16 {
                    query = query.bind(batch.get(i).map(String::as_str));
                }
                for row in query.fetch_all(&mut *self.tx).await.map_err(sqlite_err)? {
                    let entry = ArchiveEntry::from_row(self.scope, row)?;
                    match self.require_archive_support(&entry).await {
                        Ok(_) => {
                            selected.insert(entry.locator.group(), entry);
                        }
                        Err(StorageError::NotFound) => {}
                        Err(error) => return Err(error),
                    }
                }
            }
            Ok(selected)
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
