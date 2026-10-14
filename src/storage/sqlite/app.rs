//! Local application queries. SQL stays inside the native Turso adapter.
use super::*;
use crate::types::SearchHit;

mod maintenance;

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
        let rows: Vec<(Uuid, Uuid)> =
            native_turso::query_as("SELECT tenant_id,id FROM oc_workspaces")
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
        maintenance::detach_invalid_roots(&mut tx, scope).await?;
        maintenance::detach_invalid_chunks(&mut tx, scope).await?;
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
        let sources = maintenance::unowned_native_artifacts(&mut tx, scope).await?;
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
        maintenance::remove_unowned_chunks(&mut tx, scope).await?;
        maintenance::remove_missing_indexes(&mut tx, scope).await?;
        maintenance::orphan_unowned_ledger(&mut tx, scope).await?;
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
        let key = identity
            .key(self.scope)
            .map_err(|_| StorageError::Conflict("invalid memory identity".into()))?;
        let asset: Option<Uuid> = native_turso::query_scalar(
            "SELECT asset_id FROM oc_memory_identities WHERE tenant_id=? AND workspace_id=? AND identity_key=?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(key)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        let asset = if let Some(id) = asset {
            let live: Option<Uuid> = native_turso::query_scalar(
                "SELECT id FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND id=? AND kind='memory' AND NOT deleted",
            ).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(id)
                .fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
            live
        } else {
            None
        };
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
        if self.save_message_publication_inner(job, plan).await? {
            return Ok(());
        }
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
        let ids: Vec<read_model::AssociationKey> = native_turso::query_scalar(
            "SELECT artifact_id FROM oc_index_entries WHERE tenant_id=? AND workspace_id=? AND model_id=? AND dimension=? AND generation=? AND state='ready'",
        ).bind(self.scope.tenant_id).bind(self.scope.workspace_id)
            .bind(profile).bind(dimension).bind(generation)
            .fetch_all(&mut *self.tx).await.map_err(turso_err)?;
        self.index_candidate_ids(&ids, asset_filter, "vector", generation)
            .await
    }

    async fn vector_hits(
        &mut self,
        keys: &[read_model::NativeHitKey],
        profile: &str,
        dimension: Option<usize>,
    ) -> StorageResult<HashMap<read_model::NativeHitKey, SearchHit>> {
        let ready = self.ready_vector_keys(keys, profile, dimension).await?;
        self.indexed_hits(&ready, "vector").await
    }

    pub async fn vector_hit(
        &mut self,
        id: Uuid,
        source: SourceVersion,
        profile: &str,
        generation: i64,
    ) -> StorageResult<Option<SearchHit>> {
        let key = (id, source, generation);
        Ok(self.vector_hits(&[key], profile, None).await?.remove(&key))
    }

    /// Exact publication checks across native generations, loaded as one batch.
    #[cfg(feature = "local-storage")]
    pub(crate) async fn vector_hits_exact(
        &mut self,
        keys: &[read_model::NativeHitKey],
        profile: &str,
        dimension: usize,
    ) -> StorageResult<HashMap<read_model::NativeHitKey, SearchHit>> {
        self.vector_hits(keys, profile, Some(dimension)).await
    }

    #[cfg(feature = "local-storage")]
    pub(crate) async fn vector_hit_exact(
        &mut self,
        id: Uuid,
        source: SourceVersion,
        profile: &str,
        dimension: usize,
        generation: i64,
    ) -> StorageResult<Option<SearchHit>> {
        let key = (id, source, generation);
        Ok(self
            .vector_hits_exact(&[key], profile, dimension)
            .await?
            .remove(&key))
    }

    pub async fn visible_hit(&mut self, id: Uuid) -> StorageResult<Option<SearchHit>> {
        Ok(self.visible_hits(&[id]).await?.remove(&id))
    }

    pub async fn summary_hits(
        &mut self,
        terms: &str,
        asset_filter: Option<Uuid>,
    ) -> StorageResult<Vec<SearchHit>> {
        let raw = read_model::current_rows(&mut self.tx, self.scope, None, asset_filter).await?;
        let positions = self.local_current_positions(&raw).await?;
        let summary_terms = self.current_summary_terms(&raw, &positions).await?;
        let words: std::collections::HashSet<_> = terms.split_whitespace().collect();
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let keys = positions
            .iter()
            .map(|index| raw.version_key(*index))
            .collect::<StorageResult<Vec<_>>>()?;
        let visible = self.visible_version_keys(&keys).await?;
        let mut hits = Vec::new();
        for (index, key) in positions.into_iter().zip(keys) {
            if !visible.contains(&key) {
                continue;
            }
            let text: Option<String> = summary_terms
                .get(&index)
                .map(|row| row.try_get("search_terms").map_err(turso_err))
                .transpose()?
                .flatten();
            let tokens: std::collections::HashSet<_> =
                text.as_deref().unwrap_or("").split_whitespace().collect();
            if words.is_subset(&tokens) {
                hits.push(raw.hit(index, words.len() as f64 / tokens.len().max(1) as f64)?);
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
        let owners = read_model::rows_by_ids(
            &mut self.tx,
            self.scope,
            "oc_artifact_owners",
            "source_id,version",
            "artifact_id",
            &[artifact],
        )
        .await?;
        let owners: std::collections::HashSet<(Uuid, i32)> = owners
            .into_iter()
            .map(|row| {
                Ok((
                    row.try_get("source_id").map_err(turso_err)?,
                    row.try_get("version").map_err(turso_err)?,
                ))
            })
            .collect::<StorageResult<_>>()?;
        let ledger = read_model::rows_by_ids(
            &mut self.tx,
            self.scope,
            "oc_artifact_ledger",
            "source_id,version,surface,state",
            "artifact_id",
            &[artifact],
        )
        .await?;
        let mut roots = std::collections::HashSet::new();
        for row in ledger {
            if row.try_get::<String>("surface").map_err(turso_err)? == "graph"
                && row.try_get::<String>("state").map_err(turso_err)? == "committed"
            {
                let key = (
                    row.try_get("source_id").map_err(turso_err)?,
                    row.try_get("version").map_err(turso_err)?,
                );
                if owners.contains(&key) {
                    roots.insert(key);
                }
            }
        }
        if roots.is_empty() {
            return Ok(Vec::new());
        }
        self.graph_rows_for_sources(&roots).await
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
