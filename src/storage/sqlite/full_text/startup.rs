//! Bounded startup metadata reads. Keep keys native until all SQL predicates
//! have passed; neither existence nor profile discovery reads content or JSON.
use super::super::read_model::AssociationKey;
use super::*;
use crate::storage::native_turso::{FromRow, Row};
use std::collections::BTreeSet;

const PAGE_SIZE: usize = 32;
const MATCH_SLOTS: usize = 4;

const CHUNKS: &str = "SELECT asset_id,
    (tenant_id=? AND workspace_id=? AND id=? AND version=?) AS m0,
    (tenant_id=? AND workspace_id=? AND id=? AND version=?) AS m1,
    (tenant_id=? AND workspace_id=? AND id=? AND version=?) AS m2,
    (tenant_id=? AND workspace_id=? AND id=? AND version=?) AS m3
    FROM oc_chunks WHERE 1 AND (
    (tenant_id=? AND workspace_id=? AND id=? AND version=?) OR
    (tenant_id=? AND workspace_id=? AND id=? AND version=?) OR
    (tenant_id=? AND workspace_id=? AND id=? AND version=?) OR
    (tenant_id=? AND workspace_id=? AND id=? AND version=?))";

const ASSETS: &str = "SELECT 1,
    (tenant_id=? AND workspace_id=? AND id=? AND current_version=?) AS m0,
    (tenant_id=? AND workspace_id=? AND id=? AND current_version=?) AS m1,
    (tenant_id=? AND workspace_id=? AND id=? AND current_version=?) AS m2,
    (tenant_id=? AND workspace_id=? AND id=? AND current_version=?) AS m3
    FROM oc_assets WHERE NOT deleted AND (
    (tenant_id=? AND workspace_id=? AND id=? AND current_version=?) OR
    (tenant_id=? AND workspace_id=? AND id=? AND current_version=?) OR
    (tenant_id=? AND workspace_id=? AND id=? AND current_version=?) OR
    (tenant_id=? AND workspace_id=? AND id=? AND current_version=?))";

const VERSIONS: &str = "SELECT 1,
    (tenant_id=? AND workspace_id=? AND asset_id=? AND version=? AND source_event_id=?) AS m0,
    (tenant_id=? AND workspace_id=? AND asset_id=? AND version=? AND source_event_id=?) AS m1,
    (tenant_id=? AND workspace_id=? AND asset_id=? AND version=? AND source_event_id=?) AS m2,
    (tenant_id=? AND workspace_id=? AND asset_id=? AND version=? AND source_event_id=?) AS m3
    FROM oc_versions WHERE 1 AND (
    (tenant_id=? AND workspace_id=? AND asset_id=? AND version=? AND source_event_id=?) OR
    (tenant_id=? AND workspace_id=? AND asset_id=? AND version=? AND source_event_id=?) OR
    (tenant_id=? AND workspace_id=? AND asset_id=? AND version=? AND source_event_id=?) OR
    (tenant_id=? AND workspace_id=? AND asset_id=? AND version=? AND source_event_id=?))";

const EVENTS: &str = "SELECT 1,
    (tenant_id=? AND workspace_id=? AND id=?) AS m0,
    (tenant_id=? AND workspace_id=? AND id=?) AS m1,
    (tenant_id=? AND workspace_id=? AND id=?) AS m2,
    (tenant_id=? AND workspace_id=? AND id=?) AS m3
    FROM oc_events WHERE state='active' AND (
    (tenant_id=? AND workspace_id=? AND id=?) OR
    (tenant_id=? AND workspace_id=? AND id=?) OR
    (tenant_id=? AND workspace_id=? AND id=?) OR
    (tenant_id=? AND workspace_id=? AND id=?))";

