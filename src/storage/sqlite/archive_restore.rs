//! Verify the archive's SQLite evidence and allocation counters from one snapshot.
use super::*;
use crate::session::ArchiveDomain;
use std::collections::BTreeSet;

pub(crate) struct NativeRestoreSession {
    pub(crate) domain: ArchiveDomain,
    pub(crate) session: Uuid,
    pub(crate) last_sequence: i64,
    pub(crate) registered: HashMap<Uuid, i64>,
    pub(crate) expected: HashMap<Uuid, (Uuid, String)>,
}

fn corrupt() -> StorageError {
    StorageError::Unavailable("private archive restore proof does not match".into())
}

async fn read_native_restore_sessions(
    tx: &mut Connection,
) -> StorageResult<HashMap<(Uuid, Uuid, Uuid), NativeRestoreSession>> {
    let rows: Vec<(Uuid, Uuid, Uuid, i64)> = sqlite_db::query_as(
        "SELECT tenant_id,workspace_id,session_id,last_sequence FROM oc_native_archive_sessions",
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(sqlite_err)?;
    let mut sessions = HashMap::with_capacity(rows.len());
    for (tenant_id, workspace_id, session, last_sequence) in rows {
        let key = (tenant_id, workspace_id, session);
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
                registered: HashMap::new(),
                expected: HashMap::new(),
            },
        );
    }
    let commands: Vec<(Uuid, Uuid, String, String, i64)> = sqlite_db::query_as(
        "SELECT DISTINCT tenant_id,workspace_id,json_extract(response,'$.session_id'),json_extract(response,'$.turn_id'),json_extract(response,'$.sequence') FROM oc_commands WHERE operation='session.turn.archive'",
    )
    .fetch_all(tx).await.map_err(sqlite_err)?;
    for (tenant, workspace, session, turn, sequence) in commands {
        let session = Uuid::parse_str(&session).map_err(|_| corrupt())?;
        let turn = Uuid::parse_str(&turn).map_err(|_| corrupt())?;
        let entry = sessions
            .get_mut(&(tenant, workspace, session))
            .ok_or_else(corrupt)?;
        if !(1..=entry.last_sequence).contains(&sequence)
            || entry.registered.insert(turn, sequence).is_some()
        {
            return Err(corrupt());
        }
    }
    Ok(sessions)
}

impl SqliteStore {
    pub(crate) async fn archive_restore_sessions(
        &self,
    ) -> StorageResult<Vec<NativeRestoreSession>> {
        let mut tx = self.pool.begin().await.map_err(sqlite_err)?;
        let rows: Vec<(Uuid, Uuid, Uuid, Uuid, Uuid, String)> = sqlite_db::query_as("SELECT tenant_id,workspace_id,session_id,turn_id,source_event_id,request_hash FROM oc_session_turns")
            .fetch_all(&mut *tx).await.map_err(sqlite_err)?;
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
        for (tenant_id, workspace_id, session, turn, source, request_hash) in rows {
            if !native_proofs.remove(&(tenant_id, workspace_id, session, turn)) {
                return Err(corrupt());
            }
            let entry = native
                .get_mut(&(tenant_id, workspace_id, session))
                .ok_or_else(corrupt)?;
            let (state, empty): (String, bool) = sqlite_db::query_as("SELECT state,content='' FROM oc_events WHERE tenant_id=? AND workspace_id=? AND id=? AND kind='session_turn' AND file_id IS NULL")
                    .bind(tenant_id).bind(workspace_id).bind(source)
                    .fetch_optional(&mut *tx).await.map_err(sqlite_err)?.ok_or_else(corrupt)?;
            if !empty || !crate::retrieval_diagnostics::digest(&request_hash) {
                return Err(corrupt());
            }
            match state.as_str() {
                "active" => {
                    entry.expected.insert(turn, (source, request_hash));
                }
                "retracted" => {}
                _ => return Err(corrupt()),
            }
        }
        if !native_proofs.is_empty() {
            return Err(corrupt());
        }
        tx.commit().await.map_err(sqlite_err)?;
        Ok(native.into_values().collect())
    }
}
