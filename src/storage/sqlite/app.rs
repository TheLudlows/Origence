//! Local application queries. SQL stays inside the SQLite adapter.
use super::*;
use crate::types::SearchHit;
use sqlx::Row;

const VISIBLE: &str = "FROM oc_chunks c JOIN oc_assets a ON a.tenant_id=c.tenant_id AND a.workspace_id=c.workspace_id AND a.id=c.asset_id AND a.current_version=c.version JOIN oc_versions v ON v.tenant_id=c.tenant_id AND v.workspace_id=c.workspace_id AND v.asset_id=c.asset_id AND v.version=c.version JOIN oc_events e ON e.tenant_id=v.tenant_id AND e.workspace_id=v.workspace_id AND e.id=v.source_event_id WHERE c.tenant_id=? AND c.workspace_id=? AND NOT a.deleted AND e.state='active'";
const HIT: &str = "SELECT c.id, c.asset_id,c.version,a.kind,v.title,c.content,c.locator,v.source_event_id,c.search_terms";

fn hit(row: &sqlx::sqlite::SqliteRow, score: f64) -> SearchHit {
    SearchHit {
        chunk_id: row.get("id"),
        asset_id: row.get("asset_id"),
        version: row.get("version"),
        kind: row.get("kind"),
        title: row.get("title"),
        content: row.get("content"),
        locator: serde_json::from_str(row.get("locator")).unwrap_or(Value::Null),
        source_event_id: row.get("source_event_id"),
        score,
    }
}

