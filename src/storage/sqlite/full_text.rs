//! Authority checks for the derived Lance full-text surface.
//! Retrieval is staged: metadata eligibility, native search, exact source and
//! generation verification. Native rows never grant visibility.
use super::app::hit;
use super::visibility::{CHUNK_SUPPORT_ACTIVE, CURRENT_CHUNK_ROWS};
use super::*;
use crate::storage::native_turso as db;
use crate::types::SearchHit;

impl TursoStore {
    /// Root-active current publication metadata, not a visibility decision:
    /// semantic/support closure remains the retrieval transaction's job. Native
    /// availability is checked after this query releases its lease.
    pub async fn has_committed_full_text(&self) -> StorageResult<bool> {
        db::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_chunks c JOIN oc_assets a ON a.tenant_id=c.tenant_id AND a.workspace_id=c.workspace_id AND a.id=c.asset_id AND a.current_version=c.version JOIN oc_versions v ON v.tenant_id=c.tenant_id AND v.workspace_id=c.workspace_id AND v.asset_id=c.asset_id AND v.version=c.version JOIN oc_events e ON e.tenant_id=v.tenant_id AND e.workspace_id=v.workspace_id AND e.id=v.source_event_id JOIN oc_artifact_ledger l ON l.tenant_id=c.tenant_id AND l.workspace_id=c.workspace_id AND l.artifact_id=c.id AND l.artifact_type='chunk' AND l.source_id=v.source_event_id AND l.version=v.version AND l.surface='full_text' AND l.state='committed' WHERE NOT a.deleted AND e.state='active')")
            .fetch_one(self.pool()).await.map_err(turso_err)
    }

    /// Required profile schemas only; no content, embeddings or support graph
    /// is loaded. Withdrawn/superseded roots do not prevent startup cleanup.
    pub async fn committed_vector_profiles(&self) -> StorageResult<Vec<(String, usize)>> {
        let rows: Vec<(String, i64)> = db::query_as("SELECT DISTINCT i.model_id,i.dimension FROM oc_index_entries i JOIN oc_chunks c ON c.tenant_id=i.tenant_id AND c.workspace_id=i.workspace_id AND c.id=i.artifact_id JOIN oc_assets a ON a.tenant_id=c.tenant_id AND a.workspace_id=c.workspace_id AND a.id=c.asset_id AND a.current_version=c.version JOIN oc_versions v ON v.tenant_id=c.tenant_id AND v.workspace_id=c.workspace_id AND v.asset_id=c.asset_id AND v.version=c.version JOIN oc_events e ON e.tenant_id=v.tenant_id AND e.workspace_id=v.workspace_id AND e.id=v.source_event_id JOIN oc_artifact_ledger l ON l.tenant_id=c.tenant_id AND l.workspace_id=c.workspace_id AND l.artifact_id=c.id AND l.artifact_type='chunk' AND l.source_id=v.source_event_id AND l.version=v.version AND l.generation=i.generation AND l.surface='vector' AND l.state='committed' WHERE i.field='content' AND i.state='ready' AND NOT a.deleted AND e.state='active' ORDER BY i.model_id,i.dimension")
            .fetch_all(self.pool()).await.map_err(turso_err)?;
        rows.into_iter().map(|(profile, dimension)| {
            if !(1..=i64::from(i32::MAX)).contains(&dimension) {
                return Err(StorageError::Conflict("invalid committed vector dimension".into()));
            }
            Ok((profile, dimension as usize))
        }).collect()
    }
}

