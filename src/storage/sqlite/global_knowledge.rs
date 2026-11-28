//! Authority records for immutable global knowledge releases.
use super::*;
#[cfg(feature = "local-storage")]
use crate::service::GlobalKnowledgeDecision;

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct GlobalChunkRef {
    pub(crate) id: Uuid,
    pub(crate) start: i64,
    pub(crate) end: i64,
    pub(crate) hash: String,
}

fn decode_chunks(manifest: &str) -> StorageResult<Vec<GlobalChunkRef>> {
    let chunks: Vec<GlobalChunkRef> = serde_json::from_str(manifest).map_err(|_| invalid_copy())?;
    let mut end = 0;
    let mut ids = std::collections::HashSet::new();
    for chunk in &chunks {
        if chunk.start != end
            || chunk.end <= end
            || !ids.insert(chunk.id)
            || !crate::retrieval_diagnostics::digest(&chunk.hash)
        {
            return Err(invalid_copy());
        }
        end = chunk.end;
    }
    if chunks.is_empty() {
        return Err(invalid_copy());
    }
    Ok(chunks)
}

#[cfg(feature = "local-storage")]
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct GlobalRelease {
    pub(crate) id: Uuid,
    scope: Scope,
    asset: Uuid,
    pub(crate) version: i32,
    source: Uuid,
    pub(crate) title: String,
    pub(crate) content_hash: String,
    pub(crate) state: String,
    chunk_manifest: String,
    submitted_at: i64,
    reviewed_at: Option<i64>,
}

#[cfg(feature = "local-storage")]
impl GlobalRelease {
    pub(crate) fn chunks(&self) -> StorageResult<Vec<GlobalChunkRef>> {
        decode_chunks(&self.chunk_manifest)
    }
}

#[cfg(feature = "local-storage")]
impl sqlite_db::FromRow for GlobalRelease {
    fn from_row(row: &sqlite_db::Row) -> sqlite_db::Result<Self> {
        Ok(Self {
            id: row.try_get("id")?,
            scope: Scope {
                tenant_id: row.try_get("tenant_id")?,
                workspace_id: row.try_get("workspace_id")?,
            },
            asset: row.try_get("asset_id")?,
            version: row.try_get("version")?,
            source: row.try_get("source_event_id")?,
            title: row.try_get("title")?,
            content_hash: row.try_get("content_hash")?,
            state: row.try_get("state")?,
            chunk_manifest: row.try_get("chunk_manifest")?,
            submitted_at: row.try_get("submitted_at")?,
            reviewed_at: row.try_get("reviewed_at")?,
        })
    }
}
#[cfg(feature = "local-storage")]
const RELEASE_FIELDS: &str = "id,tenant_id,workspace_id,asset_id,version,source_event_id,title,content_hash,state,chunk_manifest,submitted_at,reviewed_at";

#[cfg(feature = "local-storage")]
pub(crate) fn release_value(row: &GlobalRelease, private: bool) -> Value {
    let mut value = json!({
        "publication_id":row.id,"scope":"global","title":row.title,"version":row.version,
        "content_hash":row.content_hash,"state":row.state,"reviewed_at_ms":row.reviewed_at
    });
    if private {
        value["workspace_id"] = json!(row.scope.workspace_id);
        value["asset_id"] = json!(row.asset);
        value["source_event_id"] = json!(row.source);
        value["submitted_at_ms"] = json!(row.submitted_at);
    }
    value
}

#[cfg(feature = "local-storage")]
impl SqliteStore {
    pub(crate) async fn global_moderation(
        &self,
        token: &str,
        write: bool,
    ) -> StorageResult<SqliteTx> {
        let mut tx = if write {
            self.pool.begin_with("BEGIN IMMEDIATE").await
        } else {
            self.pool.begin().await
        }
        .map_err(sqlite_err)?;
        super::deployment::require_admin(&mut tx, token).await?;
        Ok(SqliteTx {
            tx,
            scope: Scope {
                tenant_id: Uuid::nil(),
                workspace_id: Uuid::nil(),
            },
            principal_id: Uuid::nil(),
            required: Vec::new(),
            command: None,
        })
    }
}

#[cfg(feature = "local-storage")]
impl SqliteTx {
    pub(crate) async fn global_knowledge_source(
        &mut self,
        asset: Uuid,
        version: i32,
    ) -> StorageResult<Value> {
        self.require_not_learning_asset(asset).await?;
        let v = self.asset_view_inner(asset, Some(version)).await?;
        if v["kind"] != "knowledge" {
            return Err(StorageError::InvalidArgument(
                "only knowledge versions may be submitted",
            ));
        }
        Ok(v)
    }

    pub(crate) async fn global_submission(
        &mut self,
        asset: Uuid,
        version: i32,
    ) -> StorageResult<Option<Value>> {
        let sql = format!(
            "SELECT {RELEASE_FIELDS} FROM oc_global_knowledge WHERE tenant_id=? AND workspace_id=? AND asset_id=? AND version=?"
        );
        let row: Option<GlobalRelease> = sqlite_db::query_as(&sql)
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .bind(asset)
            .bind(version)
            .fetch_optional(&mut *self.tx)
            .await
            .map_err(sqlite_err)?;
        Ok(row.as_ref().map(|row| release_value(row, true)))
    }

