//! Local application queries. SQL stays inside the native SQLite adapter.
use super::*;
use crate::types::SearchHit;

mod maintenance;

impl SqliteStore {
    pub(super) async fn begin_scoped(
        &self,
        auth: AuthorizedScope,
        write: bool,
        permission: Option<Permission>,
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
        let role = tx.live_role().await?;
        if let Some(permission) = permission {
            if !permission.granted_by(&role) {
                return Err(StorageError::Forbidden);
            }
            tx.required.push(permission);
        }
        Ok(tx)
    }
    pub async fn begin_read(&self, auth: AuthorizedScope) -> StorageResult<SqliteTx> {
        self.begin_scoped(auth, false, None).await
    }

    #[cfg(feature = "host")]
    pub(crate) async fn begin_read_checked(
        &self,
        auth: AuthorizedScope,
        permission: Permission,
    ) -> StorageResult<SqliteTx> {
        self.begin_scoped(auth, false, Some(permission)).await
    }

    #[cfg(feature = "host")]
    pub(crate) async fn begin_checked(
        &self,
        auth: AuthorizedScope,
        permission: Permission,
    ) -> StorageResult<SqliteTx> {
        self.begin_scoped(auth, true, Some(permission)).await
    }
    pub async fn workspace_scope(&self, workspace: Uuid) -> StorageResult<Scope> {
        let tenant = sqlite_db::query_scalar("SELECT tenant_id FROM oc_workspaces WHERE id=?")
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
        let rows: Vec<(Uuid, Uuid)> = sqlite_db::query_as("SELECT tenant_id,id FROM oc_workspaces")
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
        sqlite_db::query("UPDATE oc_jobs SET state='pending',run_token=run_token+1,updated_at=? WHERE state='processing' AND NOT cancel_requested").bind(now_ms()).execute(&self.pool).await.map_err(sqlite_err)?;
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
        sqlite_db::query("UPDATE oc_jobs SET state=CASE WHEN ? AND attempt<5 THEN 'retry_wait' ELSE ? END,error_code=?,next_retry_at=CASE WHEN ? AND attempt<5 THEN ? ELSE NULL END,updated_at=? WHERE tenant_id=? AND workspace_id=? AND id=? AND generation=? AND run_token=? AND state='processing'")
            .bind(retry).bind(state).bind(code).bind(retry).bind(now_ms()+2000).bind(now_ms()).bind(claim.scope.tenant_id).bind(claim.scope.workspace_id).bind(claim.job_id).bind(claim.generation).bind(claim.run_token).execute(&self.pool).await.map_err(sqlite_err)?;
        Ok(())
    }
    /// Detach invalid provenance and find unowned AnySeek chunk IDs.
    /// The durable ledger retains these IDs across failed or repeated native cleanup.
    pub async fn cleanup_plan(&self, scope: Scope) -> StorageResult<Vec<Uuid>> {
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(sqlite_err)?;
        maintenance::detach_invalid_roots(&mut tx, scope).await?;
        maintenance::detach_invalid_chunks(&mut tx, scope).await?;
        // Maintenance is privileged but still uses the identical scoped,
        // fail-closed dependency guard as all user-visible Learning reads.
        let mut guarded = SqliteTx {
            tx,
            scope,
            principal_id: Uuid::nil(),
            required: Vec::new(),
            command: None,
        };
        guarded.detach_unsupported_learning_owners().await?;
        let mut tx = guarded.tx;
        let chunks = maintenance::unowned_native_artifacts(&mut tx, scope).await?;
        tx.commit().await.map_err(sqlite_err)?;
        Ok(chunks)
    }

    pub async fn finish_cleanup(&self, scope: Scope) -> StorageResult<()> {
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(sqlite_err)?;
        maintenance::remove_unowned_chunks(&mut tx, scope).await?;
        maintenance::remove_missing_indexes(&mut tx, scope).await?;
        maintenance::orphan_unowned_ledger(&mut tx, scope).await?;
        tx.commit().await.map_err(sqlite_err)
    }
}