impl SqliteStore {
    pub(super) async fn begin_scoped(
        &self,
        auth: AuthorizedScope,
        write: bool,
    ) -> StorageResult<SqliteTx> {
        let tx = self
            .pool
            .begin_with(if write { "BEGIN IMMEDIATE" } else { "BEGIN" })
            .await
            .map_err(sqlite_err)?;
        let mut tx = SqliteTx {
            tx,
            scope: auth.scope,
            principal_id: auth.principal_id,
            required: Vec::new(),
            command: None,
        };
        tx.live_role().await?;
        Ok(tx)
    }
    pub async fn begin_read(&self, auth: AuthorizedScope) -> StorageResult<SqliteTx> {
        self.begin_scoped(auth, false).await
    }
    pub async fn workspace_scope(&self, workspace: Uuid) -> StorageResult<Scope> {
        let tenant = sqlx::query_scalar("SELECT tenant_id FROM oc_workspaces WHERE id=?")
            .bind(workspace)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlite_err)?
            .ok_or(StorageError::NotFound)?;
        Ok(Scope {
            tenant_id: tenant,
            workspace_id: workspace,
        })
    }
    pub async fn scopes(&self) -> StorageResult<Vec<Scope>> {
        let rows: Vec<(Uuid, Uuid)> = sqlx::query_as("SELECT tenant_id,id FROM oc_workspaces")
            .fetch_all(&self.pool)
            .await
            .map_err(sqlite_err)?;
        Ok(rows
            .into_iter()
            .map(|(tenant_id, workspace_id)| Scope {
                tenant_id,
                workspace_id,
            })
            .collect())
    }
    /// Called exactly once by the exclusive host before it begins polling.
    pub async fn recover_jobs(&self) -> StorageResult<()> {
        sqlx::query("UPDATE oc_jobs SET state='pending',run_token=run_token+1,updated_at=? WHERE state='processing' AND NOT cancel_requested").bind(now_ms()).execute(&self.pool).await.map_err(sqlite_err)?;
        self.reconcile_ledger().await?;
        Ok(())
    }
    /// Failure settlement is privileged so revoking the creator cannot strand work.
    pub async fn fail_job(
        &self,
        claim: &ClaimedJob,
        state: &str,
        code: &str,
        retry: bool,
    ) -> StorageResult<()> {
        sqlx::query("UPDATE oc_jobs SET state=CASE WHEN ? AND attempt<5 THEN 'retry_wait' ELSE ? END,error_code=?,next_retry_at=CASE WHEN ? AND attempt<5 THEN ? ELSE NULL END,updated_at=? WHERE tenant_id=? AND workspace_id=? AND id=? AND generation=? AND run_token=? AND state='processing'")
            .bind(retry).bind(state).bind(code).bind(retry).bind(now_ms()+2000).bind(now_ms()).bind(claim.scope.tenant_id).bind(claim.scope.workspace_id).bind(claim.job_id).bind(claim.generation).bind(claim.run_token).execute(&self.pool).await.map_err(sqlite_err)?;
        Ok(())
    }
    /// Detach invalid provenance before native cleanup. Repeatable after a crash.
    pub async fn cleanup_plan(
        &self,
        scope: Scope,
    ) -> StorageResult<(Vec<Uuid>, Vec<Uuid>, Vec<Uuid>)> {
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(sqlite_err)?;
        // Historical versions retain owners while the asset and source remain valid.
        let live = "EXISTS(SELECT 1 FROM oc_versions v JOIN oc_assets a ON a.tenant_id=v.tenant_id AND a.workspace_id=v.workspace_id AND a.id=v.asset_id JOIN oc_events e ON e.tenant_id=v.tenant_id AND e.workspace_id=v.workspace_id AND e.id=v.source_event_id WHERE v.tenant_id=o.tenant_id AND v.workspace_id=o.workspace_id AND v.source_event_id=o.source_id AND v.version=o.version AND NOT a.deleted AND e.state='active')";
        sqlx::query(&format!("DELETE FROM oc_artifact_owners AS o WHERE tenant_id=? AND workspace_id=? AND NOT {live}" )).bind(scope.tenant_id).bind(scope.workspace_id).execute(&mut *tx).await.map_err(sqlite_err)?;
        sqlx::query("DELETE FROM oc_artifact_owners AS o WHERE tenant_id=? AND workspace_id=? AND artifact_type IN ('chunk','summary') AND NOT EXISTS(SELECT 1 FROM oc_chunks c JOIN oc_assets a ON a.tenant_id=c.tenant_id AND a.workspace_id=c.workspace_id AND a.id=c.asset_id WHERE c.tenant_id=o.tenant_id AND c.workspace_id=o.workspace_id AND c.id=o.chunk_id AND NOT a.deleted)").bind(scope.tenant_id).bind(scope.workspace_id).execute(&mut *tx).await.map_err(sqlite_err)?;
        let sources: Vec<Uuid> = sqlx::query_scalar("SELECT DISTINCT l.artifact_id FROM oc_artifact_ledger l WHERE l.tenant_id=? AND l.workspace_id=? AND l.surface='vector' AND NOT EXISTS(SELECT 1 FROM oc_artifact_owners o WHERE o.tenant_id=l.tenant_id AND o.workspace_id=l.workspace_id AND o.artifact_id=l.artifact_id AND o.artifact_type='chunk')").bind(scope.tenant_id).bind(scope.workspace_id).fetch_all(&mut *tx).await.map_err(sqlite_err)?;
        let rows: Vec<(String,Uuid)> = sqlx::query_as("SELECT DISTINCT artifact_type,artifact_id FROM oc_artifact_owners WHERE tenant_id=? AND workspace_id=? AND artifact_type IN ('entity','relation')").bind(scope.tenant_id).bind(scope.workspace_id).fetch_all(&mut *tx).await.map_err(sqlite_err)?;
        tx.commit().await.map_err(sqlite_err)?;
        let entities = rows
            .iter()
            .filter(|(k, _)| k == "entity")
            .map(|(_, id)| *id)
            .collect();
        let relations = rows
            .iter()
            .filter(|(k, _)| k == "relation")
            .map(|(_, id)| *id)
            .collect();
        Ok((sources, entities, relations))
    }
    pub async fn finish_cleanup(&self, scope: Scope) -> StorageResult<()> {
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(sqlite_err)?;
        let invalid = "NOT EXISTS(SELECT 1 FROM oc_artifact_owners o WHERE o.tenant_id=c.tenant_id AND o.workspace_id=c.workspace_id AND o.artifact_id=c.id AND o.artifact_type='chunk')";
        sqlx::query(&format!("DELETE FROM oc_summaries WHERE tenant_id=? AND workspace_id=? AND chunk_id IN (SELECT c.id FROM oc_chunks c WHERE c.tenant_id=? AND c.workspace_id=? AND {invalid})")).bind(scope.tenant_id).bind(scope.workspace_id).bind(scope.tenant_id).bind(scope.workspace_id).execute(&mut *tx).await.map_err(sqlite_err)?;
        sqlx::query(&format!(
            "DELETE FROM oc_chunks AS c WHERE tenant_id=? AND workspace_id=? AND {invalid}"
        ))
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .execute(&mut *tx)
        .await
        .map_err(sqlite_err)?;
        sqlx::query("UPDATE oc_index_entries SET state='removed' WHERE tenant_id=? AND workspace_id=? AND NOT EXISTS(SELECT 1 FROM oc_chunks c WHERE c.tenant_id=oc_index_entries.tenant_id AND c.workspace_id=oc_index_entries.workspace_id AND c.id=oc_index_entries.artifact_id)").bind(scope.tenant_id).bind(scope.workspace_id).execute(&mut *tx).await.map_err(sqlite_err)?;
        sqlx::query("UPDATE oc_artifact_ledger SET state='orphan',updated_at=? WHERE tenant_id=? AND workspace_id=? AND NOT EXISTS(SELECT 1 FROM oc_artifact_owners o WHERE o.tenant_id=oc_artifact_ledger.tenant_id AND o.workspace_id=oc_artifact_ledger.workspace_id AND o.source_id=oc_artifact_ledger.source_id AND o.version=oc_artifact_ledger.version AND o.artifact_id=oc_artifact_ledger.artifact_id)").bind(now_ms()).bind(scope.tenant_id).bind(scope.workspace_id).execute(&mut *tx).await.map_err(sqlite_err)?;
        tx.commit().await.map_err(sqlite_err)
    }
}