const INDEXES: &str = "SELECT model_id,dimension,
    (tenant_id=? AND workspace_id=? AND artifact_id=? AND generation=?) AS m0,
    (tenant_id=? AND workspace_id=? AND artifact_id=? AND generation=?) AS m1,
    (tenant_id=? AND workspace_id=? AND artifact_id=? AND generation=?) AS m2,
    (tenant_id=? AND workspace_id=? AND artifact_id=? AND generation=?) AS m3
    FROM oc_index_entries WHERE field='content' AND state='ready' AND (
    (tenant_id=? AND workspace_id=? AND artifact_id=? AND generation=?) OR
    (tenant_id=? AND workspace_id=? AND artifact_id=? AND generation=?) OR
    (tenant_id=? AND workspace_id=? AND artifact_id=? AND generation=?) OR
    (tenant_id=? AND workspace_id=? AND artifact_id=? AND generation=?))";

// Unlike scoped retrieval, startup ranges over all workspaces. Scope values
// therefore remain part of each native request; converting them to UUID here
// would make an excluded malformed row fail an otherwise valid EXISTS read.
async fn match_rows(
    conn: &mut Connection,
    sql: &str,
    width: usize,
    requests: &[Vec<AssociationKey>],
) -> StorageResult<Vec<(usize, Row)>> {
    let mut result = Vec::new();
    for batch_index in 0..requests.len().div_ceil(MATCH_SLOTS).max(1) {
        let start = batch_index * MATCH_SLOTS;
        let batch = &requests[start..(start + MATCH_SLOTS).min(requests.len())];
        let mut query = db::query(sql);
        for _ in 0..2 {
            for slot in 0..MATCH_SLOTS {
                for key in 0..width {
                    query = query.bind(batch.get(slot).map(|request| request[key].clone()));
                }
            }
        }
        for row in query.fetch_all(&mut *conn).await.map_err(turso_err)? {
            for (slot, name) in ["m0", "m1", "m2", "m3"]
                .iter()
                .take(batch.len())
                .enumerate()
            {
                if row.try_get::<Option<bool>>(*name).map_err(turso_err)? == Some(true) {
                    result.push((start + slot, Row::from_row(&row).map_err(turso_err)?));
                }
            }
        }
    }
    Ok(result)
}

#[derive(Default)]
struct Pages {
    after: Option<i64>,
    #[cfg(test)]
    rows_read: usize,
    #[cfg(test)]
    pages_read: usize,
}

impl Pages {
    async fn next(&mut self, conn: &mut Connection, surface: &str) -> StorageResult<Vec<Row>> {
        let generation = if surface == "vector" {
            "generation"
        } else {
            "NULL AS generation"
        };
        let seek = if self.after.is_some() {
            "rowid>?"
        } else {
            "rowid>=?"
        };
        let sql = format!(
            "SELECT rowid AS startup_rowid,tenant_id,workspace_id,artifact_id,source_id,version,{generation} FROM oc_artifact_ledger WHERE {seek} AND artifact_type='chunk' AND surface=? AND state='committed' ORDER BY rowid LIMIT ?"
        );
        let rows = db::query(&sql)
            .bind(self.after.unwrap_or(i64::MIN))
            .bind(surface)
            .bind(PAGE_SIZE)
            .fetch_all(&mut *conn)
            .await
            .map_err(turso_err)?;
        if let Some(last) = rows.last() {
            self.after = Some(last.try_get("startup_rowid").map_err(turso_err)?);
        }
        #[cfg(test)]
        {
            self.rows_read += rows.len();
            self.pages_read += 1;
        }
        Ok(rows)
    }
}

struct Publication {
    tenant: AssociationKey,
    workspace: AssociationKey,
    artifact: AssociationKey,
    source: AssociationKey,
    version: AssociationKey,
    generation: AssociationKey,
}

fn publications(rows: &[Row]) -> StorageResult<Vec<Publication>> {
    rows.iter()
        .map(|row| {
            Ok(Publication {
                tenant: row.try_get("tenant_id").map_err(turso_err)?,
                workspace: row.try_get("workspace_id").map_err(turso_err)?,
                artifact: row.try_get("artifact_id").map_err(turso_err)?,
                source: row.try_get("source_id").map_err(turso_err)?,
                version: row.try_get("version").map_err(turso_err)?,
                generation: row.try_get("generation").map_err(turso_err)?,
            })
        })
        .collect()
}

