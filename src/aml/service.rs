//! AML protocol orchestration through the existing durable worker.
use crate::{
    error::{AppError, Result},
    service::{Service, authorized},
    storage::{AuthorizedScope, DomainTx, Permission, hash},
    types::AuthContext,
};

fn context(auth: AuthorizedScope) -> AuthContext {
    AuthContext {
        id: auth.principal_id,
        tenant_id: auth.scope.tenant_id,
        workspace_id: auth.scope.workspace_id,
        role: auth.role,
    }
}

impl Service {
    /// Explicit admin opt-in for the authenticated key's native workspace.
    /// Keys issued in this workspace share one AML namespace.
    pub async fn enable_aml_namespace(&self, auth: &AuthContext) -> Result<()> {
        self.engine
            .relational()
            .enable_aml_namespace(authorized(auth))
            .await?;
        Ok(())
    }

    /// Prepare a user's scope before Add. Preserves the complete external ID.
    /// All later operations still recheck the original key's live permission.
    pub async fn ensure_aml_user(&self, auth: &AuthContext, user_id: &str) -> Result<AuthContext> {
        Ok(context(
            self.engine
                .relational()
                .ensure_aml_user(authorized(auth), user_id)
                .await?,
        ))
    }

    /// Resolve Search scope without creating a workspace or falling back.
    pub async fn lookup_aml_user(
        &self,
        auth: &AuthContext,
        user_id: &str,
    ) -> Result<Option<AuthContext>> {
        Ok(self
            .engine
            .relational()
            .lookup_aml_user(authorized(auth), user_id)
            .await?
            .map(context))
    }
}

use super::{ADD_WAIT, AddInput, AddRecord, AddResponse, SearchInput, validate_id};
use serde_json::{Value, json};
use std::time::Duration;

impl Service {
    /// Persist a whole logical Add and one job atomically. No model work here.
    pub async fn submit_aml_add(
        &self,
        namespace: &AuthContext,
        input: AddInput,
    ) -> Result<AddRecord> {
        let profile = self
            .models
            .profile
            .clone()
            .ok_or_else(|| AppError::Unavailable("AML requires an embedding model".into()))?;
        let (input, source, request_hash) = tokio::task::spawn_blocking(move || -> Result<_> {
            input.validate()?;
            let source = serde_json::to_string(&input).map_err(anyhow::Error::from)?;
            if source.len() > crate::parsing::MAX_FILE {
                return Err(AppError::Invalid("AML request too large".into()));
            }
            let request_hash = hash(source.as_bytes());
            Ok((input, source, request_hash))
        })
        .await
        .map_err(|e| AppError::Internal(e.into()))??;
        let user = self.ensure_aml_user(namespace, &input.user_id).await?;
        let mut tx = self.write(&user, Permission::Write).await?;
        if let Some(record) = tx.aml_add_record(&input.request_id).await? {
            if record.request_hash != request_hash {
                return Err(AppError::Conflict(
                    "AML request_id reused with different content".into(),
                ));
            }
            if record.embedding_profile != profile {
                return Err(AppError::Conflict(
                    "AML embedding profile differs from accepted Add".into(),
                ));
            }
            tx.commit().await?;
            return Ok(record);
        }
        let asset = tx.insert_knowledge_asset("AML conversation batch").await?;
        let source_event = tx.create_event("aml_messages_v1", &source, None).await?;
        let job = Self::enqueue(&mut tx, "aml_ingest", json!({"expected_version":null,"embedding_profile":profile,"parser":super::MESSAGE_PARSER}), Some(asset), Some(source_event)).await?;
        let record = AddRecord {
            request_id: input.request_id,
            session_id: input.session_id,
            request_hash,
            embedding_profile: profile,
            asset_id: asset,
            source_event_id: source_event,
            job_id: job,
        };
        tx.record_aml_add(&record).await?;
        tx.audit("aml.add_accepted", asset, json!({"job_id":job}))
            .await?;
        tx.commit().await?;
        Ok(record)
    }