    pub(crate) async fn submit_global_snapshot(
        &mut self,
        asset: Uuid,
        version: i32,
        view: &Value,
        id: Uuid,
        chunks: &[GlobalChunkRef],
    ) -> StorageResult<Value> {
        if let Some(existing) = self.global_submission(asset, version).await? {
            return Ok(existing);
        }
        if self.global_knowledge_source(asset, version).await? != *view {
            return Err(StorageError::Conflict("knowledge version changed".into()));
        }
        let source: Uuid =
            serde_json::from_value(view["source_event_id"].clone()).map_err(|_| invalid_copy())?;
        let title = view["title"].as_str().ok_or_else(invalid_copy)?;
        let hash = view["content_hash"].as_str().ok_or_else(invalid_copy)?;
        let manifest = serde_json::to_string(chunks).map_err(|_| invalid_copy())?;
        sqlite_db::query("INSERT INTO oc_global_knowledge(id,tenant_id,workspace_id,asset_id,version,source_event_id,title,content_hash,chunk_manifest,state,submitted_at)
            VALUES(?,?,?,?,?,?,?,?,?,'pending',?)")
            .bind(id).bind(self.scope.tenant_id).bind(self.scope.workspace_id)
            .bind(asset).bind(version).bind(source).bind(title).bind(hash).bind(manifest).bind(now_ms())
            .execute(&mut *self.tx).await.map_err(sqlite_err)?;
        Ok(release_value(&self.global_header(id).await?, true))
    }

    pub(crate) async fn global_header(&mut self, id: Uuid) -> StorageResult<GlobalRelease> {
        let sql = format!("SELECT {RELEASE_FIELDS} FROM oc_global_knowledge WHERE id=?");
        sqlite_db::query_as(&sql)
            .bind(id)
            .fetch_optional(&mut *self.tx)
            .await
            .map_err(sqlite_err)?
            .ok_or(StorageError::NotFound)
    }

    pub(crate) async fn global_catalog(
        &mut self,
        moderation_state: Option<&str>,
        after: Uuid,
        limit: usize,
    ) -> StorageResult<Value> {
        if !(1..=100).contains(&limit) {
            return Err(StorageError::InvalidArgument(
                "invalid global knowledge page",
            ));
        }
        let sql = format!(
            "SELECT {RELEASE_FIELDS} FROM oc_global_knowledge WHERE state=? AND id>? ORDER BY id LIMIT ?"
        );
        let rows: Vec<GlobalRelease> = sqlite_db::query_as(&sql)
            .bind(moderation_state.unwrap_or("approved"))
            .bind(after)
            .bind((limit + 1) as i64)
            .fetch_all(&mut *self.tx)
            .await
            .map_err(sqlite_err)?;
        let next = (rows.len() > limit).then(|| rows[limit - 1].id);
        let items: Vec<_> = rows
            .iter()
            .take(limit)
            .map(|row| release_value(row, moderation_state.is_some()))
            .collect();
        Ok(json!({"items":items,"next_after":next}))
    }

    pub(crate) async fn global_snapshot_header(
        &mut self,
        id: Uuid,
        private: bool,
    ) -> StorageResult<GlobalRelease> {
        let row = self.global_header(id).await?;
        if !private && row.state != "approved" {
            return Err(StorageError::NotFound);
        }
        Ok(row)
    }

    pub(crate) async fn approved_global_knowledge(&mut self) -> StorageResult<Vec<GlobalRelease>> {
        let sql = format!(
            "SELECT {RELEASE_FIELDS} FROM oc_global_knowledge WHERE state='approved' ORDER BY id"
        );
        sqlite_db::query_as(&sql)
            .fetch_all(&mut *self.tx)
            .await
            .map_err(sqlite_err)
    }

    pub(crate) async fn moderate_global_snapshot(
        &mut self,
        id: Uuid,
        decision: GlobalKnowledgeDecision,
        checked_content: Option<&GlobalRelease>,
    ) -> StorageResult<Value> {
        let row = self.global_header(id).await?;
        let (from, to) = decision.transition();
        if row.state == to {
            return Ok(release_value(&row, true));
        }
        if row.state != from {
            return Err(StorageError::Conflict(
                "invalid knowledge review transition".into(),
            ));
        }
        if to == "approved" {
            if checked_content != Some(&row) {
                return Err(StorageError::Conflict("global release changed".into()));
            }
            self.scope = row.scope;
            let v = self.global_knowledge_source(row.asset, row.version).await?;
            if v["source_event_id"] != json!(row.source) || v["content_hash"] != row.content_hash {
                return Err(StorageError::Conflict(
                    "submitted knowledge source changed".into(),
                ));
            }
        }
        sqlite_db::query("UPDATE oc_global_knowledge SET state=?,reviewed_at=? WHERE id=?")
            .bind(to)
            .bind(now_ms())
            .bind(id)
            .execute(&mut *self.tx)
            .await
            .map_err(sqlite_err)?;
        Ok(release_value(&self.global_header(id).await?, true))
    }
}

#[cfg(feature = "local-storage")]
impl SqliteStore {
    pub(crate) async fn has_global_knowledge(&self) -> StorageResult<bool> {
        sqlite_db::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_global_knowledge)")
            .fetch_one(&self.pool)
            .await
            .map_err(sqlite_err)
    }
}

impl SqliteTx {
    pub(super) async fn global_citation_available(
        &mut self,
        publication: Uuid,
        version: i32,
        chunk_id: Uuid,
        start: i64,
        end: i64,
    ) -> StorageResult<bool> {
        let manifest: Option<String> = sqlite_db::query_scalar(
            "SELECT chunk_manifest FROM oc_global_knowledge WHERE id=? AND version=? AND state='approved'",
        ).bind(publication).bind(version).fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        let Some(manifest) = manifest else {
            return Ok(false);
        };
        if !decode_chunks(&manifest)?
            .iter()
            .any(|chunk| chunk.id == chunk_id && chunk.start == start && chunk.end == end)
        {
            return Err(invalid_copy());
        }
        Ok(true)
    }
}

fn invalid_copy() -> StorageError {
    StorageError::Unavailable("invalid global knowledge snapshot".into())
}
