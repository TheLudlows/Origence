//! Optional S2 property catalog. Old databases are never altered on startup.
use super::*;
use crate::memory_semantics::MemoryProperty;
pub(super) const SCHEMA: &str = include_str!("../memory-semantics-schema.sql");
pub(super) async fn available(conn: &mut Connection) -> StorageResult<bool> {
    native_turso::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='oc_memory_properties')",
    )
    .fetch_one(conn)
    .await
    .map_err(turso_err)
}
impl TursoTx {
    /// Caller holds the active job guard in the same write transaction.
    pub async fn save_semantic_capture_plan(
        &mut self,
        job: Uuid,
        plan: &Value,
    ) -> StorageResult<()> {
        native_turso::query("UPDATE oc_jobs SET payload=json_set(payload,'$.semantic_capture_plan',json(?)),updated_at=? WHERE tenant_id=? AND workspace_id=? AND id=?")
            .bind(plan.to_string()).bind(now_ms()).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(job).execute(&mut *self.tx).await.map_err(turso_err)?;
        Ok(())
    }
    pub async fn memory_property(
        &mut self,
        predicate: &str,
    ) -> StorageResult<Option<MemoryProperty>> {
        if !available(&mut self.tx).await? {
            return Ok(None);
        }
        let definition: Option<String> = native_turso::query_scalar("SELECT definition FROM oc_memory_properties WHERE tenant_id=? AND workspace_id=? AND predicate=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(predicate)
            .fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        definition
            .map(|s| {
                serde_json::from_str(&s)
                    .map_err(|_| StorageError::Unavailable("invalid memory property".into()))
            })
            .transpose()
    }
    pub async fn register_memory_property(
        &mut self,
        property: &MemoryProperty,
    ) -> StorageResult<()> {
        self.check_permission(Permission::Delete).await?;
        property
            .validate()
            .map_err(|_| StorageError::InvalidArgument("invalid memory property"))?;
        if !available(&mut self.tx).await? {
            return Err(StorageError::Unavailable(
                "memory semantics requires a new database".into(),
            ));
        }
        if let Some(existing) = self.memory_property(&property.predicate).await? {
            return if existing == *property {
                Ok(())
            } else {
                Err(StorageError::Conflict(
                    "memory property is immutable".into(),
                ))
            };
        }
        // Registration cannot silently reinterpret any previously allocated identity.
        let occupied: bool = native_turso::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_memory_identities WHERE tenant_id=? AND workspace_id=? AND json_extract(identity_json,'$.predicate')=?)")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(&property.predicate)
            .fetch_one(&mut *self.tx).await.map_err(turso_err)?;
        if occupied {
            return Err(StorageError::Conflict(
                "property already has legacy identities".into(),
            ));
        }
        native_turso::query("INSERT INTO oc_memory_properties(tenant_id,workspace_id,predicate,definition,created_by,created_at) VALUES(?,?,?,?,?,?)")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(&property.predicate).bind(serde_json::to_string(property).map_err(|_| StorageError::Unavailable("property encoding failed".into()))?).bind(self.principal_id).bind(now_ms())
            .execute(&mut *self.tx).await.map_err(turso_err)?;
        self.audit(
            "memory.property_registered",
            self.scope.workspace_id,
            json!({"predicate":property.predicate,"cardinality":property.cardinality}),
        )
        .await
    }
    pub async fn require_legacy_identity(
        &mut self,
        identity: &crate::memory_identity::MemoryIdentity,
    ) -> StorageResult<()> {
        if self.memory_property(&identity.predicate).await?.is_some() {
            return Err(StorageError::Conflict(
                "registered property requires semantic memory endpoint".into(),
            ));
        }
        Ok(())
    }
}

