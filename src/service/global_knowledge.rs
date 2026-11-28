//! Submission and moderation of independent, immutable knowledge releases.
use super::*;
use crate::{
    storage::{
        ChunkPayload, FullTextEntry, FullTextQuery, SourceVersion,
        anyseek::{MAX_GROUP, operation_epoch},
        sqlite::global_knowledge::{GlobalChunkRef, GlobalRelease, release_value},
    },
    types::SearchHit,
};
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};

// Rank bounded groups of approved IDs using one collection’s lexical scores.
const GLOBAL_QUERY_BATCH: usize = 4 * MAX_GROUP;

pub(crate) const GLOBAL_SCOPE: Scope = Scope {
    tenant_id: Uuid::nil(),
    workspace_id: Uuid::nil(),
};

fn unavailable_release() -> AppError {
    AppError::Unavailable("invalid global knowledge snapshot".into())
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlobalKnowledgeDecision {
    Approve,
    Reject,
    Withdraw,
}

impl GlobalKnowledgeDecision {
    pub(crate) fn transition(self) -> (&'static str, &'static str) {
        match self {
            Self::Approve => ("pending", "approved"),
            Self::Reject => ("pending", "rejected"),
            Self::Withdraw => ("approved", "withdrawn"),
        }
    }
}

impl Service {
    /// Submit the exact published version from the caller's workspace.
    /// The model, original files, archived turns and source IDs are not exported.
    pub async fn submit_global_knowledge(
        &self,
        auth: &AuthContext,
        asset: Uuid,
        version: i32,
    ) -> Result<Value> {
        if asset.is_nil() || version <= 0 {
            return Err(AppError::Invalid(
                "a published knowledge version is required".into(),
            ));
        }
        let mut tx = self.read(auth, Permission::Write).await?;
        if let Some(existing) = tx.global_submission(asset, version).await? {
            tx.commit().await?;
            return Ok(existing);
        }
        let v = tx.global_knowledge_source(asset, version).await?;
        tx.commit().await?;
        let mut materialized = self.get(auth, asset, Some(version)).await?;
        let content: String = decode(materialized["content"].take())?;
        materialized
            .as_object_mut()
            .expect("asset view")
            .remove("content");
        if v != materialized {
            return Err(AppError::Conflict("knowledge version changed".into()));
        }
        let publication = crate::graph::entity_id(&format!(
            "global:{}:{}:{asset}:{version}",
            auth.tenant_id, auth.workspace_id
        ));
        let (manifest, entries) = tokio::task::spawn_blocking(move || {
            let mut manifest = Vec::new();
            let mut entries = Vec::new();
            for chunk in parsing::chunks(&content, "text")? {
                let start = chunk.locator["byte_start"].as_i64().ok_or_else(unavailable_release)?;
                let end = chunk.locator["byte_end"].as_i64().ok_or_else(unavailable_release)?;
                let id = crate::graph::entity_id(&format!("global-chunk:{publication}:{start}:{end}"));
                manifest.push(GlobalChunkRef { id, start, end, hash: crate::storage::hash(&chunk.content) });
                entries.push(FullTextEntry {
                    id, source: SourceVersion { source_id: publication, version }, generation: 1,
                    search_terms: parsing::lexical(&chunk.content),
                    payload: ChunkPayload {
                        content: chunk.content,
                        locator: json!({"scope":"global","publication_id":publication,
                            "byte_start":start,"byte_end":end,"parser":"global-knowledge-snapshot:v1"}),
                        summary: None,
                    },
                });
            }
            Ok::<_, AppError>((manifest, entries))
        })
        .await
        .map_err(|error| AppError::Internal(error.into()))??;
        let epoch = operation_epoch()?;
        // No authority transaction spans native writes. Each subgroup's
        // deterministic membership makes a partial or uncertain attempt retryable.
        for entries in entries.chunks(8) {
            self.engine
                .vector()
                .upsert_publication(
                    GLOBAL_SCOPE,
                    entries.to_vec(),
                    Vec::new(),
                    None,
                    Some(epoch),
                )
                .await?;
        }
        let mut tx = self.write(auth, Permission::Write).await?;
        let result = tx
            .submit_global_snapshot(asset, version, &v, publication, &manifest)
            .await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn global_knowledge_catalog(
        &self,
        auth: &AuthContext,
        after: Uuid,
        limit: usize,
    ) -> Result<Value> {
        let mut tx = self.read(auth, Permission::Read).await?;
        let result = tx.global_catalog(None, after, limit).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn global_knowledge(&self, auth: &AuthContext, id: Uuid) -> Result<Value> {
        let mut tx = self.read(auth, Permission::Read).await?;
        let header = tx.global_snapshot_header(id, false).await?;
        tx.commit().await?;
        let body = self.read_global_content(&header).await?;
        let mut tx = self.read(auth, Permission::Read).await?;
        if tx.global_snapshot_header(id, false).await? != header {
            return Err(AppError::Conflict("global release changed".into()));
        }
        tx.commit().await?;
        let mut value = release_value(&header, false);
        value["content"] = json!(body);
        Ok(value)
    }

    async fn read_global_content(&self, header: &GlobalRelease) -> Result<String> {
        let chunks = header.chunks()?;
        let ids = chunks.iter().map(|chunk| chunk.id).collect::<Vec<_>>();
        let mut payloads = self
            .engine
            .vector()
            .read_payloads(GLOBAL_SCOPE, &ids)
            .await?;
        let mut content = String::new();
        for chunk in &chunks {
            let payload = payloads.remove(&chunk.id).ok_or_else(unavailable_release)?;
            content.push_str(&checked_chunk(header.id, chunk, payload)?.content);
        }
        if crate::storage::hash(&content) != header.content_hash {
            return Err(unavailable_release());
        }
        Ok(content)
    }

    pub(crate) async fn find_global_keywords(
        &self,
        headers: Vec<GlobalRelease>,
        terms: &str,
        limit: usize,
    ) -> Result<Vec<(Arc<GlobalRelease>, SearchHit)>> {
        if terms.trim().is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let headers = headers.into_iter().map(Arc::new).collect::<Vec<_>>();
        let mut approved = HashMap::new();
        for (index, header) in headers.iter().enumerate() {
            for chunk in header.chunks()? {
                if approved.insert(chunk.id, (index, chunk)).is_some() {
                    return Err(unavailable_release());
                }
            }
        }
        if approved.is_empty() {
            return Ok(Vec::new());
        }
        let sources = approved
            .iter()
            .map(|(id, (index, _))| {
                (
                    *id,
                    SourceVersion {
                        source_id: headers[*index].id,
                        version: headers[*index].version,
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        let limit = limit.min(100);
        let mut ids = sources.keys().copied().collect::<Vec<_>>();
        ids.sort_unstable();
        let mut hits = Vec::new();
        for batch in ids.chunks(GLOBAL_QUERY_BATCH) {
            hits.extend(
                self.engine
                    .vector()
                    .search_full_text_authorized(
                        FullTextQuery {
                            scope: GLOBAL_SCOPE,
                            artifact_ids: Some(batch.to_vec()),
                            generation: 1,
                            search_terms: terms.to_owned(),
                            limit,
                        },
                        &sources,
                    )
                    .await?,
            );
            // All batches query the same index. Keep only the merged top-k,
            // preserving native order among equal scores in each batch.
            hits.sort_by(|left, right| right.score.total_cmp(&left.score));
            hits.truncate(limit);
        }
        let mut payloads = self
            .engine
            .vector()
            .read_payloads(
                GLOBAL_SCOPE,
                &hits.iter().map(|hit| hit.id).collect::<Vec<_>>(),
            )
            .await?;
        hits.into_iter()
            .enumerate()
            .map(|(rank, hit)| {
                let (index, chunk) = approved.remove(&hit.id).ok_or_else(unavailable_release)?;
                let header = &headers[index];
                let payload = payloads.remove(&hit.id).ok_or_else(unavailable_release)?;
                let payload = checked_chunk(header.id, &chunk, payload)?;
                Ok((
                    header.clone(),
                    SearchHit {
                        asset_id: header.id,
                        version: header.version,
                        chunk_id: hit.id,
                        kind: "knowledge".to_owned(),
                        title: header.title.clone(),
                        content: payload.content,
                        content_hash: chunk.hash,
                        locator: payload.locator,
                        source_event_id: header.id,
                        score: 1.0 / (61 + rank) as f64,
                        identity: None,
                    },
                ))
            })
            .collect()
    }

    pub async fn global_review_catalog(
        &self,
        token: &str,
        state: &str,
        after: Uuid,
        limit: usize,
    ) -> Result<Value> {
        if !matches!(state, "pending" | "approved" | "rejected" | "withdrawn") {
            return Err(AppError::Invalid("invalid global knowledge state".into()));
        }
        let mut tx = self
            .engine
            .relational()
            .global_moderation(token, false)
            .await?;
        let result = tx.global_catalog(Some(state), after, limit).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn global_review_detail(&self, token: &str, id: Uuid) -> Result<Value> {
        let mut tx = self
            .engine
            .relational()
            .global_moderation(token, false)
            .await?;
        let header = tx.global_snapshot_header(id, true).await?;
        tx.commit().await?;
        let body = self.read_global_content(&header).await?;
        let mut tx = self
            .engine
            .relational()
            .global_moderation(token, false)
            .await?;
        if tx.global_snapshot_header(id, true).await? != header {
            return Err(AppError::Conflict("global release changed".into()));
        }
        tx.commit().await?;
        let mut result = release_value(&header, true);
        result["content"] = json!(body);
        Ok(result)
    }

    pub async fn review_global_knowledge(
        &self,
        token: &str,
        id: Uuid,
        decision: GlobalKnowledgeDecision,
    ) -> Result<Value> {
        let checked = if matches!(decision, GlobalKnowledgeDecision::Approve) {
            let mut tx = self
                .engine
                .relational()
                .global_moderation(token, false)
                .await?;
            let header = tx.global_snapshot_header(id, true).await?;
            tx.commit().await?;
            if header.state == "pending" {
                self.read_global_content(&header).await?;
            }
            Some(header)
        } else {
            None
        };
        let mut tx = self
            .engine
            .relational()
            .global_moderation(token, true)
            .await?;
        let result = tx
            .moderate_global_snapshot(id, decision, checked.as_ref())
            .await?;
        tx.commit().await?;
        Ok(result)
    }
}

fn checked_chunk(
    publication: Uuid,
    chunk: &GlobalChunkRef,
    payload: ChunkPayload,
) -> Result<ChunkPayload> {
    if payload.locator
        != json!({"scope":"global","publication_id":publication,
        "byte_start":chunk.start,"byte_end":chunk.end,"parser":"global-knowledge-snapshot:v1"})
        || payload.summary.is_some()
        || crate::storage::hash(&payload.content) != chunk.hash
        || i64::try_from(payload.content.len()).ok() != Some(chunk.end - chunk.start)
    {
        return Err(unavailable_release());
    }
    Ok(payload)
}
