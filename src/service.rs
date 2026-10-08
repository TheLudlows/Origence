use crate::{
    error::{AppError, Result},
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

#[derive(Clone)]
pub struct Service {
    pub engine: Arc<LocalEngine>,
    pub models: Models,
    pub worker_running: Arc<AtomicBool>,
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
    pub async fn open(dir: impl AsRef<Path>, models: Models) -> Result<Self> {
        tokio::fs::create_dir_all(dir.as_ref())
            .await
            .map_err(anyhow::Error::from)?;
        let dir = tokio::fs::canonicalize(dir.as_ref())
            .await
            .map_err(anyhow::Error::from)?;
        let relational = Arc::new(SqliteStore::open(dir.join("context.db")).await?);
        let engine = Arc::new(StorageEngine::new(
            relational.clone(),
            relational,
            LanceDbStore::open(dir.join("vectors")).await?,
            KuzuStore::open(dir.join("graph")).await?,
            LocalBlobStore::open(dir.join("blobs")).await?,
        ));
        engine.check().await?;
        engine.relational().recover_jobs().await?;
        let service = Self {
            engine,
            models,
            worker_running: Arc::new(AtomicBool::new(false)),
        };
        crate::worker::cleanup(&service).await?;
        Ok(service)
    }
    pub async fn ready(&self) -> bool {
        self.worker_running.load(Ordering::Acquire) && self.engine.check().await.is_ok()
    }
    pub async fn auth(&self, token: &str) -> Result<AuthContext> {
        let a = self
            .engine
            .relational()
            .authenticate(token)
            .await
            .map_err(|e| match e {
                StorageError::Forbidden => AppError::Unauthorized,
                e => e.into(),
            })?;
        Ok(AuthContext {
            id: a.principal_id,
            tenant_id: a.scope.tenant_id,
            workspace_id: a.scope.workspace_id,
            role: a.role,
        })
    }
    pub(crate) async fn read(&self, a: &AuthContext, permission: Permission) -> Result<SqliteTx> {
        let mut tx = self.engine.relational().begin_read(authorized(a)).await?;
        tx.check_permission(permission).await?;
        Ok(tx)
    }
    pub(crate) async fn write(&self, a: &AuthContext, permission: Permission) -> Result<SqliteTx> {
        let mut tx = self.engine.relational().begin(authorized(a)).await?;
        tx.check_permission(permission).await?;
        Ok(tx)
    }
    async fn command(
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
    async fn finish(mut tx: SqliteTx, response: Value) -> Result<Value> {
        tx.finish_command(response.clone()).await?;
        tx.commit().await?;
        Ok(response)
    }
    pub(crate) async fn enqueue(
        tx: &mut SqliteTx,
        kind: &str,
        mut payload: Value,
        asset: Option<Uuid>,
        source: Option<Uuid>,
    ) -> Result<Uuid> {
        payload["schema_version"] = json!(1);
        let job_id = Uuid::new_v4();
        tx.enqueue(crate::storage::WorkItem {
            job_id,
            kind: kind.into(),
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
            "publish",
            json!({"expected_version":version,"format":"text","embedding_profile":self.models.profile}),
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
        let (asset, version) = tx.identity_slot(&input.identity).await?;
        let source = tx.create_event("identified", &input.content, None).await?;
        let job = Self::enqueue(
            &mut tx,
            "publish",
            json!({"expected_version":version,"format":"text","embedding_profile":self.models.profile}),
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
        parsing::validate_text(&input.content)?;
        input.identity.validate().map_err(|_| AppError::Invalid("invalid memory identity".into()))?;
        if !self.models.extraction_enabled() {
            return Err(AppError::Unavailable("extraction model is not configured".into()));
        }
        let (mut tx, cached) = self.command(a, Permission::Write, "identified_capture", key, &json!(input)).await?;
        if let Some(value) = cached {
            return Ok(value);
        }
        let (asset, version) = tx.identity_slot(&input.identity).await?;
        let source = tx.create_event("identified_capture", &input.content, None).await?;
        let job = Self::enqueue(
            &mut tx,
            "extract",
            json!({"identity":input.identity,"expected_version":version,"embedding_profile":self.models.profile}),
            Some(asset),
            Some(source),
        ).await?;
        tx.audit("memory.identified_capture", source, json!({"asset_id":asset,"job_id":job})).await?;
        Self::finish(tx, json!({"asset_id":asset,"source_event_id":source,"job_id":job})).await
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
            "extract",
            json!({"embedding_profile":self.models.profile}),
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
        let job=Self::enqueue(&mut tx,"ingest",json!({"format":input.format,"title":input.title,"expected_version":input.expected_version,"embedding_profile":self.models.profile}),Some(asset),Some(source)).await?;
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
                Permission::Review,
                &format!("restore/{id}"),
                key,
                &json!(input),
            )
            .await?;
        if let Some(v) = cached {
            return Ok(v);
        }
        let row = tx.restore_source(id, input.target_version).await?;
        if row["current_version"] != input.expected_version {
            return Err(AppError::Conflict("expected_version mismatch".into()));
        }
        let source = decode(row["source_event_id"].clone())?;
        let job=Self::enqueue(&mut tx,"restore",json!({"target_version":input.target_version,"expected_version":input.expected_version,"title":row["title"],"embedding_profile":self.models.profile}),Some(id),Some(source)).await?;
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
        let mut tx = self.read(a, Permission::Read).await?;
        let v = tx.asset_view(id, version).await?;
        tx.commit().await?;
        Ok(v)
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
        if row["created_by"] != json!(a.id) {
            tx.check_permission(Permission::Review).await?;
        }
        let state = row["state"].as_str().unwrap_or("");
        if ["completed", "superseded"].contains(&state)
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
        let (mut tx, cached) = self
            .command(
                a,
                Permission::Delete,
                &format!("delete/{target}/{id}"),
                key,
                &json!({}),
            )
            .await?;
        if let Some(v) = cached {
            return Ok(v);
        }
        let changed = match target {
            "asset" => tx.delete_asset(id).await?,
            "event" => tx.retract_event(id).await?,
            _ => tx.delete_file(id).await?,
        };
        if changed == 0 {
            return Err(AppError::NotFound);
        }
        tx.cancel_affected_jobs().await?;
        let job = Self::enqueue(
            &mut tx,
            "cleanup",
            json!({"target":target,"id":id}),
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
    pub async fn issue_key(&self, a: &AuthContext, role: &str) -> Result<Value> {
        let mut tx = self.write(a, Permission::Delete).await?;
        let key = tx.issue_scoped_key(role).await?;
        tx.commit().await?;
        Ok(json!(key))
    }
    pub async fn revoke_key(&self, a: &AuthContext, id: Uuid) -> Result<Value> {
        let mut tx = self.write(a, Permission::Delete).await?;
        tx.revoke_scoped_key(id).await?;
        tx.commit().await?;
        Ok(json!({"key_id":id,"revoked":true}))
    }
}
