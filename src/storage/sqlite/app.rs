//! Local application queries. SQL stays inside the native Turso adapter.
use super::visibility::{CHUNK_SUPPORT_ACTIVE, CURRENT_CHUNK_ROWS};
use super::*;
use crate::types::SearchHit;

const HIT: &str = "SELECT c.id, c.asset_id,c.version,a.kind,v.title,c.content,c.locator,v.source_event_id,c.search_terms";

pub(super) fn hit(row: &native_turso::Row, score: f64) -> SearchHit {
    let locator: Value = serde_json::from_str(row.get("locator")).unwrap_or(Value::Null);
    let source_event_id = if locator["parser"] == crate::memory_semantics::PARSER {
        locator["source_event_id"]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
            .unwrap_or_else(|| row.get("source_event_id"))
    } else {
        row.get("source_event_id")
    };
    SearchHit {
        identity: None,
        chunk_id: row.get("id"),
        asset_id: row.get("asset_id"),
        version: row.get("version"),
        kind: row.get("kind"),
        title: row.get("title"),
        content: row.get("content"),
        locator,
        source_event_id,
        score,
    }
}

impl TursoStore {
    pub(super) async fn begin_scoped(
        &self,
        auth: AuthorizedScope,
        write: bool,
    ) -> StorageResult<TursoTx> {
        let tx = self
            .pool
            .begin_with(if write { "BEGIN IMMEDIATE" } else { "BEGIN" })
            .await
            .map_err(turso_err)?;
        let mut tx = TursoTx {
            tx,
            scope: auth.scope,
            principal_id: auth.principal_id,
            required: Vec::new(),
            command: None,
        };
        tx.live_role().await?;
        Ok(tx)
    }
    pub async fn begin_read(&self, auth: AuthorizedScope) -> StorageResult<TursoTx> {
        self.begin_scoped(auth, false).await
    }
    pub async fn workspace_scope(&self, workspace: Uuid) -> StorageResult<Scope> {
        let tenant = native_turso::query_scalar("SELECT tenant_id FROM oc_workspaces WHERE id=?")
            .bind(workspace)
            .fetch_optional(&self.pool)
            .await
            .map_err(turso_err)?
            .ok_or(StorageError::NotFound)?;
        Ok(Scope {
            tenant_id: tenant,
            workspace_id: workspace,
        })
    }
    pub async fn scopes(&self) -> StorageResult<Vec<Scope>> {
        let rows: Vec<(Uuid, Uuid)> = native_turso::query_as("SELECT tenant_id,id FROM oc_workspaces")
            .fetch_all(&self.pool)
            .await
            .map_err(turso_err)?;
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
        native_turso::query("UPDATE oc_jobs SET state='pending',run_token=run_token+1,updated_at=? WHERE state='processing' AND NOT cancel_requested").bind(now_ms()).execute(&self.pool).await.map_err(turso_err)?;
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
        native_turso::query("UPDATE oc_jobs SET state=CASE WHEN ? AND attempt<5 THEN 'retry_wait' ELSE ? END,error_code=?,next_retry_at=CASE WHEN ? AND attempt<5 THEN ? ELSE NULL END,updated_at=? WHERE tenant_id=? AND workspace_id=? AND id=? AND generation=? AND run_token=? AND state='processing'")
            .bind(retry).bind(state).bind(code).bind(retry).bind(now_ms()+2000).bind(now_ms()).bind(claim.scope.tenant_id).bind(claim.scope.workspace_id).bind(claim.job_id).bind(claim.generation).bind(claim.run_token).execute(&self.pool).await.map_err(turso_err)?;
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
            .map_err(turso_err)?;
        // Historical versions retain owners while the asset and source remain valid.
        let live = "EXISTS(SELECT 1 FROM oc_versions v JOIN oc_assets a ON a.tenant_id=v.tenant_id AND a.workspace_id=v.workspace_id AND a.id=v.asset_id JOIN oc_events e ON e.tenant_id=v.tenant_id AND e.workspace_id=v.workspace_id AND e.id=v.source_event_id WHERE v.tenant_id=o.tenant_id AND v.workspace_id=o.workspace_id AND v.source_event_id=o.source_id AND v.version=o.version AND NOT a.deleted AND e.state='active')";
        native_turso::query(&format!("DELETE FROM oc_artifact_owners AS o WHERE tenant_id=? AND workspace_id=? AND NOT {live}" )).bind(scope.tenant_id).bind(scope.workspace_id).execute(&mut *tx).await.map_err(turso_err)?;
        native_turso::query("DELETE FROM oc_artifact_owners AS o WHERE tenant_id=? AND workspace_id=? AND artifact_type IN ('chunk','summary') AND NOT EXISTS(SELECT 1 FROM oc_chunks c JOIN oc_assets a ON a.tenant_id=c.tenant_id AND a.workspace_id=c.workspace_id AND a.id=c.asset_id WHERE c.tenant_id=o.tenant_id AND c.workspace_id=o.workspace_id AND c.id=o.chunk_id AND NOT a.deleted)").bind(scope.tenant_id).bind(scope.workspace_id).execute(&mut *tx).await.map_err(turso_err)?;
        // Maintenance is privileged but still uses the identical scoped,
        // fail-closed dependency guard as all user-visible Learning reads.
        let mut guarded = TursoTx {
            tx,
            scope,
            principal_id: Uuid::nil(),
            required: Vec::new(),
            command: None,
        };
        guarded.detach_unsupported_learning_owners().await?;
        let mut tx = guarded.tx;
        let sources: Vec<Uuid> = native_turso::query_scalar("SELECT DISTINCT l.artifact_id FROM oc_artifact_ledger l WHERE l.tenant_id=? AND l.workspace_id=? AND l.surface IN ('vector','full_text') AND NOT EXISTS(SELECT 1 FROM oc_artifact_owners o WHERE o.tenant_id=l.tenant_id AND o.workspace_id=l.workspace_id AND o.artifact_id=l.artifact_id AND o.artifact_type='chunk')").bind(scope.tenant_id).bind(scope.workspace_id).fetch_all(&mut *tx).await.map_err(turso_err)?;
        let rows: Vec<(String,Uuid)> = native_turso::query_as("SELECT DISTINCT artifact_type,artifact_id FROM oc_artifact_owners WHERE tenant_id=? AND workspace_id=? AND artifact_type IN ('entity','relation')").bind(scope.tenant_id).bind(scope.workspace_id).fetch_all(&mut *tx).await.map_err(turso_err)?;
        tx.commit().await.map_err(turso_err)?;
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
            .map_err(turso_err)?;
        let invalid = "NOT EXISTS(SELECT 1 FROM oc_artifact_owners o WHERE o.tenant_id=c.tenant_id AND o.workspace_id=c.workspace_id AND o.artifact_id=c.id AND o.artifact_type='chunk')";
        native_turso::query(&format!("DELETE FROM oc_summaries WHERE tenant_id=? AND workspace_id=? AND chunk_id IN (SELECT c.id FROM oc_chunks c WHERE c.tenant_id=? AND c.workspace_id=? AND {invalid})")).bind(scope.tenant_id).bind(scope.workspace_id).bind(scope.tenant_id).bind(scope.workspace_id).execute(&mut *tx).await.map_err(turso_err)?;
        native_turso::query(&format!(
            "DELETE FROM oc_chunks AS c WHERE tenant_id=? AND workspace_id=? AND {invalid}"
        ))
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .execute(&mut *tx)
        .await
        .map_err(turso_err)?;
        native_turso::query("UPDATE oc_index_entries SET state='removed' WHERE tenant_id=? AND workspace_id=? AND NOT EXISTS(SELECT 1 FROM oc_chunks c WHERE c.tenant_id=oc_index_entries.tenant_id AND c.workspace_id=oc_index_entries.workspace_id AND c.id=oc_index_entries.artifact_id)").bind(scope.tenant_id).bind(scope.workspace_id).execute(&mut *tx).await.map_err(turso_err)?;
        native_turso::query("UPDATE oc_artifact_ledger SET state='orphan',updated_at=? WHERE tenant_id=? AND workspace_id=? AND NOT EXISTS(SELECT 1 FROM oc_artifact_owners o WHERE o.tenant_id=oc_artifact_ledger.tenant_id AND o.workspace_id=oc_artifact_ledger.workspace_id AND o.source_id=oc_artifact_ledger.source_id AND o.version=oc_artifact_ledger.version AND o.artifact_id=oc_artifact_ledger.artifact_id)").bind(now_ms()).bind(scope.tenant_id).bind(scope.workspace_id).execute(&mut *tx).await.map_err(turso_err)?;
        tx.commit().await.map_err(turso_err)
    }
}

