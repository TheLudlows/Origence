use crate::{
    error::{AppError, Result},
    jobs::JobPayload,
    models::Models,
    parsing,
    storage::{
        local_blob::LocalBlobStore,
        sqlite::{SqliteStore, SqliteTx},
        *,
    },
    types::*,
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use uuid::Uuid;

mod catalog;
mod content;
mod deployment;
pub(crate) mod global_knowledge;
pub use global_knowledge::GlobalKnowledgeDecision;
mod workspaces;

#[derive(Clone)]
pub struct Service {
    pub engine: Arc<LocalEngine>,
    pub models: Models,
    pub worker_running: Arc<AtomicBool>,
}

#[derive(Clone, Copy)]
pub(crate) enum ReadAccess<'a> {
    Workspace(&'a AuthContext),
    Deployment { token: &'a str, workspace: Uuid },
}
pub(crate) fn decode<T: DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value)
        .map_err(|_| AppError::Invalid("invalid stored/request value".into()))
}
pub(crate) fn scope(a: &AuthContext) -> Scope {
    Scope {
        tenant_id: a.tenant_id,
        workspace_id: a.workspace_id,
    }
}
pub(crate) fn authorized(a: &AuthContext) -> AuthorizedScope {
    AuthorizedScope {
        scope: scope(a),
        principal_id: a.id,
        role: a.role.clone(),
    }
}
impl Service {
    /// Select the unified AnySeek projection while retaining the authority,
    /// single durable worker, and final publication checks.
    pub async fn open_with_anyseek(
        dir: impl AsRef<Path>,
        models: Models,
        anyseek: crate::storage::anyseek::AnySeekConfig,
    ) -> Result<Self> {
        Self::open(dir.as_ref(), models, anyseek, true).await
    }

    #[cfg(feature = "embedded-anyseek")]
    /// Open Origence with its in-process AnySeek catalog under `dir/anyseek`.
    pub async fn open_embedded(dir: impl AsRef<Path>, models: Models) -> Result<Self> {
        Self::open(
            dir.as_ref(),
            models,
            crate::storage::anyseek::AnySeekConfig::embedded(),
            true,
        )
        .await
    }

    pub(crate) async fn open(
        dir: &Path,
        models: Models,
        mut anyseek: crate::storage::anyseek::AnySeekConfig,
        recover_background_work: bool,
    ) -> Result<Self> {
        if let Some(profile) = &models.profile {
            anyseek = anyseek.with_embedding(profile.clone(), models.embedding_dimension())?;
        }
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(anyhow::Error::from)?;
        let dir = tokio::fs::canonicalize(dir)
            .await
            .map_err(anyhow::Error::from)?;
        let relational = Arc::new(SqliteStore::open(dir.join("context.db")).await?);
        let published = relational.has_committed_full_text().await?;
        let binding =
            crate::storage::retrieval_store::binding::prepare(&dir, &anyseek, published).await?;
        let blobs = LocalBlobStore::open(dir.join("blobs")).await?;
        let engine = Arc::new(StorageEngine::new(
            relational.clone(),
            relational,
            RetrievalStore::open_bound(anyseek, binding).await?,
            blobs,
        ));
        let service = Self {
            engine,
            models,
            worker_running: Arc::new(AtomicBool::new(false)),
        };
        // Empty fresh stores legitimately have no native tables. Once a live
        // publication committed, a missing required surface is a storage failure,
        // never an empty corpus or permission to recreate authoritative data.
        let initialized = async {
            service.engine.check().await?;
            service.check_committed_surfaces().await?;
            if recover_background_work {
                service.engine.relational().recover_jobs().await?;
                crate::worker::cleanup(&service).await?;
            }
            Ok(())
        }
        .await;
        if let Err(error) = initialized {
            let _ = service.engine.shutdown().await;
            return Err(error);
        }
        Ok(service)
    }
    pub async fn ready(&self) -> bool {
        self.worker_running.load(Ordering::Acquire)
            && self.engine.check().await.is_ok()
            && self.check_committed_surfaces().await.is_ok()
    }
    pub(crate) async fn check_committed_surfaces(&self) -> Result<()> {
        for scope in self
            .engine
            .relational()
            .committed_full_text_scopes()
            .await?
        {
            // No authority lease/transaction is held across remote IO.
            self.engine.vector().check_scope(scope).await?;
        }
        if self.engine.relational().has_global_knowledge().await? {
            self.engine
                .vector()
                .check_scope(global_knowledge::GLOBAL_SCOPE)
                .await?;
        }
        let profiles = self.engine.relational().committed_vector_profiles().await?;
        for (profile, dimension) in profiles {
            self.engine
                .vector()
                .check_vector_available(&profile, dimension)
                .await?;
        }
        Ok(())
    }
    pub async fn auth(&self, token: &str) -> Result<AuthContext> {
        self.auth_workspace(token, None).await
    }
    pub(crate) async fn read(&self, a: &AuthContext, permission: Permission) -> Result<SqliteTx> {
        Ok(self
            .engine
            .relational()
            .begin_read_checked(authorized(a), permission)
            .await?)
    }
    pub(crate) async fn write(&self, a: &AuthContext, permission: Permission) -> Result<SqliteTx> {
        Ok(self
            .engine
            .relational()
            .begin_checked(authorized(a), permission)
            .await?)
    }

