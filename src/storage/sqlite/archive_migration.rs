//! Exclusive maintenance for the supported relational and first AnySeek archive schemas.
use super::*;
use crate::session::{ArchiveDomain, ArchiveLocator, ArchivedTurnBody, EvidenceUse};
use std::{collections::BTreeSet, path::Path};

type PrefixRow = (Uuid, Uuid, Uuid, i64);

pub(crate) struct ArchiveMigrationDirectory {
    pub(crate) legacy: Vec<(ArchiveDomain, Uuid, Uuid, Uuid, i64)>,
    pub(crate) native: Vec<NativeRestoreSession>,
}

pub(crate) struct NativeRestoreSession {
    pub(crate) domain: ArchiveDomain,
    pub(crate) session: Uuid,
    pub(crate) last_sequence: i64,
    pub(crate) withdrawn: bool,
    pub(crate) withdrawn_turns: BTreeSet<Uuid>,
    pub(crate) expected: HashMap<Uuid, (Uuid, String)>,
}

pub(crate) struct LegacyArchive {
    pub(crate) locator: ArchiveLocator,
    pub(crate) sequence: i64,
    pub(crate) state: String,
    pub(crate) confirmed_seq: Option<u64>,
    pub(crate) old_body: Option<ArchivedTurnBody>,
}

fn corrupt() -> StorageError {
    StorageError::Unavailable("private archive migration proof does not match".into())
}

fn decode<T: serde::de::DeserializeOwned>(text: &str) -> StorageResult<T> {
    serde_json::from_str(text).map_err(|_| corrupt())
}

fn encode(value: &impl serde::Serialize) -> StorageResult<String> {
    serde_json::to_string(value).map_err(|_| corrupt())
}

