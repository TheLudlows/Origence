//! S2 service operations share the existing command ledger and durable worker.
use crate::{
    error::Result, jobs::JobPayload, memory_semantics::MemoryProperty, service::Service,
    storage::Permission, types::AuthContext,
};
use serde_json::{Value, json};
impl Service {
    pub async fn register_memory_property(
        &self,
        a: &AuthContext,
        key: &str,
        mut input: MemoryProperty,
    ) -> Result<Value> {
        input.validate()?;
        input.required_context.sort();
        let (mut tx, cached) = self
            .command(a, Permission::Delete, "memory_property", key, &json!(input))
            .await?;
        if let Some(value) = cached {
            return Ok(value);
        }
        tx.register_memory_property(&input).await?;
        Self::finish(tx, json!({"property":input})).await
    }
    pub async fn memory_property(&self, a: &AuthContext, predicate: &str) -> Result<Value> {
        let mut tx = self.read(a, Permission::Read).await?;
        let property = tx
            .memory_property(predicate)
            .await?
            .ok_or(crate::error::AppError::NotFound)?;
        tx.commit().await?;
        Ok(json!({"property":property}))
    }
}

use crate::{
    error::AppError,
    memory_identity::MemoryIdentity,
    memory_semantics::{self as domain, Cardinality, Decision, Member, SemanticMemoryInput},
    semantic_capture::{ExtractionProvenance, SemanticCaptureInput},
    storage::{DomainTx, sqlite::SqliteTx},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One deterministic S2 routing decision, also saved by automatic capture before embedding.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct SemanticPlan {
    pub asset: Uuid,
    pub expected_version: Option<i32>,
    pub status: String,
    pub members: Vec<Member>,
    pub identity: Option<MemoryIdentity>,
    pub duplicate: bool,
}
impl SemanticPlan {
    pub fn result(&self) -> Value {
        json!({"asset_id":self.asset,"version":if self.duplicate {self.expected_version.unwrap_or(0)} else {self.expected_version.unwrap_or(0)+1},
            "state":if self.duplicate {"duplicate"} else {"published"},"semantic_status":self.status,
            "identity":if self.status=="confirmed" {self.identity.as_ref()} else {None},
            "normalization_status":if self.status=="confirmed" {"explicit_identity"} else {"unnormalized"}})
    }
    pub fn chunks(&self) -> Vec<crate::types::Chunk> {
        domain::chunks(
            &self.members,
            &self.status,
            if self.status == "confirmed" {
                None
            } else {
                self.identity.as_ref()
            },
        )
    }
}
pub(crate) async fn plan_semantic(
    tx: &mut SqliteTx,
    input: &SemanticMemoryInput,
    source: Uuid,
    extraction: Option<ExtractionProvenance>,
) -> Result<SemanticPlan> {
    input.validate()?;
    tx.require_memory_semantics().await?;
    let mut slot = None;
    let mut current = Vec::new();
    let mut snapshot_visible = true;
    let mut cardinality = Cardinality::Single;
    if let Some(identity) = &input.identity {
        let property = tx
            .memory_property(&identity.predicate)
            .await?
            .ok_or_else(|| AppError::Invalid("unregistered memory property".into()))?;
        property.check_identity(identity)?;
        cardinality = property.cardinality;
        if input.uncertain_status().is_none() {
            let (asset, version) = tx.identity_slot(identity).await?;
            if input.expected_version != version.unwrap_or(0) {
                return Err(AppError::Conflict("expected_version mismatch".into()));
            }
            current = tx.semantic_members(asset, version).await?;
            if let Some(version) = version {
                snapshot_visible = tx.semantic_version_visible(asset, version).await?;
            }
            slot = Some((asset, version));
        }
    }
    let mut decision = domain::decide(input, cardinality, &current)?;
    if decision == Decision::Duplicate && !snapshot_visible {
        decision = Decision::Publish;
    }
    let (asset, version, status) = match decision {
        Decision::Unnormalized(status) => (tx.unnormalized_memory_slot().await?, None, status),
        _ => {
            let (asset, version) =
                slot.ok_or_else(|| AppError::Invalid("missing semantic identity".into()))?;
            (asset, version, "confirmed")
        }
    };
    let duplicate = decision == Decision::Duplicate;
    let mut members = if duplicate || (status == "confirmed" && cardinality == Cardinality::Multi) {
        current
    } else {
        Vec::new()
    };
    if !duplicate {
        for statement in &input.statements {
            if !members.iter().any(|m| m.statement.value == statement.value) {
                members.push(Member {
                    statement: statement.clone(),
                    source_event_id: source,
                    source_version: version.unwrap_or(0) + 1,
                    extraction: extraction.clone(),
                });
            }
        }
    }
    if members.len() > domain::MAX_MEMBERS {
        return Err(AppError::Invalid("too many memory values".into()));
    }
    Ok(SemanticPlan {
        asset,
        expected_version: version,
        status: status.into(),
        members,
        identity: input.identity.clone(),
        duplicate,
    })
}
impl Service {
    pub async fn semantic_memory(
        &self,
        a: &AuthContext,
        key: &str,
        input: SemanticMemoryInput,
    ) -> Result<Value> {
        input.validate()?;
        let (mut tx, cached) = self
            .command(a, Permission::Write, "semantic_memory", key, &json!(input))
            .await?;
        if let Some(value) = cached {
            return Ok(value);
        }
        let source = tx
            .create_event("semantic_memory", &input.content, None)
            .await?;
        let plan = plan_semantic(&mut tx, &input, source, None).await?;
        if plan.duplicate {
            tx.audit(
                "memory.semantic_duplicate",
                plan.asset,
                json!({"source_event_id":source,"version":plan.expected_version}),
            )
            .await?;
            return Self::finish(tx,json!({"state":"duplicate","asset_id":plan.asset,"version":plan.expected_version,"source_event_id":source})).await;
        }
        let job = Self::enqueue(
            &mut tx,
            JobPayload::SemanticPublish {
                expected_version: plan.expected_version,
                embedding_profile: self.models.profile.clone(),
                semantic_members: plan.members,
                semantic_status: plan.status.clone(),
                suggested_identity: input.identity,
                parser: domain::PARSER.into(),
            },
            Some(plan.asset),
            Some(source),
        )
        .await?;
        tx.audit(
            "memory.semantic_accepted",
            plan.asset,
            json!({"job_id":job,"status":plan.status,"operation":input.operation}),
        )
        .await?;
        Self::finish(tx,json!({"state":"accepted","asset_id":plan.asset,"source_event_id":source,"job_id":job,"semantic_status":plan.status,
            "normalization_status":if plan.status=="confirmed" {"explicit_identity"} else {"unnormalized"}})).await
    }
    pub async fn semantic_capture(
        &self,
        a: &AuthContext,
        key: &str,
        input: SemanticCaptureInput,
    ) -> Result<Value> {
        input.validate()?;
        let (mut tx, cached) = self
            .command(a, Permission::Write, "semantic_capture", key, &json!(input))
            .await?;
        if let Some(value) = cached {
            return Ok(value);
        }
        tx.require_memory_semantics().await?;
        let profile = self
            .models
            .semantic_extraction_profile()
            .ok_or_else(|| AppError::Unavailable("extraction model is not configured".into()))?;
        let mut properties = Vec::new();
        for target in &input.targets {
            let property = tx
                .memory_property(&target.identity.predicate)
                .await?
                .ok_or_else(|| AppError::Invalid("unregistered memory property".into()))?;
            property.check_identity(&target.identity)?;
            if target.allow_update && property.cardinality != Cardinality::Single {
                return Err(AppError::Invalid(
                    "allow_update requires a single property".into(),
                ));
            }
            let version = match tx.identity_candidate_asset(&target.identity).await? {
                Some(asset) => tx.asset_current_version(asset).await?,
                None => None,
            };
            if target.expected_version != version.unwrap_or(0) {
                return Err(AppError::Conflict("expected_version mismatch".into()));
            }
            properties.push(property);
        }
        let source = tx
            .create_event("semantic_capture", &input.content, None)
            .await?;
        let job = Self::enqueue(
            &mut tx,
            JobPayload::SemanticExtract {
                targets: input.targets,
                properties,
                extractor: crate::semantic_capture::EXTRACTOR.into(),
                extraction_profile: profile,
                extraction_model: self
                    .models
                    .summary_model()
                    .ok_or_else(|| {
                        AppError::Unavailable("extraction model is not configured".into())
                    })?
                    .to_owned(),
                embedding_profile: self.models.profile.clone(),
            },
            None,
            Some(source),
        )
        .await?;
        tx.audit("memory.semantic_capture", source, json!({"job_id":job}))
            .await?;
        Self::finish(
            tx,
            json!({"state":"accepted","source_event_id":source,"job_id":job}),
        )
        .await
    }
}