    pub(crate) async fn read_with_access(
        &self,
        access: ReadAccess<'_>,
        permission: Permission,
    ) -> Result<SqliteTx> {
        match access {
            ReadAccess::Workspace(auth) => self.read(auth, permission).await,
            ReadAccess::Deployment { token, workspace } => {
                self.inspect_workspace(token, workspace).await
            }
        }
    }
    pub(crate) async fn command(
        &self,
        a: &AuthContext,
        permission: Permission,
        op: &str,
        key: &str,
        input: &Value,
    ) -> Result<(SqliteTx, Option<Value>)> {
        if key.is_empty() || key.len() > 200 || !key.is_ascii() {
            return Err(AppError::Invalid(
                "Idempotency-Key must contain 1..200 ASCII characters".into(),
            ));
        }
        let mut tx = self.write(a, permission).await?;
        let cached = tx.begin_command(op, key, &hash(input.to_string())).await?;
        Ok((tx, cached))
    }
    pub(crate) async fn finish(mut tx: SqliteTx, response: Value) -> Result<Value> {
        tx.finish_command(response.clone()).await?;
        tx.commit().await?;
        Ok(response)
    }
    pub(crate) async fn enqueue(
        tx: &mut SqliteTx,
        payload: JobPayload,
        asset: Option<Uuid>,
        source: Option<Uuid>,
    ) -> Result<Uuid> {
        let (kind, payload) = payload.encode()?;
        let job_id = Uuid::new_v4();
        tx.enqueue(crate::storage::WorkItem {
            job_id,
            kind: kind.as_str().into(),
            payload,
            asset,
            source,
        })
        .await?;
        Ok(job_id)
    }
    pub async fn memory(&self, a: &AuthContext, key: &str, input: MemoryInput) -> Result<Value> {
        parsing::validate_text(&input.content)?;
        if input.fact_key.trim().is_empty()
            || input.fact_key.len() > 256
            || input.fact_key.contains('\0')
        {
            return Err(AppError::Invalid("invalid fact_key".into()));
        }
        let (mut tx, cached) = self
            .command(a, Permission::Write, "memory", key, &json!(input))
            .await?;
        if let Some(v) = cached {
            return Ok(v);
        }
        let source = tx.create_event("structured", &input.content, None).await?;
        let (asset, version) = tx.slot(&input.fact_key).await?;
        let job = Self::enqueue(
            &mut tx,
            JobPayload::Publish {
                expected_version: version,
                format: "text".into(),
                embedding_profile: self.models.profile.clone(),
            },
            Some(asset),
            Some(source),
        )
        .await?;
        tx.audit(
            "memory.direct_published",
            asset,
            json!({"content_hash":hash(&input.content),"expected_version":version,"job_id":job}),
        )
        .await?;
        Self::finish(
            tx,
            json!({"asset_id":asset,"source_event_id":source,"job_id":job,"state":"accepted","conflict":version.is_some()}),
        )
        .await
    }
    pub async fn identified_memory(
        &self,
        a: &AuthContext,
        key: &str,
        input: IdentifiedMemoryInput,
    ) -> Result<Value> {
        input.validate_version()?;
        parsing::validate_text(&input.content)?;
        input
            .identity
            .validate()
            .map_err(|_| AppError::Invalid("invalid memory identity".into()))?;
        let (mut tx, cached) = self
            .command(
                a,
                Permission::Write,
                "identified_memory",
                key,
                &json!(input),
            )
            .await?;
        if let Some(value) = cached {
            return Ok(value);
        }
        tx.require_legacy_identity(&input.identity).await?;
        let (asset, version) = tx.identity_slot(&input.identity).await?;
        input.check_version(version)?;
        let source = tx.create_event("identified", &input.content, None).await?;
        let job = Self::enqueue(
            &mut tx,
            JobPayload::Publish {
                expected_version: version,
                format: "text".into(),
                embedding_profile: self.models.profile.clone(),
            },
            Some(asset),
            Some(source),
        )
        .await?;
        tx.audit(
            "memory.identified_accepted",
            asset,
            json!({"content_hash":hash(&input.content),"expected_version":version,"job_id":job}),
        )
        .await?;
        Self::finish(
            tx,
            json!({"asset_id":asset,"source_event_id":source,"job_id":job,"state":"accepted","conflict":version.is_some()}),
        )
        .await
    }

