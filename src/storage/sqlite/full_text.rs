//! Authority checks for the derived Lance full-text surface. Single-table
//! reads are associated inside one authority transaction before native ranking.
use super::read_model::{
    NativeHitKey, current_chunk_roots, current_chunks, rows_by_ids, unique_ids,
};
use super::*;
use crate::storage::native_turso as db;
use crate::types::SearchHit;
use std::collections::{BTreeSet, HashSet};

async fn current_publications(
    conn: &mut Connection,
    surface: &str,
) -> StorageResult<Vec<(Scope, Uuid, i64)>> {
    let rows = db::query("SELECT tenant_id,workspace_id,artifact_id,source_id,version,generation FROM oc_artifact_ledger WHERE artifact_type='chunk' AND surface=? AND state='committed'")
        .bind(surface).fetch_all(&mut *conn).await.map_err(turso_err)?;
    let mut scoped: HashMap<Scope, Vec<NativeHitKey>> = HashMap::new();
    for row in rows {
        let scope = Scope {
            tenant_id: row.try_get("tenant_id").map_err(turso_err)?,
            workspace_id: row.try_get("workspace_id").map_err(turso_err)?,
        };
        scoped.entry(scope).or_default().push((
            row.try_get("artifact_id").map_err(turso_err)?,
            SourceVersion {
                source_id: row.try_get("source_id").map_err(turso_err)?,
                version: row.try_get("version").map_err(turso_err)?,
            },
            row.try_get("generation").map_err(turso_err)?,
        ));
    }
    let mut publications = Vec::new();
    for (scope, keys) in scoped {
        let ids = unique_ids(keys.iter().map(|(id, _, _)| *id));
        let roots = current_chunk_roots(conn, scope, &ids).await?;
        publications.extend(
            keys.into_iter()
                .filter(|(id, source, _)| roots.get(id) == Some(source))
                .map(|(id, _, generation)| (scope, id, generation)),
        );
    }
    Ok(publications)
}

impl TursoStore {
    /// Root-active current publication metadata, not a visibility decision.
    pub async fn has_committed_full_text(&self) -> StorageResult<bool> {
        let mut tx = self.pool.begin().await.map_err(turso_err)?;
        let required = !current_publications(&mut tx, "full_text").await?.is_empty();
        tx.rollback().await.map_err(turso_err)?;
        Ok(required)
    }

    /// Required profile schemas in one snapshot. Withdrawn or superseded roots
    /// do not prevent startup cleanup; no support graph is loaded here.
    pub async fn committed_vector_profiles(&self) -> StorageResult<Vec<(String, usize)>> {
        let mut tx = self.pool.begin().await.map_err(turso_err)?;
        let publications = current_publications(&mut tx, "vector").await?;
        let mut scoped: HashMap<Scope, HashSet<(Uuid, i64)>> = HashMap::new();
        for (scope, id, generation) in publications {
            scoped.entry(scope).or_default().insert((id, generation));
        }
        let mut profiles = BTreeSet::new();
        for (scope, keys) in scoped {
            let ids = unique_ids(keys.iter().map(|(id, _)| *id));
            for row in rows_by_ids(
                &mut tx,
                scope,
                "oc_index_entries",
                "artifact_id,field,model_id,dimension,generation,state",
                "artifact_id",
                &ids,
            )
            .await?
            {
                if row.try_get::<String>("field").map_err(turso_err)? != "content"
                    || row.try_get::<String>("state").map_err(turso_err)? != "ready"
                    || !keys.contains(&(
                        row.try_get("artifact_id").map_err(turso_err)?,
                        row.try_get("generation").map_err(turso_err)?,
                    ))
                {
                    continue;
                }
                let dimension: i64 = row.try_get("dimension").map_err(turso_err)?;
                if !(1..=i64::from(i32::MAX)).contains(&dimension) {
                    return Err(StorageError::Conflict(
                        "invalid committed vector dimension".into(),
                    ));
                }
                profiles.insert((
                    row.try_get("model_id").map_err(turso_err)?,
                    dimension as usize,
                ));
            }
        }
        tx.rollback().await.map_err(turso_err)?;
        Ok(profiles.into_iter().collect())
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
        let route_ids: HashSet<_> = self
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