impl TursoTx {
    pub async fn require_memory_semantics(&mut self) -> StorageResult<()> {
        if !available(&mut self.tx).await? {
            return Err(StorageError::Unavailable(
                "memory semantics requires a new database".into(),
            ));
        }
        Ok(())
    }
    pub async fn unnormalized_memory_slot(&mut self) -> StorageResult<Uuid> {
        let id = Uuid::new_v4();
        native_turso::query("INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,created_at) VALUES(?,?,?,'memory','Unnormalized evidence',?)")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(id).bind(now_ms()).execute(&mut *self.tx).await.map_err(turso_err)?;
        Ok(id)
    }
    pub async fn is_semantic_asset(&mut self, asset: Uuid) -> StorageResult<bool> {
        native_turso::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_chunks WHERE tenant_id=? AND workspace_id=? AND asset_id=? AND json_extract(locator,'$.parser')='memory-semantics:v1')")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(asset).fetch_one(&mut *self.tx).await.map_err(turso_err)
    }
    /// Read only still-supported members; never relabel an old quote with a new source.
    pub async fn semantic_members(
        &mut self,
        asset: Uuid,
        version: Option<i32>,
    ) -> StorageResult<Vec<crate::memory_semantics::Member>> {
        let Some(version) = version else {
            return Ok(Vec::new());
        };
        let source: Uuid=native_turso::query_scalar("SELECT source_event_id FROM oc_versions WHERE tenant_id=? AND workspace_id=? AND asset_id=? AND version=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(asset).bind(version).fetch_one(&mut *self.tx).await.map_err(turso_err)?;
        if self.event_content(source, true).await?.is_none() {
            return Ok(Vec::new());
        }
        match self.require_learning_source_visible(source).await {
            Ok(()) => {}
            Err(StorageError::NotFound) => return Ok(Vec::new()),
            Err(error) => return Err(error),
        }
        let mut members = Vec::new();
        for (quote, locator) in self.chunks_for_version(asset, version).await? {
            if locator["parser"] != crate::memory_semantics::PARSER
                || locator["semantic_status"] != "confirmed"
            {
                return Err(StorageError::Conflict(
                    "existing version is not a semantic fact".into(),
                ));
            }
            let member: crate::memory_semantics::Member=serde_json::from_value(json!({"source_event_id":locator["source_event_id"],"source_version":locator["source_version"],"extraction":locator["extraction"],"statement":{"value":locator["semantic_value"],"quote":quote,"byte_start":locator["byte_start"],"byte_end":locator["byte_end"]}}))
                .map_err(|_|StorageError::Unavailable("invalid semantic provenance".into()))?;
            if self
                .event_content(member.source_event_id, true)
                .await?
                .is_some()
            {
                match self
                    .require_learning_source_visible(member.source_event_id)
                    .await
                {
                    Ok(()) => members.push(member),
                    Err(StorageError::NotFound) => {}
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(members)
    }
    pub async fn semantic_version_visible(
        &mut self,
        asset: Uuid,
        version: i32,
    ) -> StorageResult<bool> {
        if !self.semantic_version_visible_local(asset, version).await?
            || !self.learning_asset_visible(asset).await?
        {
            return Ok(false);
        }
        let sources: Vec<String> = native_turso::query_scalar("SELECT DISTINCT ss.value FROM oc_chunks c, json_each(c.locator,'$.support_sources') ss WHERE c.tenant_id=? AND c.workspace_id=? AND c.asset_id=? AND c.version=? AND json_extract(c.locator,'$.parser')='memory-semantics:v1'")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(asset).bind(version)
            .fetch_all(&mut *self.tx).await.map_err(turso_err)?;
        let sources = sources
            .iter()
            .map(|source| Uuid::parse_str(source).map_err(|_| StorageError::NotFound))
            .collect::<StorageResult<Vec<_>>>()?;
        match self.require_learning_sources_visible(&sources).await {
            Ok(()) => Ok(true),
            Err(StorageError::NotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }
}

impl TursoTx {
    pub async fn semantic_version_status(
        &mut self,
        asset: Uuid,
        version: i32,
    ) -> StorageResult<Option<String>> {
        native_turso::query_scalar("SELECT json_extract(locator,'$.semantic_status') FROM oc_chunks WHERE tenant_id=? AND workspace_id=? AND asset_id=? AND version=? AND json_extract(locator,'$.parser')='memory-semantics:v1' ORDER BY ordinal LIMIT 1")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(asset).bind(version).fetch_optional(&mut *self.tx).await.map_err(turso_err)
    }
}