    pub async fn identified_capture(
        &self,
        a: &AuthContext,
        key: &str,
        input: IdentifiedMemoryInput,
    ) -> Result<Value> {
        input.validate_version()?;
        parsing::validate_text(&input.content)?;
        input
            .identity
            .validate()
            .map_err(|_| AppError::Invalid("invalid memory identity".into()))?;
        let (mut tx, cached) = self
            .command(
                a,
                Permission::Write,
                "identified_capture",
                key,
                &json!(input),
            )
            .await?;
        if let Some(value) = cached {
            return Ok(value);
        }
        tx.require_legacy_identity(&input.identity).await?;
        if !self.models.extraction_enabled() {
            return Err(AppError::Unavailable(
                "extraction model is not configured".into(),
            ));
        }
        let (asset, version) = tx.identity_slot(&input.identity).await?;
        input.check_version(version)?;
        let asset = if version.is_some() {
            tx.unnormalized_memory_slot().await?
        } else {
            asset
        };
        let source = tx
            .create_event("identified_capture", &input.content, None)
            .await?;
        let job = Self::enqueue(
            &mut tx,
            JobPayload::IdentifiedExtract {
                identity: input.identity,
                expected_version: None,
                embedding_profile: self.models.profile.clone(),
            },
            Some(asset),
            Some(source),
        )
        .await?;
        tx.audit(
            "memory.identified_capture",
            source,
            json!({"asset_id":asset,"job_id":job}),
        )
        .await?;
        Self::finish(
            tx,
            json!({"asset_id":asset,"source_event_id":source,"job_id":job}),
        )
        .await
    }

