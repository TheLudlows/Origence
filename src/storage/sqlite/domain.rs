//! DomainTx adapter: authorization/idempotency envelope and private responsibility dispatch.
//! Delegates borrow the same transaction; no new transaction or connection boundary.
use super::*;

impl TursoTx {
    /// The role in this transaction. IMMEDIATE writes serialize with revoke;
    /// model/native IO must precede a fresh authorized write transaction.
    pub(super) async fn live_role(&mut self) -> StorageResult<String> {
        self.live_role_for_principal(self.principal_id).await
    }

    async fn live_role_for_principal(&mut self, principal_id: Uuid) -> StorageResult<String> {
        let key: Option<(String, Uuid)> = native_turso::query_as(
            "SELECT k.role,k.workspace_id FROM oc_api_keys k \
                 JOIN oc_workspaces target ON target.tenant_id=k.tenant_id AND target.id=? \
                 JOIN oc_workspaces root ON root.tenant_id=k.tenant_id AND root.id=k.workspace_id \
                 WHERE k.id=? AND k.tenant_id=? AND NOT k.revoked \
                   AND (k.workspace_id=target.id OR \
                       (k.allow_owned_workspaces=1 AND target.owner_workspace_id=k.workspace_id \
                        AND root.owner_workspace_id IS NULL))",
        )
        .bind(self.scope.workspace_id)
        .bind(principal_id)
        .bind(self.scope.tenant_id)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        match key {
            Some((role, workspace)) if workspace == self.scope.workspace_id => Ok(role),
            Some((role, _)) => Ok(if Permission::Write.granted_by(&role) {
                "writer"
            } else {
                "reader"
            }
            .into()),
            None => Err(StorageError::Forbidden),
        }
    }
}

impl DomainTx for TursoTx {
    fn scope(&self) -> Scope {
        self.scope
    }

    async fn check_permission(&mut self, permission: Permission) -> StorageResult<()> {
        let role = self.live_role().await?;
        if permission.granted_by(&role) {
            self.required.push(permission);
            Ok(())
        } else {
            Err(StorageError::Forbidden)
        }
    }