    /// Observe the original job only. Dropping/timing out this future never
    /// cancels, retries, or duplicates durable work owned by the single worker.
    pub async fn wait_aml_add(
        &self,
        namespace: &AuthContext,
        user_id: &str,
        request_id: &str,
        wait: Duration,
    ) -> Result<AddResponse> {
        validate_id(user_id)?;
        validate_id(request_id)?;
        if wait.is_zero() || wait > ADD_WAIT {
            return Err(AppError::Invalid(
                "AML wait must be within 25 minutes".into(),
            ));
        }
        let user = self
            .lookup_aml_user(namespace, user_id)
            .await?
            .ok_or(AppError::NotFound)?;
        tokio::time::timeout(wait, async {
            loop {
                let mut tx = self.read(&user, Permission::Write).await?;
                let record = tx
                    .aml_add_record(request_id)
                    .await?
                    .ok_or(AppError::NotFound)?;
                if self.models.profile.as_deref() != Some(record.embedding_profile.as_str()) {
                    return Err(AppError::Conflict(
                        "AML embedding profile differs from accepted Add".into(),
                    ));
                }
                let job = tx.job_view(record.job_id).await?;
                match job["state"].as_str() {
                    Some("completed") => {
                        if !tx.aml_add_visible(&record).await? {
                            return Err(AppError::Conflict(
                                "AML evidence is no longer searchable".into(),
                            ));
                        }
                        tx.commit().await?;
                        return Ok(AddResponse {
                            success: true,
                            request_id: record.request_id,
                            user_id: user_id.into(),
                            session_id: record.session_id,
                        });
                    }
                    Some("failed") => {
                        return Err(AppError::Unavailable(
                            "AML Add failed; its original job requires authorized retry".into(),
                        ));
                    }
                    Some("cancelled" | "superseded") => {
                        return Err(AppError::Conflict("AML Add did not complete".into()));
                    }
                    Some("pending" | "processing" | "retry_wait") => {}
                    _ => return Err(AppError::Internal(anyhow::anyhow!("invalid AML job state"))),
                }
                tx.commit().await?;
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .map_err(|_| {
            AppError::Unavailable("AML Add is still pending; retry with the same request_id".into())
        })?
    }

    pub async fn aml_add(&self, namespace: &AuthContext, input: AddInput) -> Result<AddResponse> {
        let user_id = input.user_id.clone();
        let record = self.submit_aml_add(namespace, input).await?;
        self.wait_aml_add(namespace, &user_id, &record.request_id, ADD_WAIT)
            .await
    }

    /// Only retrieves stored evidence; options never become memory or answers.
    pub async fn aml_search(&self, namespace: &AuthContext, input: SearchInput) -> Result<Value> {
        input.validate()?;
        let Some(user) = self.lookup_aml_user(namespace, &input.user_id).await? else {
            return Ok(json!({"data":[]}));
        };
        let found = self
            .search(
                &user,
                crate::types::SearchInput {
                    query: input.query,
                    limit: input.top_k,
                    mode: "vector".into(),
                    allow_partial: false,
                    memory_identity: None,
                    components: crate::types::RetrievalComponents {
                        summaries: false,
                        graph: false,
                    },
                },
            )
            .await?;
        let hits: Vec<crate::types::SearchHit> = crate::service::decode(found["hits"].clone())?;
        let data: Vec<Value> = hits.into_iter().map(|hit| {
            let content = if hit.locator["parser"] == super::MESSAGE_PARSER {
                let metadata = json!({"role":hit.locator["role"],"timestamp":hit.locator["timestamp"],"session_id":hit.locator["session_id"],"message_index":hit.locator["message_index"],"message_count":hit.locator["message_count"],"parser":hit.locator["parser"],"source_path":hit.locator["source_path"],"byte_basis":hit.locator["byte_basis"],"byte_start":hit.locator["byte_start"],"byte_end":hit.locator["byte_end"]});
                format!("Source metadata: {metadata}\n{}", hit.content)
            } else { hit.content };
            json!({"id":hit.chunk_id,"content":content})
        }).collect();
        Ok(json!({"data":data}))
    }
}