impl SqliteTx {
    /// Restrict retrieval candidates by an exact identity before top-k/ranking.
    /// This does not allocate a memory slot or reinterpret legacy fact keys.
    pub async fn identity_candidate_asset(
        &mut self,
        identity: &crate::memory_identity::MemoryIdentity,
    ) -> StorageResult<Option<Uuid>> {
        let key = identity
            .key(self.scope)
            .map_err(|_| StorageError::Conflict("invalid memory identity".into()))?;
        let row = sqlite_db::query(
            "SELECT id,kind,identity_key,identity_json FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND identity_key=? AND kind='memory' AND NOT deleted",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(key)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let asset = row.try_get("id").map_err(sqlite_err)?;
        if identity::checked_identity(self.scope, &row)?.as_ref() != Some(identity) {
            return Err(StorageError::Unavailable(
                "memory identity binding mismatch".into(),
            ));
        }
        Ok(Some(asset))
    }

    pub async fn asset_metadata(&mut self, id: Uuid) -> StorageResult<Value> {
        let row: Option<(String, Option<i32>, bool)> = sqlite_db::query_as("SELECT kind,current_version,deleted FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND id=?").bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(id).fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        let (kind, current_version, deleted) = row.ok_or(StorageError::NotFound)?;
        Ok(json!({"kind":kind,"current_version":current_version,"deleted":deleted}))
    }
    pub async fn save_publication(&mut self, job: Uuid, plan: &Value) -> StorageResult<()> {
        if self.save_message_publication_inner(job, plan).await? {
            return Ok(());
        }
        sqlite_db::query("UPDATE oc_jobs SET payload=json_set(payload,'$.publication',json(?)),updated_at=? WHERE tenant_id=? AND workspace_id=? AND id=?").bind(plan.to_string()).bind(now_ms()).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(job).execute(&mut *self.tx).await.map_err(sqlite_err)?;
        Ok(())
    }
    #[cfg(feature = "host")]
    pub(crate) async fn save_lexical_publication(
        &mut self,
        job: Uuid,
        seal: &Value,
    ) -> StorageResult<()> {
        sqlite_db::query(
            "UPDATE oc_jobs SET payload=json_set(json_remove(payload,'$.publication'),'$.lexical_publication',json(?)),updated_at=? WHERE tenant_id=? AND workspace_id=? AND id=?",
        )
        .bind(seal.to_string())
        .bind(now_ms())
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(job)
        .execute(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        Ok(())
    }
    pub async fn index_ready(
        &mut self,
        chunk: Uuid,
        profile: &str,
        dimension: usize,
        generation: i64,
    ) -> StorageResult<()> {
        sqlite_db::query("INSERT INTO oc_index_entries(tenant_id,workspace_id,artifact_id,field,model_id,dimension,generation,state,created_at) VALUES(?,?,?,'content',?,?,?,'ready',?) ON CONFLICT DO UPDATE SET state='ready'").bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(chunk).bind(profile).bind(dimension as i64).bind(generation).bind(now_ms()).execute(&mut *self.tx).await.map_err(sqlite_err)?;
        Ok(())
    }
    pub async fn vector_generations(
        &mut self,
        profile: &str,
        dimension: usize,
    ) -> StorageResult<Vec<i64>> {
        sqlite_db::query_scalar("SELECT DISTINCT generation FROM oc_index_entries WHERE tenant_id=? AND workspace_id=? AND model_id=? AND dimension=? AND state='ready'").bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(profile).bind(dimension as i64).fetch_all(&mut *self.tx).await.map_err(sqlite_err)
    }
    pub async fn vector_candidates(
        &mut self,
        profile: &str,
        dimension: usize,
        generation: i64,
        asset_filter: Option<Uuid>,
    ) -> StorageResult<Vec<Uuid>> {
        let ids: Vec<read_model::AssociationKey> = sqlite_db::query_scalar(
            "SELECT artifact_id FROM oc_index_entries WHERE tenant_id=? AND workspace_id=? AND model_id=? AND dimension=? AND generation=? AND state='ready'",
        ).bind(self.scope.tenant_id).bind(self.scope.workspace_id)
            .bind(profile).bind(dimension).bind(generation)
            .fetch_all(&mut *self.tx).await.map_err(sqlite_err)?;
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
    #[cfg(feature = "host")]
    pub(crate) async fn vector_hits_exact(
        &mut self,
        keys: &[read_model::NativeHitKey],
        profile: &str,
        dimension: usize,
    ) -> StorageResult<HashMap<read_model::NativeHitKey, SearchHit>> {
        self.vector_hits(keys, profile, Some(dimension)).await
    }

    #[cfg(feature = "host")]
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

    pub async fn graph_evidence(&mut self, artifact: Uuid) -> StorageResult<Vec<Value>> {
        self.graph_evidence_matching(artifact, None).await
    }

    #[cfg(feature = "host")]
    pub(crate) async fn graph_evidence_for_publication(
        &mut self,
        artifact: Uuid,
        source: SourceVersion,
        generation: i64,
    ) -> StorageResult<Vec<Value>> {
        self.graph_evidence_matching(artifact, Some((source, generation)))
            .await
    }

    async fn graph_evidence_matching(
        &mut self,
        artifact: Uuid,
        publication: Option<(SourceVersion, i64)>,
    ) -> StorageResult<Vec<Value>> {
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
                    row.try_get("source_id").map_err(sqlite_err)?,
                    row.try_get("version").map_err(sqlite_err)?,
                ))
            })
            .collect::<StorageResult<_>>()?;
        let ledger = read_model::rows_by_ids(
            &mut self.tx,
            self.scope,
            "oc_artifact_ledger",
            "source_id,version,generation,surface,state",
            "artifact_id",
            &[artifact],
        )
        .await?;
        let mut roots = std::collections::HashSet::new();
        for row in ledger {
            if row.try_get::<String>("surface").map_err(sqlite_err)? == "graph"
                && row.try_get::<String>("state").map_err(sqlite_err)? == "committed"
            {
                let key = (
                    row.try_get("source_id").map_err(sqlite_err)?,
                    row.try_get("version").map_err(sqlite_err)?,
                );
                if let Some((source, generation)) = publication
                    && (key != (source.source_id, source.version)
                        || row.try_get::<i64>("generation").map_err(sqlite_err)? != generation)
                {
                    continue;
                }
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
        self.check_permission(Permission::WorkspaceAdmin).await?;
        self.issue_scoped_key_inner(role, capabilities).await
    }

    pub(super) async fn issue_scoped_key_inner(
        &mut self,
        role: &str,
        capabilities: KeyCapabilities,
    ) -> StorageResult<IssuedKey> {
        if role == "workspace" || !Permission::Read.granted_by(role) {
            return Err(StorageError::Conflict("invalid role".into()));
        }
        if super::workspace_key::has_workspace_key(&mut self.tx, self.scope).await? {
            return Err(StorageError::Forbidden);
        }
        if capabilities.allow_owned_workspaces {
            let root: bool = sqlite_db::query_scalar(
                "SELECT owner_workspace_id IS NULL FROM oc_workspaces WHERE tenant_id=? AND id=?",
            )
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .fetch_one(&mut *self.tx)
            .await
            .map_err(sqlite_err)?;
            if !root {
                return Err(StorageError::Forbidden);
            }
        }
        let key_id = Uuid::new_v4();
        let token = format!("oc_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        sqlite_db::query("INSERT INTO oc_api_keys(id,logical_key_id,tenant_id,workspace_id,token_hash,role,allow_owned_workspaces,created_at) VALUES(?,?,?,?,?,?,?,?)").bind(key_id).bind(key_id).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(hash(&token)).bind(role).bind(capabilities.allow_owned_workspaces).bind(now_ms()).execute(&mut *self.tx).await.map_err(sqlite_err)?;
        self.audit(
            "key.issue",
            key_id,
            json!({"role":role,"allow_owned_workspaces":capabilities.allow_owned_workspaces}),
        )
        .await?;
        Ok(IssuedKey { key_id, token })
    }
    pub async fn revoke_scoped_key(&mut self, id: Uuid) -> StorageResult<()> {
        self.check_permission(Permission::WorkspaceAdmin).await?;
        self.revoke_scoped_key_inner(id).await
    }

    pub(super) async fn revoke_scoped_key_inner(&mut self, id: Uuid) -> StorageResult<()> {
        let result = sqlite_db::query(
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
        sqlite_db::query("UPDATE oc_artifact_ledger SET state='pending',updated_at=? WHERE tenant_id=? AND workspace_id=? AND source_id=? AND version=? AND artifact_id=? AND surface=? AND generation=? AND state IN ('orphan','retry_wait')").bind(now_ms()).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(key.source.source_id).bind(key.source.version).bind(key.artifact_id).bind(key.surface.as_str()).bind(key.generation).execute(&mut *self.tx).await.map_err(sqlite_err)?;
        Ok(())
    }
}