    async fn begin_command(
        &mut self,
        operation: &str,
        idempotency_key: &str,
        request_hash: &str,
    ) -> StorageResult<Option<Value>> {
        // Receipt access is authorized before lookup. The operation contract,
        // never a request field, determines the cross-principal dedup domain.
        self.check_permission(Permission::Write).await?;
        let dedup_scope = if operation == "message_batch.ingest" {
            "workspace".to_string()
        } else {
            format!("principal:{}", self.principal_id)
        };
        let row: Option<(String, String)> = native_turso::query_as(
            "SELECT request_hash, response FROM oc_commands \
             WHERE tenant_id = ? AND workspace_id = ? AND dedup_scope = ? AND operation = ? AND key = ?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(&dedup_scope)
        .bind(operation)
        .bind(idempotency_key)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        match row {
            Some((hash, response)) if hash == request_hash => {
                Ok(Some(serde_json::from_str(&response).unwrap_or(Value::Null)))
            }
            Some((_hash, _response)) => Err(StorageError::Conflict(format!(
                "idempotency key {idempotency_key} reused with different input"
            ))),
            None => {
                self.command = Some(PendingCommand {
                    dedup_scope,
                    operation: operation.to_string(),
                    key: idempotency_key.to_string(),
                    hash: request_hash.to_string(),
                });
                Ok(None)
            }
        }
    }

    async fn require_job_creator_permission(
        &mut self,
        job: Uuid,
        permission: Permission,
    ) -> StorageResult<()> {
        let creator: Option<Uuid> = native_turso::query_scalar(
            "SELECT created_by FROM oc_jobs WHERE tenant_id=? AND workspace_id=? AND id=?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(job)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        let creator = creator.ok_or(StorageError::NotFound)?;
        let role = self.live_role_for_principal(creator).await?;
        if !permission.granted_by(&role) {
            return Err(StorageError::Forbidden);
        }
        Ok(())
    }

    async fn record_source(&mut self, _source: SourceVersion) -> StorageResult<()> {
        // The domain operations that materialise a source (event/version) land
        // with the full adapter; the envelope itself records nothing.
        Ok(())
    }

    async fn enqueue(&mut self, work: WorkItem) -> StorageResult<()> {
        self.enqueue_inner(work).await
    }

    async fn audit(&mut self, action: &str, target: Uuid, detail: Value) -> StorageResult<()> {
        native_turso::query(
            "INSERT INTO oc_audit(tenant_id, workspace_id, id, actor, action, target, details, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(Uuid::new_v4())
        .bind(self.principal_id)
        .bind(action)
        .bind(target)
        .bind(serde_json::to_string(&detail).unwrap_or_default())
        .bind(now_ms())
        .execute(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        Ok(())
    }

    async fn finish_command(&mut self, response: Value) -> StorageResult<()> {
        let command = self.command.take().ok_or_else(|| {
            StorageError::Unavailable("finish_command without begin_command".to_string())
        })?;
        native_turso::query(
            "INSERT INTO oc_commands(tenant_id, workspace_id, principal_id, dedup_scope, operation, key, request_hash, response, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(self.principal_id)
        .bind(command.dedup_scope)
        .bind(command.operation)
        .bind(command.key)
        .bind(command.hash)
        .bind(serde_json::to_string(&response).unwrap_or_default())
        .bind(now_ms())
        .execute(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        Ok(())
    }

    async fn create_event(
        &mut self,
        kind: &str,
        content: &str,
        file: Option<Uuid>,
    ) -> StorageResult<Uuid> {
        self.create_event_inner(kind, content, file).await
    }

    async fn slot(&mut self, fact_key: &str) -> StorageResult<(Uuid, Option<i32>)> {
        self.slot_inner(fact_key).await
    }

    async fn identity_view(
        &mut self,
        identity: &crate::memory_identity::MemoryIdentity,
    ) -> StorageResult<Value> {
        self.identity_view_inner(identity).await
    }

    async fn identity_slot(
        &mut self,
        identity: &crate::memory_identity::MemoryIdentity,
    ) -> StorageResult<(Uuid, Option<i32>)> {
        self.identity_slot_inner(identity).await
    }

    async fn insert_knowledge_asset(&mut self, title: &str) -> StorageResult<Uuid> {
        self.insert_knowledge_asset_inner(title).await
    }

    async fn insert_file(
        &mut self,
        name: &str,
        media_type: &str,
        hash: &str,
        size: i64,
    ) -> StorageResult<Uuid> {
        self.insert_file_inner(name, media_type, hash, size).await
    }

    async fn file_exists(&mut self, id: Uuid) -> StorageResult<bool> {
        self.file_exists_inner(id).await
    }

    async fn file_hash(&mut self, id: Uuid) -> StorageResult<Option<String>> {
        self.file_hash_inner(id).await
    }

    async fn file_visible(&mut self, id: Uuid) -> StorageResult<bool> {
        self.file_visible_inner(id).await
    }

    async fn restore_source(&mut self, asset: Uuid, version: i32) -> StorageResult<Value> {
        self.restore_source_inner(asset, version).await
    }

    async fn delete_asset(&mut self, id: Uuid) -> StorageResult<u64> {
        self.delete_asset_inner(id).await
    }

    async fn retract_event(&mut self, id: Uuid) -> StorageResult<u64> {
        self.retract_event_inner(id).await
    }

    async fn delete_file(&mut self, id: Uuid) -> StorageResult<u64> {
        self.delete_file_inner(id).await
    }

    async fn cancel_affected_jobs(&mut self) -> StorageResult<()> {
        self.cancel_affected_jobs_inner().await
    }

    async fn job_row(&mut self, id: Uuid) -> StorageResult<Value> {
        self.job_row_inner(id).await
    }

    async fn set_job_action(&mut self, id: Uuid, action: &str) -> StorageResult<i64> {
        self.set_job_action_inner(id, action).await
    }

    async fn valid_targets(
        &mut self,
        asset: Option<Uuid>,
        source: Option<Uuid>,
    ) -> StorageResult<()> {
        self.valid_targets_inner(asset, source).await
    }

    async fn asset_view(&mut self, id: Uuid, version: Option<i32>) -> StorageResult<Value> {
        self.asset_view_inner(id, version).await
    }

    async fn event_view(&mut self, id: Uuid) -> StorageResult<Value> {
        self.event_view_inner(id).await
    }

    async fn job_view(&mut self, id: Uuid) -> StorageResult<Value> {
        self.job_view_inner(id).await
    }

    async fn event_content(
        &mut self,
        source: Uuid,
        require_active: bool,
    ) -> StorageResult<Option<String>> {
        self.event_content_inner(source, require_active).await
    }

    async fn event_file(&mut self, source: Uuid) -> StorageResult<Value> {
        self.event_file_inner(source).await
    }

    async fn chunks_for_version(
        &mut self,
        asset: Uuid,
        version: i32,
    ) -> StorageResult<Vec<(String, Value)>> {
        self.chunks_for_version_inner(asset, version).await
    }

    async fn asset_current_version(&mut self, asset: Uuid) -> StorageResult<Option<i32>> {
        self.asset_current_version_inner(asset).await
    }

    async fn insert_version(
        &mut self,
        asset: Uuid,
        version: i32,
        content: &str,
        content_hash: &str,
        source: Uuid,
        restored_from: Option<i32>,
        title: Option<&str>,
    ) -> StorageResult<()> {
        self.insert_version_inner(
            asset,
            version,
            content,
            content_hash,
            source,
            restored_from,
            title,
        )
        .await
    }

    async fn insert_chunk(
        &mut self,
        chunk: Uuid,
        asset: Uuid,
        version: i32,
        ordinal: i32,
        content: &str,
        locator: &Value,
        search_terms: &str,
    ) -> StorageResult<()> {
        self.insert_chunk_inner(
            chunk,
            asset,
            version,
            ordinal,
            content,
            locator,
            search_terms,
        )
        .await
    }

    async fn insert_summary(
        &mut self,
        summary: Uuid,
        chunk: Uuid,
        text: &str,
        model_revision: &str,
        search_terms: &str,
    ) -> StorageResult<()> {
        self.insert_summary_inner(summary, chunk, text, model_revision, search_terms)
            .await
    }

    async fn update_asset_version(
        &mut self,
        asset: Uuid,
        version: i32,
        title: Option<&str>,
    ) -> StorageResult<()> {
        self.update_asset_version_inner(asset, version, title).await
    }

    async fn active_job(
        &mut self,
        job: Uuid,
        generation: i64,
        run_token: i64,
    ) -> StorageResult<bool> {
        self.active_job_inner(job, generation, run_token).await
    }

    async fn cleanup_derived_chunks(&mut self) -> StorageResult<u64> {
        self.cleanup_derived_chunks_inner().await
    }

    async fn register_owner(
        &mut self,
        source: SourceVersion,
        artifact_type: &str,
        artifact_id: Uuid,
        chunk_id: Option<Uuid>,
    ) -> StorageResult<()> {
        self.register_owner_inner(source, artifact_type, artifact_id, chunk_id)
            .await
    }

    async fn detach_owners(&mut self, source: SourceVersion) -> StorageResult<u64> {
        self.detach_owners_inner(source).await
    }

    async fn owned_artifacts(&mut self) -> StorageResult<Value> {
        self.owned_artifacts_inner().await
    }

    async fn register_pending(&mut self, entry: LedgerEntry) -> StorageResult<()> {
        self.register_pending_inner(entry).await
    }

    async fn confirm_committed(&mut self, key: LedgerKey) -> StorageResult<()> {
        self.confirm_committed_inner(key).await
    }

    async fn fail_retryable(&mut self, key: LedgerKey, error: &str) -> StorageResult<()> {
        self.fail_retryable_inner(key, error).await
    }

    async fn fail_permanent(&mut self, key: LedgerKey, error: &str) -> StorageResult<()> {
        self.fail_permanent_inner(key, error).await
    }

    async fn settle_completed(
        &mut self,
        job: Uuid,
        outcome: &str,
        result: &Value,
    ) -> StorageResult<()> {
        self.settle_completed_inner(job, outcome, result).await
    }

    async fn settle_failed(
        &mut self,
        job: Uuid,
        generation: i64,
        run_token: i64,
        state: &str,
        error_code: &str,
    ) -> StorageResult<()> {
        self.settle_failed_inner(job, generation, run_token, state, error_code)
            .await
    }

    async fn commit(mut self) -> StorageResult<()> {
        if !self.required.is_empty() {
            let role = self.live_role().await?;
            for permission in &self.required {
                if !permission.granted_by(&role) {
                    return Err(StorageError::Forbidden);
                }
            }
        }
        self.tx.commit().await.map_err(turso_err)
    }

    async fn rollback(self) -> StorageResult<()> {
        self.tx.rollback().await.map_err(turso_err)
    }
}
