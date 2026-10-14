//! Authority checks for the derived Lance full-text surface. Single-table
//! reads are associated inside one authority transaction before native ranking.
use super::read_model::{
    NativeHitKey, current_chunk_roots, current_chunks, unique_ids,
};
use super::*;
use crate::storage::native_turso as db;
use crate::types::SearchHit;
use std::collections::HashSet;

mod startup;

impl TursoStore {
    /// Root-active current publication metadata, not a visibility decision.
    pub async fn has_committed_full_text(&self) -> StorageResult<bool> {
        let mut tx = self.pool.begin().await.map_err(turso_err)?;
        let required = startup::has_full_text(&mut tx).await?;
        tx.rollback().await.map_err(turso_err)?;
        Ok(required)
    }

    /// Required profile schemas in one snapshot. Withdrawn or superseded roots
    /// do not prevent startup cleanup; no support graph is loaded here.
    pub async fn committed_vector_profiles(&self) -> StorageResult<Vec<(String, usize)>> {
        let mut tx = self.pool.begin().await.map_err(turso_err)?;
        let profiles = startup::vector_profiles(&mut tx).await?;
        tx.rollback().await.map_err(turso_err)?;
        Ok(profiles)
    }
}

impl TursoTx {
    /// Exact roots for already-eligible IDs in the same authority snapshot.
    pub async fn native_candidate_sources(
        &mut self,
        ids: &[Uuid],
    ) -> StorageResult<HashMap<Uuid, SourceVersion>> {
        let wanted = unique_ids(ids.iter().copied());
        let mut sources = current_chunk_roots(&mut self.tx, self.scope, &wanted).await?;
        let current_ids: Vec<_> = sources.keys().copied().collect();
        let support = self.local_chunk_support_visibility(&current_ids).await?;
        sources.retain(|id, _| support.get(id) == Some(&true));
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
        let ids: Vec<Uuid> = db::query_scalar("SELECT artifact_id FROM oc_artifact_ledger WHERE tenant_id=? AND workspace_id=? AND generation=? AND surface='full_text' AND state='committed'")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(generation)
            .fetch_all(&mut *self.tx).await.map_err(turso_err)?;
        let publications = self.committed_publications(&ids, "full_text").await?;
        let mut chunks = current_chunks(&mut self.tx, self.scope, Some(&ids), asset_filter).await?;
        chunks.retain(|chunk| publications.contains(&(chunk.hit.chunk_id, chunk.root, generation)));
        Ok(unique_ids(
            self.guard_retrieval_chunks(chunks)
                .await?
                .into_iter()
                .map(|chunk| chunk.hit.chunk_id),
        ))
    }

    pub(crate) async fn full_text_hits(
        &mut self,
        keys: &[NativeHitKey],
    ) -> StorageResult<HashMap<NativeHitKey, SearchHit>> {
        let ids = unique_ids(keys.iter().map(|(id, _, _)| *id));
        let publications = self.committed_publications(&ids, "full_text").await?;
        let allowed: HashSet<_> = keys
            .iter()
            .filter(|key| publications.contains(*key))
            .map(|(id, source, _)| (*id, *source))
            .collect();
        let mut chunks = current_chunks(&mut self.tx, self.scope, Some(&ids), None).await?;
        chunks.retain(|chunk| allowed.contains(&(chunk.hit.chunk_id, chunk.root)));
        let chunks: HashMap<_, _> = self
            .guard_retrieval_chunks(chunks)
            .await?
            .into_iter()
            .map(|chunk| (chunk.hit.chunk_id, chunk))
            .collect();
        let mut hits = HashMap::new();
        for key in keys {
            if let Some(chunk) = chunks.get(&key.0)
                && chunk.root == key.1
                && publications.contains(key)
            {
                hits.insert(*key, chunk.hit.clone());
            }
        }
        Ok(hits)
    }

    pub async fn full_text_hit(
        &mut self,
        id: Uuid,
        source: SourceVersion,
        generation: i64,
    ) -> StorageResult<Option<SearchHit>> {
        let key = (id, source, generation);
        Ok(self.full_text_hits(&[key]).await?.remove(&key))
    }

    #[cfg(feature = "local-storage")]
    pub(crate) async fn context_full_text_candidates(
        &mut self,
        generation: i64,
        route: crate::context_retrieval::ContextRoute,
        identity: Option<&crate::memory_identity::MemoryIdentity>,
    ) -> StorageResult<Vec<Uuid>> {
        let route_ids = unique_ids(
            self.context_route_rows(route, identity)
                .await?
                .into_iter()
                .map(|(id, _)| id),
        );
        let mut indexed = Vec::new();
        for batch_index in 0..route_ids.len().div_ceil(8).max(1) {
            let start = batch_index * 8;
            let batch = &route_ids[start..(start + 8).min(route_ids.len())];
            let mut query = db::query_scalar::<Uuid>(
                "SELECT artifact_id FROM oc_artifact_ledger WHERE tenant_id=? AND workspace_id=? AND generation=? AND surface='full_text' AND state='committed' AND artifact_id IN (?,?,?,?,?,?,?,?)",
            )
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .bind(generation);
            for slot in 0..8 {
                query = query.bind(batch.get(slot).copied());
            }
            indexed.extend(query.fetch_all(&mut *self.tx).await.map_err(turso_err)?);
        }
        let publications = self.committed_publications(&indexed, "full_text").await?;
        let mut chunks = current_chunks(&mut self.tx, self.scope, Some(&indexed), None).await?;
        chunks.retain(|chunk| publications.contains(&(chunk.hit.chunk_id, chunk.root, generation)));
        Ok(unique_ids(
            self.guard_retrieval_chunks(chunks)
                .await?
                .into_iter()
                .map(|chunk| chunk.hit.chunk_id),
        ))
    }
}
