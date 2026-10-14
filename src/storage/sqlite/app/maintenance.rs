//! Scoped, transaction-local maintenance associations. The caller retains the
//! two authority transactions on opposite sides of native derived-object IO.
use super::*;
use crate::storage::sqlite::read_model::{self, AssociationKey};
use std::collections::HashSet;

#[cfg(test)]
mod tests;

const LIVE_VERSIONS: &str = "SELECT asset_id,source_event_id,(source_event_id=? AND version=?) AS m0,(source_event_id=? AND version=?) AS m1,(source_event_id=? AND version=?) AS m2,(source_event_id=? AND version=?) AS m3,(source_event_id=? AND version=?) AS m4,(source_event_id=? AND version=?) AS m5,(source_event_id=? AND version=?) AS m6,(source_event_id=? AND version=?) AS m7 FROM oc_versions WHERE tenant_id=? AND workspace_id=? AND (1) AND ((source_event_id=? AND version=?) OR (source_event_id=? AND version=?) OR (source_event_id=? AND version=?) OR (source_event_id=? AND version=?) OR (source_event_id=? AND version=?) OR (source_event_id=? AND version=?) OR (source_event_id=? AND version=?) OR (source_event_id=? AND version=?))";

const LIVE_ASSETS: &str = "SELECT id,(id=?) AS m0,(id=?) AS m1,(id=?) AS m2,(id=?) AS m3,(id=?) AS m4,(id=?) AS m5,(id=?) AS m6,(id=?) AS m7 FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND (NOT deleted) AND ((id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?))";

const ACTIVE_EVENTS: &str = "SELECT id,(id=?) AS m0,(id=?) AS m1,(id=?) AS m2,(id=?) AS m3,(id=?) AS m4,(id=?) AS m5,(id=?) AS m6,(id=?) AS m7 FROM oc_events WHERE tenant_id=? AND workspace_id=? AND (state='active') AND ((id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?))";

const OWNER_CHUNKS: &str = "SELECT asset_id,(id=?) AS m0,(id=?) AS m1,(id=?) AS m2,(id=?) AS m3,(id=?) AS m4,(id=?) AS m5,(id=?) AS m6,(id=?) AS m7 FROM oc_chunks WHERE tenant_id=? AND workspace_id=? AND (1) AND ((id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?))";

const CHUNK_OWNERS: &str = "SELECT artifact_id,(artifact_id=?) AS m0,(artifact_id=?) AS m1,(artifact_id=?) AS m2,(artifact_id=?) AS m3,(artifact_id=?) AS m4,(artifact_id=?) AS m5,(artifact_id=?) AS m6,(artifact_id=?) AS m7 FROM oc_artifact_owners WHERE tenant_id=? AND workspace_id=? AND (artifact_type='chunk') AND ((artifact_id=?) OR (artifact_id=?) OR (artifact_id=?) OR (artifact_id=?) OR (artifact_id=?) OR (artifact_id=?) OR (artifact_id=?) OR (artifact_id=?))";

const PRESENT_CHUNKS: &str = "SELECT id,(id=?) AS m0,(id=?) AS m1,(id=?) AS m2,(id=?) AS m3,(id=?) AS m4,(id=?) AS m5,(id=?) AS m6,(id=?) AS m7 FROM oc_chunks WHERE tenant_id=? AND workspace_id=? AND (1) AND ((id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?))";

const EXACT_OWNERS: &str = "SELECT artifact_id,(source_id=? AND version=? AND artifact_id=?) AS m0,(source_id=? AND version=? AND artifact_id=?) AS m1,(source_id=? AND version=? AND artifact_id=?) AS m2,(source_id=? AND version=? AND artifact_id=?) AS m3,(source_id=? AND version=? AND artifact_id=?) AS m4,(source_id=? AND version=? AND artifact_id=?) AS m5,(source_id=? AND version=? AND artifact_id=?) AS m6,(source_id=? AND version=? AND artifact_id=?) AS m7 FROM oc_artifact_owners WHERE tenant_id=? AND workspace_id=? AND (1) AND ((source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?))";

const DELETE_OWNERS: &str = "DELETE FROM oc_artifact_owners WHERE tenant_id=? AND workspace_id=? AND rowid IN (?,?,?,?,?,?,?,?)";

const DELETE_CHUNKS: &str =
    "DELETE FROM oc_chunks WHERE tenant_id=? AND workspace_id=? AND rowid IN (?,?,?,?,?,?,?,?)";

const REMOVE_INDEXES: &str = "UPDATE oc_index_entries SET state='removed' WHERE tenant_id=? AND workspace_id=? AND rowid IN (?,?,?,?,?,?,?,?)";

