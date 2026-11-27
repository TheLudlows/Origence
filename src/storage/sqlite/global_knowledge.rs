//! Immutable global release snapshots and their approved keyword projection.
use super::*;
use crate::{
    service::{GlobalKnowledgeDecision, global_knowledge::GlobalKnowledgeChunk},
    types::SearchHit,
};

struct Release {
    id: Uuid,
    scope: Scope,
    asset: Uuid,
    version: i32,
    source: Uuid,
    title: String,
    content_hash: String,
    state: String,
    submitted_at: i64,
    reviewed_at: Option<i64>,
}

impl sqlite_db::FromRow for Release {
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
            submitted_at: row.try_get("submitted_at")?,
            reviewed_at: row.try_get("reviewed_at")?,
        })
    }
}
const RELEASE_FIELDS: &str = "id,tenant_id,workspace_id,asset_id,version,source_event_id,title,content_hash,state,submitted_at,reviewed_at";

fn release_value(row: &Release, private: bool) -> Value {
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
        let row: Option<Release> = sqlite_db::query_as(&sql)
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
        chunks: &[GlobalKnowledgeChunk],
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
        let id = Uuid::new_v4();
        sqlite_db::query("INSERT INTO oc_global_knowledge(id,tenant_id,workspace_id,asset_id,version,source_event_id,title,content_hash,state,submitted_at)
            VALUES(?,?,?,?,?,?,?,?,'pending',?)")
            .bind(id).bind(self.scope.tenant_id).bind(self.scope.workspace_id)
            .bind(asset).bind(version).bind(source).bind(title).bind(hash).bind(now_ms())
            .execute(&mut *self.tx).await.map_err(sqlite_err)?;
        for chunk in chunks {
            sqlite_db::query("INSERT INTO oc_global_knowledge_chunks(publication_id,id,byte_start,byte_end,content,terms) VALUES(?,?,?,?,?,?)")
                .bind(id).bind(Uuid::new_v4()).bind(chunk.start).bind(chunk.end).bind(&chunk.content).bind(&chunk.terms)
                .execute(&mut *self.tx).await.map_err(sqlite_err)?;
        }
        Ok(release_value(&self.global_header(id).await?, true))
    }

    async fn global_header(&mut self, id: Uuid) -> StorageResult<Release> {
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
        let rows: Vec<Release> = sqlite_db::query_as(&sql)
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

    pub(crate) async fn global_snapshot(
        &mut self,
        id: Uuid,
        private: bool,
    ) -> StorageResult<Value> {
        let row = self.global_header(id).await?;
        if !private && row.state != "approved" {
            return Err(StorageError::NotFound);
        }
        let content = self.global_snapshot_content(&row).await?;
        let mut value = release_value(&row, private);
        value["content"] = json!(content);
        Ok(value)
    }

    async fn global_snapshot_content(&mut self, row: &Release) -> StorageResult<String> {
        let chunks: Vec<String> = sqlite_db::query_scalar("SELECT content FROM oc_global_knowledge_chunks WHERE publication_id=? ORDER BY byte_start")
            .bind(row.id).fetch_all(&mut *self.tx).await.map_err(sqlite_err)?;
        let content = chunks.concat();
        if hash(&content) != row.content_hash {
            return Err(invalid_copy());
        }
        Ok(content)
    }

    pub(crate) async fn moderate_global_snapshot(
        &mut self,
        id: Uuid,
        decision: GlobalKnowledgeDecision,
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
            self.scope = row.scope;
            let v = self.global_knowledge_source(row.asset, row.version).await?;
            if v["source_event_id"] != json!(row.source) || v["content_hash"] != row.content_hash {
                return Err(StorageError::Conflict(
                    "submitted knowledge source changed".into(),
                ));
            }
            self.global_snapshot_content(&row).await?;
        }
        sqlite_db::query("UPDATE oc_global_knowledge SET state=?,reviewed_at=? WHERE id=?")
            .bind(to)
            .bind(now_ms())
            .bind(id)
            .execute(&mut *self.tx)
            .await
            .map_err(sqlite_err)?;
        sqlite_db::query("UPDATE oc_global_knowledge_chunks SET enabled=? WHERE publication_id=?")
            .bind(to == "approved")
            .bind(id)
            .execute(&mut *self.tx)
            .await
            .map_err(sqlite_err)?;
        Ok(release_value(&self.global_header(id).await?, true))
    }

    /// All normalized query terms must match one released chunk. Every lookup
    /// and the final authorization share this read transaction.
    pub(crate) async fn global_keyword_hits(
        &mut self,
        terms: &str,
        limit: usize,
    ) -> StorageResult<Vec<SearchHit>> {
        let tokens: std::collections::BTreeSet<_> = terms.split_whitespace().collect();
        if tokens.is_empty() {
            return Ok(Vec::new());
        }
        // Group predicates so long valid queries stay below SQLite's
        // expression-depth limit. The SQL contains only fixed fragments.
        let predicates = (0..tokens.len())
            .step_by(32)
            .map(|start| {
                let count = (tokens.len() - start).min(32);
                format!("({})", vec!["instr(terms,?)>0"; count].join(" AND "))
            })
            .collect::<Vec<_>>()
            .join(" AND ");
        let sql = format!(
            "SELECT publication_id,id,byte_start,byte_end,content FROM oc_global_knowledge_chunks WHERE enabled AND {predicates} ORDER BY publication_id,byte_start LIMIT ?"
        );
        let mut query = sqlite_db::query_as(&sql);
        for token in tokens {
            query = query.bind(format!(" {token} "));
        }
        let rows: Vec<(Uuid, Uuid, i64, i64, String)> = query
            .bind(limit as i64)
            .fetch_all(&mut *self.tx)
            .await
            .map_err(sqlite_err)?;
        let mut hits = Vec::with_capacity(rows.len());
        for (publication, chunk, start, end, content) in rows {
            let row = self.global_header(publication).await?;
            if row.state != "approved" {
                return Err(invalid_copy());
            }
            hits.push(SearchHit {
                content_hash: hash(&content),
                asset_id: publication,
                version: row.version,
                chunk_id: chunk,
                kind: "knowledge".into(),
                title: row.title,
                content,
                locator: json!({"scope":"global","publication_id":publication,"byte_start":start,
                    "byte_end":end,"parser":"global-knowledge-snapshot:v1"}),
                source_event_id: publication,
                score: 1.0 / (61 + hits.len()) as f64,
                identity: None,
            });
        }
        Ok(hits)
    }
}

fn invalid_copy() -> StorageError {
    StorageError::Unavailable("invalid global knowledge snapshot".into())
}