    pub async fn capture(&self, a: &AuthContext, key: &str, input: CaptureInput) -> Result<Value> {
        parsing::validate_text(&input.content)?;
        let (mut tx, cached) = self
            .command(a, Permission::Write, "capture", key, &json!(input))
            .await?;
        if let Some(v) = cached {
            return Ok(v);
        }
        let source = tx.create_event("capture", &input.content, None).await?;
        let job = Self::enqueue(
            &mut tx,
            JobPayload::LegacyExtract {
                embedding_profile: self.models.profile.clone(),
            },
            None,
            Some(source),
        )
        .await?;
        tx.audit("memory.capture", source, json!({"job_id":job}))
            .await?;
        Self::finish(tx, json!({"source_event_id":source,"job_id":job})).await
    }
    pub async fn knowledge(
        &self,
        a: &AuthContext,
        key: &str,
        input: KnowledgeInput,
    ) -> Result<Value> {
        if input.title.trim().is_empty()
            || input.title.len() > 512
            || input.title.contains('\0')
            || !["text", "markdown", "pdf"].contains(&input.format.as_str())
            || input.content.is_some() == input.file_id.is_some()
            || (input.format == "pdf" && input.file_id.is_none())
        {
            return Err(AppError::Invalid(
                "provide title, format and exactly one of content/file_id; PDF requires file"
                    .into(),
            ));
        }
        if let Some(text) = &input.content {
            parsing::validate_text(text)?;
        }
        let (mut tx, cached) = self
            .command(a, Permission::Write, "knowledge", key, &json!(input))
            .await?;
        if let Some(v) = cached {
            return Ok(v);
        }
        if let Some(file) = input.file_id
            && !tx.file_exists(file).await?
        {
            return Err(AppError::NotFound);
        }
        let asset = if let Some(id) = input.asset_id {
            tx.require_not_learning_asset(id).await?;
            let row = tx.asset_metadata(id).await?;
            if row["deleted"] == true
                || row["kind"] != "knowledge"
                || row["current_version"] != json!(input.expected_version)
            {
                return Err(AppError::Conflict("asset/version mismatch".into()));
            }
            id
        } else {
            if input.expected_version.is_some() {
                return Err(AppError::Invalid(
                    "new asset has no expected_version".into(),
                ));
            }
            tx.insert_knowledge_asset(&input.title).await?
        };
        let source = tx
            .create_event(
                "knowledge",
                input.content.as_deref().unwrap_or(""),
                input.file_id,
            )
            .await?;
        let job = Self::enqueue(
            &mut tx,
            JobPayload::Ingest {
                format: input.format,
                title: input.title,
                expected_version: input.expected_version,
                embedding_profile: self.models.profile.clone(),
            },
            Some(asset),
            Some(source),
        )
        .await?;
        tx.audit("knowledge.ingest", asset, json!({"job_id":job}))
            .await?;
        Self::finish(
            tx,
            json!({"asset_id":asset,"source_event_id":source,"job_id":job}),
        )
        .await
    }
    pub async fn restore(
        &self,
        a: &AuthContext,
        key: &str,
        id: Uuid,
        input: RestoreInput,
    ) -> Result<Value> {
        if input.reason.trim().is_empty() || input.reason.len() > 2000 {
            return Err(AppError::Invalid(
                "restore reason required (max 2000 bytes)".into(),
            ));
        }
        let (mut tx, cached) = self
            .command(
                a,
                Permission::Write,
                &format!("restore/{id}"),
                key,
                &json!(input),
            )
            .await?;
        if let Some(v) = cached {
            return Ok(v);
        }
        tx.require_not_learning_asset(id).await?;
        if tx.is_semantic_asset(id).await? {
            return Err(AppError::Conflict(
                "semantic memories require an explicit semantic write".into(),
            ));
        }
        let row = tx.restore_source(id, input.target_version).await?;
        if row["current_version"] != input.expected_version {
            return Err(AppError::Conflict("expected_version mismatch".into()));
        }
        let source = decode(row["source_event_id"].clone())?;
        let job = Self::enqueue(
            &mut tx,
            JobPayload::Restore {
                target_version: input.target_version,
                expected_version: input.expected_version,
                title: decode(row["title"].clone())?,
                embedding_profile: self.models.profile.clone(),
            },
            Some(id),
            Some(source),
        )
        .await?;
        tx.audit(
            "asset.restore_authorized",
            id,
            json!({"reason":input.reason,"target_version":input.target_version}),
        )
        .await?;
        Self::finish(
            tx,
            json!({"asset_id":id,"job_id":job,"restored_from":input.target_version}),
        )
        .await
    }
    pub async fn get(&self, a: &AuthContext, id: Uuid, version: Option<i32>) -> Result<Value> {
        self.read_asset(ReadAccess::Workspace(a), id, version).await
    }