const ORPHAN_LEDGER: &str = "UPDATE oc_artifact_ledger SET state='orphan',updated_at=? WHERE tenant_id=? AND workspace_id=? AND rowid IN (?,?,?,?,?,?,?,?)";

fn keys(rows: &[native_turso::Row], columns: &[&str]) -> StorageResult<Vec<Vec<AssociationKey>>> {
    rows.iter()
        .map(|row| {
            columns
                .iter()
                .map(|column| row.try_get(*column).map_err(turso_err))
                .collect()
        })
        .collect()
}

fn unmatched_rowids(
    rows: &[native_turso::Row],
    matched: &HashSet<usize>,
) -> StorageResult<Vec<i64>> {
    rows.iter()
        .enumerate()
        .filter(|(index, _)| !matched.contains(index))
        .map(|(_, row)| row.try_get("cleanup_rowid").map_err(turso_err))
        .collect()
}

/// Fresh-schema maintenance tables are ordinary rowid tables. These locators
/// are never business keys or persisted: scope and the caller's write snapshot
/// bind every mutation to the exact row whose predicates were just evaluated.
async fn mutate_rowids(
    conn: &mut Connection,
    scope: Scope,
    sql: &str,
    ids: &[i64],
    timestamp: Option<i64>,
) -> StorageResult<u64> {
    let mut changed = 0;
    for index in 0..ids.len().div_ceil(8).max(1) {
        let start = index * 8;
        let batch = &ids[start..(start + 8).min(ids.len())];
        let mut query = native_turso::query(sql);
        if let Some(timestamp) = timestamp {
            query = query.bind(timestamp);
        }
        query = query.bind(scope.tenant_id).bind(scope.workspace_id);
        for slot in 0..8 {
            query = query.bind(batch.get(slot).copied());
        }
        changed += query
            .execute(&mut *conn)
            .await
            .map_err(turso_err)?
            .rows_affected();
    }
    Ok(changed)
}