impl SqliteTx {
    pub async fn asset_metadata(&mut self, id: Uuid) -> StorageResult<Value> {
        let row: Option<(String, Option<i32>, bool)> = sqlx::query_as("SELECT kind,current_version,deleted FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND id=?").bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(id).fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        let (kind, current_version, deleted) = row.ok_or(StorageError::NotFound)?;
        Ok(json!({"kind":kind,"current_version":current_version,"deleted":deleted}))
    }
    pub async fn save_publication(&mut self, job: Uuid, plan: &Value) -> StorageResult<()> {
        sqlx::query("UPDATE oc_jobs SET payload=json_set(payload,'$.publication',json(?)),updated_at=? WHERE tenant_id=? AND workspace_id=? AND id=?").bind(plan.to_string()).bind(now_ms()).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(job).execute(&mut *self.tx).await.map_err(sqlite_err)?;
        Ok(())
    }
    pub async fn index_ready(
        &mut self,
        chunk: Uuid,
        profile: &str,
        dimension: usize,
        generation: i64,
    ) -> StorageResult<()> {
        sqlx::query("INSERT INTO oc_index_entries(tenant_id,workspace_id,artifact_id,field,model_id,dimension,generation,state,created_at) VALUES(?,?,?,'content',?,?,?,'ready',?) ON CONFLICT DO UPDATE SET state='ready'").bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(chunk).bind(profile).bind(dimension as i64).bind(generation).bind(now_ms()).execute(&mut *self.tx).await.map_err(sqlite_err)?;
        Ok(())
    }
    pub async fn vector_generations(
        &mut self,
        profile: &str,
        dimension: usize,
    ) -> StorageResult<Vec<i64>> {
        sqlx::query_scalar("SELECT DISTINCT generation FROM oc_index_entries WHERE tenant_id=? AND workspace_id=? AND model_id=? AND dimension=? AND state='ready'").bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(profile).bind(dimension as i64).fetch_all(&mut *self.tx).await.map_err(sqlite_err)
    }
    pub async fn vector_candidates(
        &mut self,
        profile: &str,
        dimension: usize,
        generation: i64,
    ) -> StorageResult<Vec<Uuid>> {
        sqlx::query_scalar(&format!("SELECT c.id {VISIBLE} AND EXISTS(SELECT 1 FROM oc_index_entries i WHERE i.tenant_id=c.tenant_id AND i.workspace_id=c.workspace_id AND i.artifact_id=c.id AND i.model_id=? AND i.dimension=? AND i.generation=? AND i.state='ready') AND EXISTS(SELECT 1 FROM oc_artifact_ledger l WHERE l.tenant_id=c.tenant_id AND l.workspace_id=c.workspace_id AND l.artifact_id=c.id AND l.source_id=v.source_event_id AND l.version=v.version AND l.generation=? AND l.surface='vector' AND l.state='committed')"))
        .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(profile).bind(dimension as i64).bind(generation).bind(generation).fetch_all(&mut *self.tx).await.map_err(sqlite_err)
    }
    pub async fn vector_hit(
        &mut self,
        id: Uuid,
        source: SourceVersion,
        profile: &str,
        generation: i64,
    ) -> StorageResult<Option<SearchHit>> {
        let row = sqlx::query(&format!("{HIT} {VISIBLE} AND c.id=? AND v.source_event_id=? AND v.version=? AND EXISTS(SELECT 1 FROM oc_index_entries i WHERE i.tenant_id=c.tenant_id AND i.workspace_id=c.workspace_id AND i.artifact_id=c.id AND i.model_id=? AND i.generation=? AND i.state='ready') AND EXISTS(SELECT 1 FROM oc_artifact_ledger l WHERE l.tenant_id=c.tenant_id AND l.workspace_id=c.workspace_id AND l.artifact_id=c.id AND l.source_id=v.source_event_id AND l.version=v.version AND l.generation=? AND l.surface='vector' AND l.state='committed')")).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(id).bind(source.source_id).bind(source.version).bind(profile).bind(generation).bind(generation).fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        Ok(row.as_ref().map(|r| hit(r, 0.0)))
    }
    pub async fn visible_hit(&mut self, id: Uuid) -> StorageResult<Option<SearchHit>> {
        let row = sqlx::query(&format!("{HIT} {VISIBLE} AND c.id=?"))
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .bind(id)
            .fetch_optional(&mut *self.tx)
            .await
            .map_err(sqlite_err)?;
        Ok(row.as_ref().map(|r| hit(r, 0.0)))
    }
    pub async fn keyword_hits(
        &mut self,
        terms: &str,
        summaries: bool,
    ) -> StorageResult<Vec<SearchHit>> {
        let select = if summaries {
            "SELECT c.id,c.asset_id,c.version,a.kind,v.title,c.content,c.locator,v.source_event_id,(SELECT group_concat(s.search_terms,' ') FROM oc_summaries s WHERE s.tenant_id=c.tenant_id AND s.workspace_id=c.workspace_id AND s.chunk_id=c.id) search_terms"
        } else {
            HIT
        };
        let rows = sqlx::query(&format!("{select} {VISIBLE}"))
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .fetch_all(&mut *self.tx)
            .await
            .map_err(sqlite_err)?;
        let words: std::collections::HashSet<_> = terms.split_whitespace().collect();
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let mut hits = Vec::new();
        for row in rows {
            let text: Option<String> = row.get("search_terms");
            let tokens: std::collections::HashSet<_> =
                text.as_deref().unwrap_or("").split_whitespace().collect();
            if words.is_subset(&tokens) {
                hits.push(hit(&row, words.len() as f64 / tokens.len().max(1) as f64));
            }
        }
        hits.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then(a.chunk_id.cmp(&b.chunk_id))
        });
        hits.truncate(100);
        Ok(hits)
    }
    pub async fn graph_evidence(&mut self, artifact: Uuid) -> StorageResult<Vec<Value>> {
        let rows = sqlx::query(&format!("{HIT} {VISIBLE} AND EXISTS(SELECT 1 FROM oc_artifact_owners o JOIN oc_artifact_ledger l ON l.tenant_id=o.tenant_id AND l.workspace_id=o.workspace_id AND l.source_id=o.source_id AND l.version=o.version AND l.artifact_id=o.artifact_id AND l.surface='graph' AND l.state='committed' WHERE o.tenant_id=c.tenant_id AND o.workspace_id=c.workspace_id AND o.artifact_id=? AND o.source_id=v.source_event_id AND o.version=v.version) ORDER BY c.id LIMIT 20")).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(artifact).fetch_all(&mut *self.tx).await.map_err(sqlite_err)?;
        Ok(rows.iter().map(|r| {let h=hit(r,0.0);json!({"asset_id":h.asset_id,"version":h.version,"chunk_id":h.chunk_id,"source_event_id":h.source_event_id,"locator":h.locator})}).collect())
    }
    pub async fn issue_scoped_key(&mut self, role: &str) -> StorageResult<IssuedKey> {
        self.check_permission(Permission::Delete).await?;
        if !ROLES.contains(&role) {
            return Err(StorageError::Conflict("invalid role".into()));
        }
        let key_id = Uuid::new_v4();
        let token = format!("oc_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        sqlx::query("INSERT INTO oc_api_keys(id,tenant_id,workspace_id,token_hash,role,created_at) VALUES(?,?,?,?,?,?)").bind(key_id).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(hash(&token)).bind(role).bind(now_ms()).execute(&mut *self.tx).await.map_err(sqlite_err)?;
        self.audit("key.issue", key_id, json!({"role":role}))
            .await?;
        Ok(IssuedKey { key_id, token })
    }
    pub async fn revoke_scoped_key(&mut self, id: Uuid) -> StorageResult<()> {
        self.check_permission(Permission::Delete).await?;
        let result = sqlx::query(
            "UPDATE oc_api_keys SET revoked=1 WHERE tenant_id=? AND workspace_id=? AND id=?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(id)
        .execute(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        if result.rows_affected() == 0 {
            return Err(StorageError::NotFound);
        }
        self.audit("key.revoke", id, json!({})).await?;
        // Self-revocation was authorized under the IMMEDIATE write lock; its
        // intended effect must not invalidate its own commit.
        if id == self.principal_id {
            self.required.clear();
        }
        Ok(())
    }
}

impl SqliteTx {
    pub async fn resume_pending(&mut self, key: LedgerKey) -> StorageResult<()> {
        if key.scope != self.scope {
            return Err(StorageError::Forbidden);
        }
        sqlx::query("UPDATE oc_artifact_ledger SET state='pending',updated_at=? WHERE tenant_id=? AND workspace_id=? AND source_id=? AND version=? AND artifact_id=? AND surface=? AND generation=? AND state IN ('orphan','retry_wait')").bind(now_ms()).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(key.source.source_id).bind(key.source.version).bind(key.artifact_id).bind(key.surface.as_str()).bind(key.generation).execute(&mut *self.tx).await.map_err(sqlite_err)?;
        Ok(())
    }
}