impl TursoTx {
    /// Restrict retrieval candidates by an exact identity before top-k/ranking.
    /// This does not allocate a memory slot or reinterpret legacy fact keys.
    pub async fn identity_candidate_asset(
        &mut self,
        identity: &crate::memory_identity::MemoryIdentity,
    ) -> StorageResult<Option<Uuid>> {
        if !identity_table_present(&mut self.tx).await? {
            return Ok(None);
        }
        let key = identity
            .key(self.scope)
            .map_err(|_| StorageError::Conflict("invalid memory identity".into()))?;
        let asset: Option<Uuid> = native_turso::query_scalar(
            "SELECT i.asset_id FROM oc_memory_identities i
             JOIN oc_assets a ON a.tenant_id=i.tenant_id
                 AND a.workspace_id=i.workspace_id AND a.id=i.asset_id
             WHERE i.tenant_id=? AND i.workspace_id=?
                 AND i.identity_key=? AND a.kind='memory' AND NOT a.deleted",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(key)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        if let Some(id) = asset
            && self.identity_for_asset(id).await?.as_ref() != Some(identity)
        {
            return Err(StorageError::Unavailable(
                "memory identity binding mismatch".into(),
            ));
        }
        Ok(asset)
    }

    pub async fn asset_metadata(&mut self, id: Uuid) -> StorageResult<Value> {
        let row: Option<(String, Option<i32>, bool)> = native_turso::query_as("SELECT kind,current_version,deleted FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND id=?").bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(id).fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        let (kind, current_version, deleted) = row.ok_or(StorageError::NotFound)?;
        Ok(json!({"kind":kind,"current_version":current_version,"deleted":deleted}))
    }
    pub async fn save_publication(&mut self, job: Uuid, plan: &Value) -> StorageResult<()> {
        native_turso::query("UPDATE oc_jobs SET payload=json_set(payload,'$.publication',json(?)),updated_at=? WHERE tenant_id=? AND workspace_id=? AND id=?").bind(plan.to_string()).bind(now_ms()).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(job).execute(&mut *self.tx).await.map_err(turso_err)?;
        Ok(())
    }
    pub async fn index_ready(
        &mut self,
        chunk: Uuid,
        profile: &str,
        dimension: usize,
        generation: i64,
    ) -> StorageResult<()> {
        native_turso::query("INSERT INTO oc_index_entries(tenant_id,workspace_id,artifact_id,field,model_id,dimension,generation,state,created_at) VALUES(?,?,?,'content',?,?,?,'ready',?) ON CONFLICT DO UPDATE SET state='ready'").bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(chunk).bind(profile).bind(dimension as i64).bind(generation).bind(now_ms()).execute(&mut *self.tx).await.map_err(turso_err)?;
        Ok(())
    }
    pub async fn vector_generations(
        &mut self,
        profile: &str,
        dimension: usize,
    ) -> StorageResult<Vec<i64>> {
        native_turso::query_scalar("SELECT DISTINCT generation FROM oc_index_entries WHERE tenant_id=? AND workspace_id=? AND model_id=? AND dimension=? AND state='ready'").bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(profile).bind(dimension as i64).fetch_all(&mut *self.tx).await.map_err(turso_err)
    }
    pub async fn vector_candidates(
        &mut self,
        profile: &str,
        dimension: usize,
        generation: i64,
        asset_filter: Option<Uuid>,
    ) -> StorageResult<Vec<Uuid>> {
        let asset_clause = if asset_filter.is_some() {
            " AND c.asset_id=?"
        } else {
            ""
        };
        let sql = format!(
            "SELECT c.id,c.asset_id,c.version {CURRENT_CHUNK_ROWS} AND {CHUNK_SUPPORT_ACTIVE}{asset_clause} AND EXISTS(SELECT 1 FROM oc_index_entries i WHERE i.tenant_id=c.tenant_id AND i.workspace_id=c.workspace_id AND i.artifact_id=c.id AND i.model_id=? AND i.dimension=? AND i.generation=? AND i.state='ready') AND EXISTS(SELECT 1 FROM oc_artifact_ledger l WHERE l.tenant_id=c.tenant_id AND l.workspace_id=c.workspace_id AND l.artifact_id=c.id AND l.source_id=v.source_event_id AND l.version=v.version AND l.generation=? AND l.surface='vector' AND l.state='committed')"
        );
        let query = native_turso::query_as::< (Uuid, Uuid, i32)>(&sql)
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id);
        let query = if let Some(asset) = asset_filter {
            query.bind(asset)
        } else {
            query
        };
        let rows = query
            .bind(profile)
            .bind(dimension as i64)
            .bind(generation)
            .bind(generation)
            .fetch_all(&mut *self.tx)
            .await
            .map_err(turso_err)?;
        let mut visibility = HashMap::new();
        let mut candidates = Vec::new();
        for (chunk, asset, version) in rows {
            let visible = if let Some(visible) = visibility.get(&(asset, version)) {
                *visible
            } else {
                let visible = self.semantic_version_visible(asset, version).await?;
                visibility.insert((asset, version), visible);
                visible
            };
            if visible {
                candidates.push(chunk);
            }
        }
        // The native backend receives only eligible IDs, before its top-k.
        Ok(candidates)
    }
    pub async fn vector_hit(
        &mut self,
        id: Uuid,
        source: SourceVersion,
        profile: &str,
        generation: i64,
    ) -> StorageResult<Option<SearchHit>> {
        let row = native_turso::query(&format!("{HIT} {CURRENT_CHUNK_ROWS} AND {CHUNK_SUPPORT_ACTIVE} AND c.id=? AND v.source_event_id=? AND v.version=? AND EXISTS(SELECT 1 FROM oc_index_entries i WHERE i.tenant_id=c.tenant_id AND i.workspace_id=c.workspace_id AND i.artifact_id=c.id AND i.model_id=? AND i.generation=? AND i.state='ready') AND EXISTS(SELECT 1 FROM oc_artifact_ledger l WHERE l.tenant_id=c.tenant_id AND l.workspace_id=c.workspace_id AND l.artifact_id=c.id AND l.source_id=v.source_event_id AND l.version=v.version AND l.generation=? AND l.surface='vector' AND l.state='committed')")).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(id).bind(source.source_id).bind(source.version).bind(profile).bind(generation).bind(generation).fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
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
    pub async fn visible_hit(&mut self, id: Uuid) -> StorageResult<Option<SearchHit>> {
        let row = native_turso::query(&format!(
            "{HIT} {CURRENT_CHUNK_ROWS} AND {CHUNK_SUPPORT_ACTIVE} AND c.id=?"
        ))
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(id)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
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
    pub async fn keyword_hits(
        &mut self,
        terms: &str,
        summaries: bool,
        asset_filter: Option<Uuid>,
    ) -> StorageResult<Vec<SearchHit>> {
        let select = if summaries {
            "SELECT c.id,c.asset_id,c.version,a.kind,v.title,c.content,c.locator,v.source_event_id,(SELECT group_concat(s.search_terms,' ') FROM oc_summaries s WHERE s.tenant_id=c.tenant_id AND s.workspace_id=c.workspace_id AND s.chunk_id=c.id) search_terms"
        } else {
            HIT
        };
        let asset_clause = if asset_filter.is_some() {
            " AND c.asset_id=?"
        } else {
            ""
        };
        let sql = format!("{select} {CURRENT_CHUNK_ROWS} AND {CHUNK_SUPPORT_ACTIVE}{asset_clause}");
        let query = native_turso::query(&sql)
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id);
        let query = if let Some(asset) = asset_filter {
            query.bind(asset)
        } else {
            query
        };
        let rows = query.fetch_all(&mut *self.tx).await.map_err(turso_err)?;
        let words: std::collections::HashSet<_> = terms.split_whitespace().collect();
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let mut hits = Vec::new();
        let mut visibility = HashMap::new();
        for row in rows {
            let key: (Uuid, i32) = (row.get("asset_id"), row.get("version"));
            let visible = if let Some(visible) = visibility.get(&key) {
                *visible
            } else {
                let visible = self.semantic_version_visible(key.0, key.1).await?;
                visibility.insert(key, visible);
                visible
            };
            if !visible {
                continue;
            }
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
        let rows = native_turso::query(&format!("{HIT} {CURRENT_CHUNK_ROWS} AND {CHUNK_SUPPORT_ACTIVE} AND EXISTS(SELECT 1 FROM oc_artifact_owners o JOIN oc_artifact_ledger l ON l.tenant_id=o.tenant_id AND l.workspace_id=o.workspace_id AND l.source_id=o.source_id AND l.version=o.version AND l.artifact_id=o.artifact_id AND l.surface='graph' AND l.state='committed' WHERE o.tenant_id=c.tenant_id AND o.workspace_id=c.workspace_id AND o.artifact_id=? AND o.source_id=v.source_event_id AND o.version=v.version) ORDER BY c.id")).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(artifact).fetch_all(&mut *self.tx).await.map_err(turso_err)?;
        let mut evidence = Vec::new();
        let mut visibility = HashMap::new();
        for row in rows {
            let key: (Uuid, i32) = (row.get("asset_id"), row.get("version"));
            let visible = if let Some(visible) = visibility.get(&key) {
                *visible
            } else {
                let visible = self.semantic_version_visible(key.0, key.1).await?;
                visibility.insert(key, visible);
                visible
            };
            if !visible {
                continue;
            }
            let h = hit(&row, 0.0);
            evidence.push(json!({"asset_id":h.asset_id,"version":h.version,"chunk_id":h.chunk_id,"source_event_id":h.source_event_id,"locator":h.locator}));
            if evidence.len() == 20 {
                break;
            }
        }
        Ok(evidence)
    }
    pub async fn issue_scoped_key(&mut self, role: &str) -> StorageResult<IssuedKey> {
        self.issue_scoped_key_with_capabilities(role, KeyCapabilities::default())
            .await
    }
    pub async fn issue_scoped_key_with_capabilities(
        &mut self,
        role: &str,
        capabilities: KeyCapabilities,
    ) -> StorageResult<IssuedKey> {
        self.check_permission(Permission::Delete).await?;
        if !ROLES.contains(&role) {
            return Err(StorageError::Conflict("invalid role".into()));
        }
        if capabilities.allow_owned_workspaces {
            let root: bool = native_turso::query_scalar(
                "SELECT owner_workspace_id IS NULL FROM oc_workspaces WHERE tenant_id=? AND id=?",
            )
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .fetch_one(&mut *self.tx)
            .await
            .map_err(turso_err)?;
            if !root {
                return Err(StorageError::Forbidden);
            }
        }
        let key_id = Uuid::new_v4();
        let token = format!("oc_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        native_turso::query("INSERT INTO oc_api_keys(id,tenant_id,workspace_id,token_hash,role,allow_owned_workspaces,created_at) VALUES(?,?,?,?,?,?,?)").bind(key_id).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(hash(&token)).bind(role).bind(capabilities.allow_owned_workspaces).bind(now_ms()).execute(&mut *self.tx).await.map_err(turso_err)?;
        self.audit(
            "key.issue",
            key_id,
            json!({"role":role,"allow_owned_workspaces":capabilities.allow_owned_workspaces}),
        )
        .await?;
        Ok(IssuedKey { key_id, token })
    }
    pub async fn revoke_scoped_key(&mut self, id: Uuid) -> StorageResult<()> {
        self.check_permission(Permission::Delete).await?;
        let result = native_turso::query(
            "UPDATE oc_api_keys SET revoked=1 WHERE tenant_id=? AND workspace_id=? AND id=?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(id)
        .execute(&mut *self.tx)
        .await
        .map_err(turso_err)?;
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

impl TursoTx {
    pub async fn resume_pending(&mut self, key: LedgerKey) -> StorageResult<()> {
        if key.scope != self.scope {
            return Err(StorageError::Forbidden);
        }
        native_turso::query("UPDATE oc_artifact_ledger SET state='pending',updated_at=? WHERE tenant_id=? AND workspace_id=? AND source_id=? AND version=? AND artifact_id=? AND surface=? AND generation=? AND state IN ('orphan','retry_wait')").bind(now_ms()).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(key.source.source_id).bind(key.source.version).bind(key.artifact_id).bind(key.surface.as_str()).bind(key.generation).execute(&mut *self.tx).await.map_err(turso_err)?;
        Ok(())
    }
}