pub(super) async fn detach_invalid_roots(
    conn: &mut Connection,
    scope: Scope,
) -> StorageResult<u64> {
    let owners = native_turso::query("SELECT rowid AS cleanup_rowid,source_id,version FROM oc_artifact_owners WHERE tenant_id=? AND workspace_id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .fetch_all(&mut *conn)
        .await
        .map_err(turso_err)?;
    let versions = read_model::associate_query(
        conn,
        scope,
        LIVE_VERSIONS,
        2,
        &keys(&owners, &["source_id", "version"])?,
    )
    .await?;
    let asset_keys = versions
        .iter()
        .map(|(_, row)| Ok(vec![row.try_get("asset_id").map_err(turso_err)?]))
        .collect::<StorageResult<Vec<_>>>()?;
    let event_keys = versions
        .iter()
        .map(|(_, row)| Ok(vec![row.try_get("source_event_id").map_err(turso_err)?]))
        .collect::<StorageResult<Vec<_>>>()?;
    let assets: HashSet<_> = read_model::associate_query(conn, scope, LIVE_ASSETS, 1, &asset_keys)
        .await?
        .into_iter()
        .map(|(index, _)| index)
        .collect();
    let events: HashSet<_> =
        read_model::associate_query(conn, scope, ACTIVE_EVENTS, 1, &event_keys)
            .await?
            .into_iter()
            .map(|(index, _)| index)
            .collect();
    let live: HashSet<_> = versions
        .iter()
        .enumerate()
        .filter(|(index, _)| assets.contains(index) && events.contains(index))
        .map(|(_, (owner, _))| *owner)
        .collect();
    mutate_rowids(
        conn,
        scope,
        DELETE_OWNERS,
        &unmatched_rowids(&owners, &live)?,
        None,
    )
    .await
}

pub(super) async fn detach_invalid_chunks(
    conn: &mut Connection,
    scope: Scope,
) -> StorageResult<u64> {
    let owners = native_turso::query("SELECT rowid AS cleanup_rowid,chunk_id FROM oc_artifact_owners WHERE tenant_id=? AND workspace_id=? AND artifact_type IN ('chunk','summary')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .fetch_all(&mut *conn)
        .await
        .map_err(turso_err)?;
    let chunks =
        read_model::associate_query(conn, scope, OWNER_CHUNKS, 1, &keys(&owners, &["chunk_id"])?)
            .await?;
    let asset_keys = chunks
        .iter()
        .map(|(_, row)| Ok(vec![row.try_get("asset_id").map_err(turso_err)?]))
        .collect::<StorageResult<Vec<_>>>()?;
    let assets = read_model::associate_query(conn, scope, LIVE_ASSETS, 1, &asset_keys).await?;
    let live: HashSet<_> = assets
        .into_iter()
        .map(|(index, _)| chunks[index].0)
        .collect();
    mutate_rowids(
        conn,
        scope,
        DELETE_OWNERS,
        &unmatched_rowids(&owners, &live)?,
        None,
    )
    .await
}

pub(super) async fn unowned_native_artifacts(
    conn: &mut Connection,
    scope: Scope,
) -> StorageResult<Vec<Uuid>> {
    let ledger = native_turso::query("SELECT DISTINCT artifact_id FROM oc_artifact_ledger WHERE tenant_id=? AND workspace_id=? AND surface IN ('vector','full_text')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .fetch_all(&mut *conn)
        .await
        .map_err(turso_err)?;
    let owned: HashSet<_> = read_model::associate_query(
        conn,
        scope,
        CHUNK_OWNERS,
        1,
        &keys(&ledger, &["artifact_id"])?,
    )
    .await?
    .into_iter()
    .map(|(index, _)| index)
    .collect();
    ledger
        .iter()
        .enumerate()
        .filter(|(index, _)| !owned.contains(index))
        .map(|(_, row)| row.try_get("artifact_id").map_err(turso_err))
        .collect()
}

pub(super) async fn remove_unowned_chunks(
    conn: &mut Connection,
    scope: Scope,
) -> StorageResult<u64> {
    let chunks = native_turso::query(
        "SELECT rowid AS cleanup_rowid,id FROM oc_chunks WHERE tenant_id=? AND workspace_id=?",
    )
    .bind(scope.tenant_id)
    .bind(scope.workspace_id)
    .fetch_all(&mut *conn)
    .await
    .map_err(turso_err)?;
    let owned: HashSet<_> =
        read_model::associate_query(conn, scope, CHUNK_OWNERS, 1, &keys(&chunks, &["id"])?)
            .await?
            .into_iter()
            .map(|(index, _)| index)
            .collect();
    let dead_keys: Vec<AssociationKey> = chunks
        .iter()
        .enumerate()
        .filter(|(index, _)| !owned.contains(index))
        .map(|(_, row)| row.try_get("id").map_err(turso_err))
        .collect::<StorageResult<_>>()?;
    // Delete every affected summary before any chunk, retaining the original
    // global FK-safe order. A summary with no chunk at all is not selected.
    for index in 0..dead_keys.len().div_ceil(8).max(1) {
        let start = index * 8;
        let batch = &dead_keys[start..(start + 8).min(dead_keys.len())];
        let mut query = native_turso::query("DELETE FROM oc_summaries WHERE tenant_id=? AND workspace_id=? AND chunk_id IN (?,?,?,?,?,?,?,?)")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id);
        for slot in 0..8 {
            query = query.bind(batch.get(slot).cloned());
        }
        query.execute(&mut *conn).await.map_err(turso_err)?;
    }
    mutate_rowids(
        conn,
        scope,
        DELETE_CHUNKS,
        &unmatched_rowids(&chunks, &owned)?,
        None,
    )
    .await
}

pub(super) async fn remove_missing_indexes(
    conn: &mut Connection,
    scope: Scope,
) -> StorageResult<u64> {
    let rows = native_turso::query("SELECT rowid AS cleanup_rowid,artifact_id FROM oc_index_entries WHERE tenant_id=? AND workspace_id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .fetch_all(&mut *conn)
        .await
        .map_err(turso_err)?;
    let present: HashSet<_> = read_model::associate_query(
        conn,
        scope,
        PRESENT_CHUNKS,
        1,
        &keys(&rows, &["artifact_id"])?,
    )
    .await?
    .into_iter()
    .map(|(index, _)| index)
    .collect();
    mutate_rowids(
        conn,
        scope,
        REMOVE_INDEXES,
        &unmatched_rowids(&rows, &present)?,
        None,
    )
    .await
}

pub(super) async fn orphan_unowned_ledger(
    conn: &mut Connection,
    scope: Scope,
) -> StorageResult<u64> {
    let now = now_ms();
    let rows = native_turso::query("SELECT rowid AS cleanup_rowid,source_id,version,artifact_id FROM oc_artifact_ledger WHERE tenant_id=? AND workspace_id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .fetch_all(&mut *conn)
        .await
        .map_err(turso_err)?;
    // No type, surface or generation condition belonged to this owner match.
    let owned: HashSet<_> = read_model::associate_query(
        conn,
        scope,
        EXACT_OWNERS,
        3,
        &keys(&rows, &["source_id", "version", "artifact_id"])?,
    )
    .await?
    .into_iter()
    .map(|(index, _)| index)
    .collect();
    mutate_rowids(
        conn,
        scope,
        ORPHAN_LEDGER,
        &unmatched_rowids(&rows, &owned)?,
        Some(now),
    )
    .await
}