async fn read_native_restore_sessions(
    tx: &mut Connection,
) -> StorageResult<HashMap<(Uuid, Uuid, Uuid), NativeRestoreSession>> {
    let withdrawn: BTreeSet<(Uuid, Uuid, Uuid)> = sqlite_db::query_as(
        "SELECT tenant_id,workspace_id,session_id FROM oc_withdrawn_private_sessions",
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(sqlite_err)?
    .into_iter()
    .collect();
    let rows: Vec<(Uuid, Uuid, Uuid, i64, bool)> = sqlite_db::query_as(
        "SELECT tenant_id,workspace_id,session_id,last_sequence,cleanup_after IS NOT NULL FROM oc_native_archive_sessions",
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(sqlite_err)?;
    let mut sessions = HashMap::with_capacity(rows.len());
    for (tenant_id, workspace_id, session, last_sequence, cleaning) in rows {
        let key = (tenant_id, workspace_id, session);
        let withdrawn = withdrawn.contains(&key);
        if cleaning != withdrawn {
            return Err(corrupt());
        }
        let owner = sqlite_db::query_scalar("SELECT logical_key_id FROM oc_key_sessions WHERE tenant_id=? AND workspace_id=? AND session_id=?")
            .bind(tenant_id).bind(workspace_id).bind(session)
            .fetch_optional(&mut *tx).await.map_err(sqlite_err)?.ok_or_else(corrupt)?;
        sessions.insert(
            key,
            NativeRestoreSession {
                domain: ArchiveDomain {
                    scope: Scope {
                        tenant_id,
                        workspace_id,
                    },
                    owner,
                },
                session,
                last_sequence,
                withdrawn,
                withdrawn_turns: BTreeSet::new(),
                expected: HashMap::new(),
            },
        );
    }
    let tombstones: Vec<(Uuid, Uuid, Uuid, Uuid)> = sqlite_db::query_as(
        "SELECT tenant_id,workspace_id,session_id,turn_id FROM oc_native_archive_withdrawals",
    )
    .fetch_all(tx)
    .await
    .map_err(sqlite_err)?;
    for (tenant_id, workspace_id, session, turn) in tombstones {
        sessions
            .get_mut(&(tenant_id, workspace_id, session))
            .ok_or_else(corrupt)?
            .withdrawn_turns
            .insert(turn);
    }
    Ok(sessions)
}

impl SqliteStore {
    /// Only called by explicit offline maintenance. Ordinary opens still reject
    /// the preceding schema before opening it writable.
    pub(crate) async fn prepare_archive_migration(path: &Path) -> StorageResult<()> {
        let path = tokio::fs::canonicalize(path).await?;
        let (lock, opening) = locking::acquire_path_lock(&path, false)?;
        let lock = std::sync::Arc::new(lock);
        let _opening = opening.lock().await;
        let source = Pool::open_read_only(&path).await.map_err(sqlite_err)?;
        source.hold_lifetime_guard(lock.clone());
        let predecessor_has_v1_archive = async {
            let mut conn = source.acquire().await.map_err(sqlite_err)?;
            if schema::check_conn(&mut conn).await.is_ok() {
                sqlite_db::check_format(&mut conn, "authority")
                    .await
                    .map_err(sqlite_err)?;
                return Ok::<_, StorageError>(None);
            }
            if schema::check_legacy_v1_archive_schema(&mut conn)
                .await
                .is_ok()
            {
                return Ok(Some(true));
            }
            schema::check_legacy_archive_schema(&mut conn).await?;
            Ok(Some(false))
        }
        .await;
        source.close().await;
        let Some(has_v1_archive) = predecessor_has_v1_archive? else {
            return Ok(());
        };
        let pool = Pool::open(&path).await.map_err(sqlite_err)?;
        pool.hold_lifetime_guard(lock);
        let result = async {
            let mut tx = pool
                .begin_with("BEGIN IMMEDIATE")
                .await
                .map_err(sqlite_err)?;
            if has_v1_archive {
                schema::check_legacy_v1_archive_schema(&mut tx).await?;
            } else {
                schema::check_legacy_archive_schema(&mut tx).await?;
                sqlite_db::raw_sql(private_archive::SCHEMA)
                    .execute(&mut *tx)
                    .await
                    .map_err(sqlite_err)?;
                sqlite_db::query(
                    "INSERT INTO oc_private_archive_migration(id,phase) VALUES(1,'copying')",
                )
                .execute(&mut *tx)
                .await
                .map_err(sqlite_err)?;
            }
            schema::convert_legacy_learning_catalog(&mut tx).await?;
            sqlite_db::raw_sql(private_archive::NATIVE_SCHEMA)
                .execute(&mut *tx)
                .await
                .map_err(sqlite_err)?;
            schema::check_conn(&mut tx).await?;
            tx.commit().await.map_err(sqlite_err)
        }
        .await;
        pool.close().await;
        result
    }

    pub(crate) async fn archive_migration_phase(&self) -> StorageResult<Option<String>> {
        sqlite_db::query_scalar("SELECT phase FROM oc_private_archive_migration WHERE id=1")
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlite_err)
    }

    /// Historically created_at and stable turn IDs defined order for equal timestamps.
    pub(crate) async fn archive_migration_directory(
        &self,
    ) -> StorageResult<ArchiveMigrationDirectory> {
        let mut tx = self.pool.begin().await.map_err(sqlite_err)?;
        let rows: Vec<(Uuid, Uuid, Uuid, Uuid, Uuid, String)> = sqlite_db::query_as("SELECT tenant_id,workspace_id,session_id,turn_id,source_event_id,request_hash FROM oc_session_turns ORDER BY tenant_id,workspace_id,session_id,created_at,turn_id")
            .fetch_all(&mut *tx).await.map_err(sqlite_err)?;
        // Each turn below proves one unique session source. Refuse to clear
        // any source absent from that directory.
        let source_count: i64 =
            sqlite_db::query_scalar("SELECT COUNT(*) FROM oc_events WHERE kind='session_turn'")
                .fetch_one(&mut *tx)
                .await
                .map_err(sqlite_err)?;
        if source_count != rows.len() as i64 {
            return Err(corrupt());
        }
        let mut native_proofs: BTreeSet<(Uuid, Uuid, Uuid, Uuid)> = sqlite_db::query_as(
            "SELECT tenant_id,workspace_id,session_id,turn_id FROM oc_native_archive_proofs",
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(sqlite_err)?
        .into_iter()
        .collect();
        let mut native = read_native_restore_sessions(&mut tx).await?;
        let mut directory = Vec::with_capacity(rows.len());
        let mut previous = None;
        let mut sequence = 0;
        let mut owner = Uuid::nil();
        for (tenant_id, workspace_id, session, turn, source, request_hash) in rows {
            if native_proofs.remove(&(tenant_id, workspace_id, session, turn)) {
                let entry = native
                    .get_mut(&(tenant_id, workspace_id, session))
                    .ok_or_else(corrupt)?;
                let (state, empty): (String, bool) = sqlite_db::query_as("SELECT state,content='' FROM oc_events WHERE tenant_id=? AND workspace_id=? AND id=? AND kind='session_turn' AND file_id IS NULL")
                    .bind(tenant_id).bind(workspace_id).bind(source)
                    .fetch_optional(&mut *tx).await.map_err(sqlite_err)?.ok_or_else(corrupt)?;
                if !empty || !crate::retrieval_diagnostics::digest(&request_hash) {
                    return Err(corrupt());
                }
                if !entry.withdrawn && !entry.withdrawn_turns.contains(&turn) {
                    if state != "active" {
                        return Err(corrupt());
                    }
                    entry.expected.insert(turn, (source, request_hash));
                }
                continue;
            }
            let scope = Scope {
                tenant_id,
                workspace_id,
            };
            if previous != Some((scope, session)) {
                owner = sqlite_db::query_scalar("SELECT logical_key_id FROM oc_key_sessions WHERE tenant_id=? AND workspace_id=? AND session_id=?")
                    .bind(tenant_id).bind(workspace_id).bind(session).fetch_one(&mut *tx).await.map_err(sqlite_err)?;
                previous = Some((scope, session));
                sequence = 0;
            }
            sequence += 1;
            directory.push((
                ArchiveDomain { scope, owner },
                session,
                turn,
                source,
                sequence,
            ));
        }
        if !native_proofs.is_empty() {
            return Err(corrupt());
        }
        tx.commit().await.map_err(sqlite_err)?;
        Ok(ArchiveMigrationDirectory {
            legacy: directory,
            native: native.into_values().collect(),
        })
    }

    pub(crate) async fn archive_directory_count(&self) -> StorageResult<usize> {
        let count: i64 = sqlite_db::query_scalar("SELECT COUNT(*) FROM oc_private_archives")
            .fetch_one(&self.pool)
            .await
            .map_err(sqlite_err)?;
        count.try_into().map_err(|_| corrupt())
    }

    pub(crate) async fn migration_archive(
        &self,
        domain: ArchiveDomain,
        session: Uuid,
        turn: Uuid,
        source: Uuid,
        sequence: i64,
        copying: bool,
        stage: bool,
    ) -> StorageResult<LegacyArchive> {
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(sqlite_err)?;
        let scope = domain.scope;
        let existing: Option<(String, String, Uuid, Uuid, i64, i64, bool, Option<i64>, String)> = sqlite_db::query_as(
            "SELECT state,body_hash,logical_key_id,source_event_id,operation_epoch,turn_sequence,evidence_reported,confirmed_seq,group_id FROM oc_private_archives WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=?",
        ).bind(scope.tenant_id).bind(scope.workspace_id).bind(session).bind(turn)
            .fetch_optional(&mut *tx).await.map_err(sqlite_err)?;
        let (old, source_state): (String,String) = sqlite_db::query_as("SELECT content,state FROM oc_events WHERE tenant_id=? AND workspace_id=? AND id=? AND kind='session_turn' AND file_id IS NULL")
            .bind(scope.tenant_id).bind(scope.workspace_id).bind(source).fetch_one(&mut *tx).await.map_err(sqlite_err)?;
        let old_body = if old.is_empty() {
            None
        } else {
            Some(decode::<ArchivedTurnBody>(&old)?)
        };
        if let Some((
            state,
            hash,
            owner,
            saved_source,
            epoch,
            saved_seq,
            reported,
            confirmed_seq,
            group,
        )) = existing
        {
            if owner != domain.owner
                || saved_source != source
                || (copying && saved_seq != sequence)
                || (!copying && old_body.is_some())
                || (source_state == "retracted" && state != "withdrawn")
                || !crate::retrieval_diagnostics::digest(&hash)
                || old_body
                    .as_ref()
                    .is_some_and(|b| b.digest() != hash || b.evidence_use.is_reported() != reported)
            {
                return Err(corrupt());
            }
            let locator = ArchiveLocator {
                domain,
                session,
                turn,
                source,
                body_hash: hash,
                epoch: epoch.try_into().map_err(|_| corrupt())?,
            };
            if locator.group() != group {
                return Err(corrupt());
            }
            tx.commit().await.map_err(sqlite_err)?;
            return Ok(LegacyArchive {
                locator,
                sequence: saved_seq,
                state,
                confirmed_seq: confirmed_seq
                    .map(u64::try_from)
                    .transpose()
                    .map_err(|_| corrupt())?,
                old_body,
            });
        }
        let body = old_body.as_ref().ok_or_else(corrupt)?;
        let hash = body.digest();
        let epoch = crate::storage::anyseek::operation_epoch()?;
        let state = if source_state == "retracted" {
            "withdrawn"
        } else {
            "pending"
        };
        if stage {
            let locator = ArchiveLocator {
                domain,
                session,
                turn,
                source,
                body_hash: hash.clone(),
                epoch,
            };
            let request_hash: String = sqlite_db::query_scalar("SELECT request_hash FROM oc_session_turns WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=?")
                .bind(scope.tenant_id).bind(scope.workspace_id).bind(session).bind(turn)
                .fetch_one(&mut *tx).await.map_err(sqlite_err)?;
            let refs:Vec<String> = sqlite_db::query_scalar("SELECT evidence FROM oc_session_turn_evidence WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=? ORDER BY ordinal")
                .bind(scope.tenant_id).bind(scope.workspace_id).bind(session).bind(turn)
                .fetch_all(&mut *tx).await.map_err(sqlite_err)?;
            let used_evidence = refs
                .iter()
                .map(|value| Ok(decode::<crate::session::ArchivedEvidence>(value)?.reference))
                .collect::<StorageResult<Vec<_>>>()?;
            let input = crate::session::SessionTurnInput {
                turn_id: turn,
                question: body.question.clone(),
                answer: body.answer.clone(),
                used_evidence,
                evidence_use: body.evidence_use,
            };
            if request_hash != super::hash(json!({"session_id":session,"input":input}).to_string())
            {
                return Err(corrupt());
            }
            sqlite_db::query("INSERT INTO oc_private_archives(tenant_id,workspace_id,logical_key_id,session_id,turn_id,source_event_id,group_id,body_hash,body_bytes,operation_epoch,evidence_reported,turn_sequence,state,cleanup_done) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
                .bind(scope.tenant_id).bind(scope.workspace_id).bind(domain.owner).bind(session).bind(turn).bind(source)
                .bind(locator.group()).bind(&hash).bind(body.model_json_bytes() as i64).bind(epoch as i64)
                .bind(body.evidence_use != EvidenceUse::Unknown).bind(sequence).bind(state).bind(state == "withdrawn")
                .execute(&mut *tx).await.map_err(sqlite_err)?;
        }
        let state = if stage { state } else { "legacy" }.to_owned();
        tx.commit().await.map_err(sqlite_err)?;
        Ok(LegacyArchive {
            locator: ArchiveLocator {
                domain,
                session,
                turn,
                source,
                body_hash: hash,
                epoch,
            },
            sequence,
            state,
            confirmed_seq: None,
            old_body,
        })
    }

    pub(crate) async fn confirm_migration_archive(
        &self,
        archive: &LegacyArchive,
        seq: u64,
    ) -> StorageResult<()> {
        let l = &archive.locator;
        let affected = sqlite_db::query("UPDATE oc_private_archives SET state='archived',confirmed_seq=? WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=? AND state='pending' AND body_hash=?")
            .bind(i64::try_from(seq).map_err(|_| corrupt())?).bind(l.domain.scope.tenant_id).bind(l.domain.scope.workspace_id)
            .bind(l.session).bind(l.turn).bind(&l.body_hash).execute(&self.pool).await.map_err(sqlite_err)?.rows_affected();
        if affected != 1 {
            return Err(corrupt());
        }
        Ok(())
    }

    pub(crate) async fn purge_legacy_archives(&self) -> StorageResult<()> {
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(sqlite_err)?;
        let phase: String =
            sqlite_db::query_scalar("SELECT phase FROM oc_private_archive_migration WHERE id=1")
                .fetch_one(&mut *tx)
                .await
                .map_err(sqlite_err)?;
        if phase == "complete" {
            return Ok(());
        }
        if phase == "copying" {
            let secure: i64 = sqlite_db::query_scalar("PRAGMA secure_delete=ON")
                .fetch_one(&mut *tx)
                .await
                .map_err(sqlite_err)?;
            if secure != 1 {
                return Err(corrupt());
            }
            purge_replay(&mut tx).await?;
            purge_improve(&mut tx).await?;
            sqlite_db::query(
                "UPDATE oc_events SET content='' WHERE kind='session_turn' AND content<>''",
            )
            .execute(&mut *tx)
            .await
            .map_err(sqlite_err)?;
            sqlite_db::query("UPDATE oc_private_archive_migration SET phase='purging' WHERE id=1")
                .execute(&mut *tx)
                .await
                .map_err(sqlite_err)?;
        }
        tx.commit().await.map_err(sqlite_err)?;
        let mut conn = self.pool.acquire().await.map_err(sqlite_err)?;
        sqlite_db::query("VACUUM")
            .execute(&mut *conn)
            .await
            .map_err(sqlite_err)?;
        let (busy, _, _): (i64, i64, i64) = sqlite_db::query_as("PRAGMA wal_checkpoint(TRUNCATE)")
            .fetch_one(&mut *conn)
            .await
            .map_err(sqlite_err)?;
        if busy != 0 {
            return Err(StorageError::TransientUnavailable(
                "private archive SQLite checkpoint was busy".into(),
            ));
        }
        sqlite_db::query("UPDATE oc_private_archive_migration SET phase='complete' WHERE id=1")
            .execute(&mut *conn)
            .await
            .map_err(sqlite_err)?;
        Ok(())
    }
}

async fn purge_improve(tx: &mut Connection) -> StorageResult<()> {
    let runs: Vec<(Uuid, Uuid, Uuid, String, Option<String>)> = sqlite_db::query_as(
        "SELECT tenant_id,workspace_id,improve_id,snapshot,proposal_plan FROM oc_improve_runs",
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(sqlite_err)?;
    for (tenant, workspace, improve, snapshot, proposal) in runs {
        let mut snapshot: crate::learning::FrozenSnapshot = decode(&snapshot)?;
        for item in &mut snapshot.items {
            item.turn.question.clear();
            item.turn.answer.clear();
        }
        let proposal = proposal
            .map(|value| -> StorageResult<_> {
                let mut plan: Value = decode(&value)?;
                for candidate in plan["candidates"].as_array_mut().ok_or_else(corrupt)? {
                    hash_support_quotes(candidate)?;
                }
                encode(&plan)
            })
            .transpose()?;
        sqlite_db::query("UPDATE oc_improve_runs SET snapshot=?,proposal_plan=? WHERE tenant_id=? AND workspace_id=? AND improve_id=?")
            .bind(encode(&snapshot)?).bind(proposal).bind(tenant).bind(workspace).bind(improve)
            .execute(&mut *tx).await.map_err(sqlite_err)?;
    }
    let learnings: Vec<(Uuid, Uuid, Uuid, String)> =
        sqlite_db::query_as("SELECT tenant_id,workspace_id,id,candidate FROM oc_learnings")
            .fetch_all(&mut *tx)
            .await
            .map_err(sqlite_err)?;
    for (tenant, workspace, id, candidate) in learnings {
        let mut candidate: Value = decode(&candidate)?;
        hash_support_quotes(&mut candidate)?;
        sqlite_db::query(
            "UPDATE oc_learnings SET candidate=? WHERE tenant_id=? AND workspace_id=? AND id=?",
        )
        .bind(encode(&candidate)?)
        .bind(tenant)
        .bind(workspace)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(sqlite_err)?;
    }
    Ok(())
}

fn hash_support_quotes(candidate: &mut Value) -> StorageResult<()> {
    for support in candidate["supports"].as_array_mut().ok_or_else(corrupt)? {
        let quote = support["quote"].as_str().ok_or_else(corrupt)?;
        support["quote"] = json!(hash(quote));
    }
    Ok(())
}

async fn purge_replay(tx: &mut Connection) -> StorageResult<()> {
    let mut updated = Vec::new();
    let rows: Vec<(Uuid, Uuid, Uuid, i64, String)> = sqlite_db::query_as("SELECT tenant_id,workspace_id,run_id,sequence,payload FROM oc_run_events WHERE json_extract(payload,'$.kind')='context_prepared'")
        .fetch_all(&mut *tx).await.map_err(sqlite_err)?;
    for (tenant, workspace, run, sequence, payload) in rows {
        let mut payload: Value = decode(&payload)?;
        if runs::redact_archive_replay(&mut payload)? {
            updated.push((tenant, workspace, run, sequence, encode(&payload)?));
        }
    }
    if updated.is_empty() {
        return Ok(());
    }

    let mut refs = std::collections::BTreeMap::<(Uuid, Uuid, Uuid), BTreeSet<i64>>::new();
    let verifications: Vec<(Uuid, Uuid, Uuid, String)> = sqlite_db::query_as(
        "SELECT tenant_id,workspace_id,run_id,payload FROM oc_run_verifications",
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(sqlite_err)?;
    for (tenant, workspace, run, payload) in verifications {
        let v: crate::run_verification::RunVerificationInput = decode(&payload)?;
        refs.entry((tenant, workspace, run))
            .or_default()
            .insert(v.through_sequence);
    }
    let skills: Vec<(Uuid, Uuid, String)> =
        sqlite_db::query_as("SELECT tenant_id,workspace_id,content FROM oc_skill_revisions")
            .fetch_all(&mut *tx)
            .await
            .map_err(sqlite_err)?;
    for (tenant, workspace, content) in skills {
        let input: crate::skill::SkillCandidateInput = decode(&content)?;
        save_skill_bound_prefixes(&mut refs, tenant, workspace, input.sources);
    }
    let releases: Vec<(Uuid, Uuid, String)> =
        sqlite_db::query_as("SELECT tenant_id,workspace_id,sources FROM oc_skill_packages")
            .fetch_all(&mut *tx)
            .await
            .map_err(sqlite_err)?;
    for (tenant, workspace, sources) in releases {
        save_skill_bound_prefixes(&mut refs, tenant, workspace, decode(&sources)?);
    }
    let affected: BTreeSet<PrefixRow> = updated
        .iter()
        .flat_map(|(tenant, workspace, run, sequence, _)| {
            refs.get(&(*tenant, *workspace, *run))
                .into_iter()
                .flat_map(move |seqs| {
                    seqs.range(*sequence..)
                        .map(move |seq| (*tenant, *workspace, *run, *seq))
                })
        })
        .collect();
    for (tenant, workspace, run, through) in affected {
        let input: String = sqlite_db::query_scalar(
            "SELECT input FROM oc_runs WHERE tenant_id=? AND workspace_id=? AND run_id=?",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(run)
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlite_err)?;
        let rows:Vec<(i64,String,String,String)>=sqlite_db::query_as("SELECT sequence,payload_hash,references_json,payload FROM oc_run_events WHERE tenant_id=? AND workspace_id=? AND run_id=? AND sequence<=? ORDER BY sequence")
            .bind(tenant).bind(workspace).bind(run).bind(through).fetch_all(&mut *tx).await.map_err(sqlite_err)?;
        if rows.len() != through as usize {
            continue;
        }
        let digest = hash(encode(&(tenant, workspace, run, input, rows))?);
        sqlite_db::query("INSERT INTO oc_archive_legacy_run_prefixes(tenant_id,workspace_id,run_id,through_sequence,digest) VALUES(?,?,?,?,?)")
            .bind(tenant).bind(workspace).bind(run).bind(through).bind(digest).execute(&mut *tx).await.map_err(sqlite_err)?;
    }
    for (tenant, workspace, run, sequence, payload) in updated {
        sqlite_db::query("UPDATE oc_run_events SET payload=?,payload_hash=? WHERE tenant_id=? AND workspace_id=? AND run_id=? AND sequence=?")
            .bind(&payload).bind(hash(&payload)).bind(tenant).bind(workspace).bind(run).bind(sequence)
            .execute(&mut *tx).await.map_err(sqlite_err)?;
    }
    Ok(())
}

fn save_skill_bound_prefixes(
    refs: &mut std::collections::BTreeMap<(Uuid, Uuid, Uuid), BTreeSet<i64>>,
    tenant: Uuid,
    workspace: Uuid,
    sources: Vec<crate::skill::SkillSource>,
) {
    for source in sources {
        if let crate::skill::SkillSource::Run {
            run_id,
            through_sequence,
            ..
        } = source
        {
            refs.entry((tenant, workspace, run_id))
                .or_default()
                .insert(through_sequence);
        }
    }
}