async fn current_publications(
    conn: &mut Connection,
    publications: &[Publication],
) -> StorageResult<Vec<usize>> {
    let requests: Vec<_> = publications
        .iter()
        .map(|p| {
            vec![
                p.tenant.clone(),
                p.workspace.clone(),
                p.artifact.clone(),
                p.version.clone(),
            ]
        })
        .collect();
    let chunks = match_rows(conn, CHUNKS, 4, &requests).await?;
    let mut selected = Vec::new();
    for (index, chunk) in chunks {
        selected.push((
            index,
            chunk
                .try_get::<AssociationKey>("asset_id")
                .map_err(turso_err)?,
        ));
    }
    let requests: Vec<_> = selected
        .iter()
        .map(|(index, asset)| {
            let p = &publications[*index];
            vec![
                p.tenant.clone(),
                p.workspace.clone(),
                asset.clone(),
                p.version.clone(),
            ]
        })
        .collect();
    let assets = match_rows(conn, ASSETS, 4, &requests).await?;
    let requests: Vec<_> = assets
        .iter()
        .map(|(index, _)| {
            let (publication, asset) = &selected[*index];
            let p = &publications[*publication];
            vec![
                p.tenant.clone(),
                p.workspace.clone(),
                asset.clone(),
                p.version.clone(),
                p.source.clone(),
            ]
        })
        .collect();
    let versions = match_rows(conn, VERSIONS, 5, &requests).await?;
    let requests: Vec<_> = versions
        .iter()
        .map(|(index, _)| {
            let p = &publications[selected[assets[*index].0].0];
            vec![p.tenant.clone(), p.workspace.clone(), p.source.clone()]
        })
        .collect();
    // Empty requests still prepare every relation named by the former single
    // statement, so an empty earlier table cannot conceal a missing schema.
    let events = match_rows(conn, EVENTS, 3, &requests).await?;
    let mut current: Vec<_> = events
        .into_iter()
        .map(|(index, _)| selected[assets[versions[index].0].0].0)
        .collect();
    current.sort_unstable();
    current.dedup();
    Ok(current)
}

async fn full_text_in_pages(conn: &mut Connection, pages: &mut Pages) -> StorageResult<bool> {
    loop {
        let rows = pages.next(conn, "full_text").await?;
        if !current_publications(conn, &publications(&rows)?)
            .await?
            .is_empty()
        {
            return Ok(true);
        }
        if rows.len() < PAGE_SIZE {
            return Ok(false);
        }
    }
}

pub(super) async fn has_full_text(conn: &mut Connection) -> StorageResult<bool> {
    full_text_in_pages(conn, &mut Pages::default()).await
}

async fn profiles_in_pages(
    conn: &mut Connection,
    pages: &mut Pages,
) -> StorageResult<Vec<(String, usize)>> {
    let mut profiles: BTreeSet<(String, i64)> = BTreeSet::new();
    loop {
        let rows = pages.next(conn, "vector").await?;
        let publications = publications(&rows)?;
        let current = current_publications(conn, &publications).await?;
        let requests: Vec<_> = current
            .into_iter()
            .map(|index| {
                let p = &publications[index];
                vec![
                    p.tenant.clone(),
                    p.workspace.clone(),
                    p.artifact.clone(),
                    p.generation.clone(),
                ]
            })
            .collect();
        for (_, row) in match_rows(conn, INDEXES, 4, &requests).await? {
            profiles.insert((
                row.try_get("model_id").map_err(turso_err)?,
                row.try_get("dimension").map_err(turso_err)?,
            ));
        }
        if rows.len() < PAGE_SIZE {
            break;
        }
    }
    // Match the former fetch_all::<(String, i64)> phase: selected type errors
    // anywhere precede dimension validation; rejected index rows never decode.
    profiles
        .into_iter()
        .map(|(profile, dimension)| {
            if !(1..=i64::from(i32::MAX)).contains(&dimension) {
                return Err(StorageError::Conflict(
                    "invalid committed vector dimension".into(),
                ));
            }
            Ok((profile, dimension as usize))
        })
        .collect()
}

pub(super) async fn vector_profiles(conn: &mut Connection) -> StorageResult<Vec<(String, usize)>> {
    profiles_in_pages(conn, &mut Pages::default()).await
}

#[cfg(test)]
mod tests;