impl TursoTx {
    /// Exact root identity for already-eligible IDs, in the same authority snapshot.
    /// The native metadata preflight compares every row before ranking.
    pub async fn native_candidate_sources(
        &mut self,
        ids: &[Uuid],
    ) -> StorageResult<HashMap<Uuid, SourceVersion>> {
        let wanted: std::collections::HashSet<_> = ids.iter().copied().collect();
        if wanted.is_empty() {
            return Ok(HashMap::new());
        }
        let rows: Vec<(Uuid, Uuid, i32)> = db::query_as(&format!(
            "SELECT c.id,v.source_event_id,v.version {CURRENT_CHUNK_ROWS} AND {CHUNK_SUPPORT_ACTIVE}"))
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id)
            .fetch_all(&mut *self.tx).await.map_err(turso_err)?;
        let sources: HashMap<_, _> = rows
            .into_iter()
            .filter(|(id, _, _)| wanted.contains(id))
            .map(|(id, source_id, version)| (id, SourceVersion { source_id, version }))
            .collect();
        if sources.len() != wanted.len() {
            return Err(StorageError::Unavailable(
                "inconsistent authoritative candidates".into(),
            ));
        }
        Ok(sources)
    }

    pub async fn full_text_generations(&mut self) -> StorageResult<Vec<i64>> {
        db::query_scalar("SELECT DISTINCT generation FROM oc_artifact_ledger WHERE tenant_id=? AND workspace_id=? AND surface='full_text' AND state='committed'")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id)
            .fetch_all(&mut *self.tx).await.map_err(turso_err)
    }

    pub async fn full_text_candidates(
        &mut self,
        generation: i64,
        asset_filter: Option<Uuid>,
    ) -> StorageResult<Vec<Uuid>> {
        let rows: Vec<(Uuid, Uuid, i32)> = db::query_as(&format!(
            "SELECT c.id,c.asset_id,c.version {CURRENT_CHUNK_ROWS} AND {CHUNK_SUPPORT_ACTIVE} AND EXISTS(SELECT 1 FROM oc_artifact_ledger l WHERE l.tenant_id=c.tenant_id AND l.workspace_id=c.workspace_id AND l.artifact_id=c.id AND l.source_id=v.source_event_id AND l.version=v.version AND l.generation=? AND l.surface='full_text' AND l.state='committed')"))
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(generation)
            .fetch_all(&mut *self.tx).await.map_err(turso_err)?;
        let mut visibility = HashMap::new();
        let mut ids = Vec::new();
        for (id, asset, version) in rows {
            if asset_filter.is_some_and(|expected| asset != expected) {
                continue;
            }
            let visible = if let Some(visible) = visibility.get(&(asset, version)) {
                *visible
            } else {
                let visible = self.semantic_version_visible(asset, version).await?;
                visibility.insert((asset, version), visible);
                visible
            };
            if visible {
                ids.push(id);
            }
        }
        Ok(ids)
    }

    pub async fn full_text_hit(
        &mut self,
        id: Uuid,
        source: SourceVersion,
        generation: i64,
    ) -> StorageResult<Option<SearchHit>> {
        let row = db::query(&format!(
            "SELECT c.id,c.asset_id,c.version,a.kind,v.title,c.content,c.locator,v.source_event_id,c.search_terms {CURRENT_CHUNK_ROWS} AND {CHUNK_SUPPORT_ACTIVE} AND c.id=? AND v.source_event_id=? AND v.version=? AND EXISTS(SELECT 1 FROM oc_artifact_ledger l WHERE l.tenant_id=c.tenant_id AND l.workspace_id=c.workspace_id AND l.artifact_id=c.id AND l.source_id=v.source_event_id AND l.version=v.version AND l.generation=? AND l.surface='full_text' AND l.state='committed')"))
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(id).bind(source.source_id).bind(source.version).bind(generation)
            .fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        let Some(row) = row else {
            return Ok(None);
        };
        if !self
            .semantic_version_visible(row.get("asset_id"), row.get("version"))
            .await?
        {
            return Ok(None);
        }
        Ok(Some(hit(&row, 0.0)))
    }

    #[cfg(feature = "local-storage")]
    pub(crate) async fn context_full_text_candidates(
        &mut self,
        generation: i64,
        route: crate::context_retrieval::ContextRoute,
        identity: Option<&crate::memory_identity::MemoryIdentity>,
    ) -> StorageResult<Vec<Uuid>> {
        let route_ids: std::collections::HashSet<_> = self
            .context_route_rows(route, identity)
            .await?
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        let mut ids = self.full_text_candidates(generation, None).await?;
        ids.retain(|id| route_ids.contains(id));
        Ok(ids)
    }
}