    async fn read_asset(
        &self,
        access: ReadAccess<'_>,
        id: Uuid,
        version: Option<i32>,
    ) -> Result<Value> {
        let mut tx = self.read_with_access(access, Permission::Read).await?;
        let scope = tx.scope();
        let mut v = tx.asset_view(id, version).await?;
        let references = tx
            .chunk_refs_for_version(id, decode(v["version"].clone())?)
            .await?;
        tx.commit().await?;
        let chunks = self.read_chunk_content(scope, &references).await?;
        let content = chunks
            .into_iter()
            .map(|chunk| chunk.content)
            .collect::<String>();
        if v["content_hash"].as_str() != Some(hash(&content).as_str()) {
            return Err(AppError::Unavailable(
                "document content does not match its publication".into(),
            ));
        }
        let mut verify = self.read_with_access(access, Permission::Read).await?;
        if verify.asset_view(id, version).await? != v
            || verify
                .chunk_refs_for_version(id, decode(v["version"].clone())?)
                .await?
                != references
        {
            return Err(AppError::Conflict("document publication changed".into()));
        }
        verify.commit().await?;
        v["content"] = json!(content);
        Ok(v)
    }
    pub async fn lookup_memory(
        &self,
        a: &AuthContext,
        identity: crate::memory_identity::MemoryIdentity,
    ) -> Result<Value> {
        identity
            .validate()
            .map_err(|_| AppError::Invalid("invalid memory identity".into()))?;
        let mut tx = self.read(a, Permission::Read).await?;
        let value = tx.identity_view(&identity).await?;
        tx.commit().await?;
        let value = self
            .get(a, decode(value["asset_id"].clone())?, None)
            .await?;
        if value["identity"] != json!(identity) {
            return Err(AppError::Conflict("memory identity changed".into()));
        }
        Ok(value)
    }
    pub async fn source_event(&self, a: &AuthContext, id: Uuid) -> Result<Value> {
        let mut tx = self.read(a, Permission::Write).await?;
        let linked = tx.native_archive_turn_for_source(id).await?;
        if linked.is_none() {
            match tx.event_view(id).await {
                Ok(value) if value["kind"] != "session_turn" => {
                    tx.commit().await?;
                    return Ok(value);
                }
                Ok(_) | Err(StorageError::NotFound) => {}
                Err(err) => return Err(err.into()),
            }
        }
        tx.commit().await?;
        let record = match linked {
            Some((session, turn)) => self
                .load_native_archive(a, session, turn)
                .await?
                .ok_or(AppError::NotFound)?,
            None => self
                .load_native_archive_source(a, id)
                .await?
                .ok_or(AppError::NotFound)?,
        };
        Ok(json!({
            "event_id":record.source,"kind":"session_turn","content":json!(record.body).to_string(),
            "file_id":null,"created_at":record.created_at
        }))
    }
    pub async fn job(&self, a: &AuthContext, id: Uuid) -> Result<Value> {
        let mut tx = self.read(a, Permission::Write).await?;
        let v = tx.job_view(id).await?;
        tx.commit().await?;
        Ok(v)
    }
    pub async fn job_action(
        &self,
        a: &AuthContext,
        key: &str,
        id: Uuid,
        action: &str,
    ) -> Result<Value> {
        if !["retry", "cancel"].contains(&action) {
            return Err(AppError::Invalid("invalid job action".into()));
        }
        let op = format!("job/{id}/{action}");
        let (mut tx, cached) = self
            .command(a, Permission::Write, &op, key, &json!({}))
            .await?;
        if let Some(v) = cached {
            return Ok(v);
        }
        let row = tx.job_row(id).await?;
        let state = row["state"].as_str().unwrap_or("");
        // Improve's model job can be completed while its Ingest children are
        // still publishing. Explicit cancellation releases only the unfinished
        // run, preserves already-published children, and never consumes input.
        let cancelled_improve = action == "cancel"
            && row["operation"] == "improve"
            && tx.cancel_improve_job(id).await?;
        if (!cancelled_improve && ["completed", "superseded"].contains(&state))
            || (action == "retry" && !["failed", "cancelled"].contains(&state))
        {
            return Err(AppError::Conflict(
                "job state does not allow this action".into(),
            ));
        }
        if row["operation"] == "cleanup" {
            if action == "cancel" {
                return Err(AppError::Conflict("cleanup cannot be cancelled".into()));
            }
            tx.check_permission(Permission::Delete).await?;
        }
        if action == "retry" {
            // Reclamation is privileged in the Worker and must remain possible
            // after the deleting key is revoked. The retrying actor still needs
            // Delete above; publication jobs retain their original authority.
            if row["operation"] != "cleanup" {
                tx.require_job_creator_write(id).await?;
            }
            tx.require_improve_job_retry_allowed(id).await?;
            tx.valid_targets(
                decode(row["asset_id"].clone())?,
                decode(row["source_event_id"].clone())?,
            )
            .await?;
        }
        let generation = tx.set_job_action(id, action).await?;
        tx.audit(&op, id, json!({"generation":generation})).await?;
        Self::finish(tx,json!({"job_id":id,"state":if action=="cancel"{"cancelled"}else{"pending"},"generation":generation})).await
    }
    pub async fn delete(
        &self,
        a: &AuthContext,
        key: &str,
        id: Uuid,
        target: &str,
    ) -> Result<Value> {
        if !["asset", "event", "file"].contains(&target) {
            return Err(AppError::Invalid("invalid deletion target".into()));
        }
        let operation = format!("delete/{target}/{id}");
        let (mut tx, cached) = self
            .command(a, Permission::Delete, &operation, key, &json!({}))
            .await?;
        if let Some(receipt) = cached {
            return Ok(receipt);
        }
        if target == "event" && tx.native_archive_turn_for_source(id).await?.is_some() {
            return Err(AppError::Forbidden);
        }
        let changed = match target {
            "asset" => tx.delete_asset(id).await?,
            "event" => tx.retract_event(id).await?,
            _ => tx.delete_file(id).await?,
        };
        if changed == 0 {
            return Err(AppError::NotFound);
        }
        if target == "event" {
            tx.retract_consolidation_source(id).await?;
        }
        tx.cancel_affected_jobs().await?;
        let job = Self::enqueue(
            &mut tx,
            JobPayload::Cleanup {
                target: target.into(),
                id,
            },
            None,
            None,
        )
        .await?;
        tx.audit(
            "source.logical_delete",
            id,
            json!({"target":target,"job_id":job}),
        )
        .await?;
        Self::finish(
            tx,
            json!({"id":id,"blocked":true,"cleanup_job_id":job,"originals_retained":true}),
        )
        .await
    }
    pub(crate) fn blob_key(a: &AuthContext, content_hash: &str) -> BlobKey {
        BlobKey {
            scope: scope(a),
            root: "uploads".into(),
            path: content_hash.into(),
        }
    }
    pub async fn upload(
        &self,
        a: &AuthContext,
        key: &str,
        name: &str,
        format: &str,
        bytes: &[u8],
    ) -> Result<Value> {
        if name.len() > 512
            || name.contains('\0')
            || bytes.is_empty()
            || bytes.len() > parsing::MAX_FILE
            || !["text", "markdown", "pdf"].contains(&format)
        {
            return Err(AppError::Invalid(
                "invalid file name, format or size".into(),
            ));
        }
        if format != "pdf" {
            parsing::validate_text(
                std::str::from_utf8(bytes)
                    .map_err(|_| AppError::Invalid("UTF-8 required".into()))?,
            )?;
        }
        self.read(a, Permission::Write).await?.rollback().await?;
        let content_hash = hash(bytes);
        self.engine
            .blobs()
            .put(&Self::blob_key(a, &content_hash), bytes.to_vec())
            .await?;
        let (mut tx, cached) = self
            .command(
                a,
                Permission::Write,
                "upload",
                key,
                &json!({"name":name,"format":format,"hash":content_hash}),
            )
            .await?;
        if let Some(v) = cached {
            return Ok(v);
        }
        let id = tx
            .insert_file(name, format, &content_hash, bytes.len() as i64)
            .await?;
        tx.audit("file.upload", id, json!({"size":bytes.len()}))
            .await?;
        Self::finish(tx, json!({"file_id":id,"size":bytes.len(),"format":format})).await
    }
    pub async fn file(&self, a: &AuthContext, id: Uuid) -> Result<Vec<u8>> {
        let mut tx = self.read(a, Permission::Write).await?;
        let expected = tx.file_hash(id).await?.ok_or(AppError::NotFound)?;
        tx.commit().await?;
        let bytes = self
            .engine
            .blobs()
            .get(&Self::blob_key(a, &expected))
            .await?;
        if hash(&bytes) != expected {
            return Err(AppError::Unavailable(
                "stored file integrity check failed".into(),
            ));
        }
        let mut tx = self.read(a, Permission::Write).await?;
        if !tx.file_visible(id).await? {
            return Err(AppError::NotFound);
        }
        tx.commit().await?;
        Ok(bytes)
    }
}
