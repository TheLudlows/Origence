//! Legacy fact slots and exact structured identity bindings.
use super::*;

impl TursoTx {
    pub(super) async fn identity_for_asset(
        &mut self,
        asset: Uuid,
    ) -> StorageResult<Option<crate::memory_identity::MemoryIdentity>> {
        if !identity_table_present(&mut self.tx).await? {
            return Ok(None);
        }
        let value: Option<String> = native_turso::query_scalar(
            "SELECT identity_json FROM oc_memory_identities WHERE tenant_id = ? AND workspace_id = ? AND asset_id = ?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(asset)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        value
            .map(|value| {
                serde_json::from_str(&value)
                    .map_err(|_| StorageError::Unavailable("invalid stored memory identity".into()))
            })
            .transpose()
    }
}

impl TursoTx {
    pub(super) async fn slot_inner(
        &mut self,
        fact_key: &str,
    ) -> StorageResult<(Uuid, Option<i32>)> {
        let existing: Option<(Uuid, Option<i32>, bool)> = native_turso::query_as(
            "SELECT id, current_version, deleted FROM oc_assets \
             WHERE tenant_id = ? AND workspace_id = ? AND kind = 'memory' AND fact_key = ?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(fact_key)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        if let Some((id, version, deleted)) = existing {
            if self.identity_for_asset(id).await?.is_some() {
                return Err(StorageError::Conflict(
                    "structured memory identity requires the identified memory endpoint".into(),
                ));
            }
            if deleted {
                return Err(StorageError::Conflict(
                    "fact slot was deleted; use a new fact key".to_string(),
                ));
            }
            return Ok((id, version));
        }
        let id = Uuid::new_v4();
        native_turso::query(
            "INSERT INTO oc_assets(tenant_id, workspace_id, id, kind, title, fact_key, created_at) \
             VALUES (?, ?, ?, 'memory', ?, ?, ?)",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(id)
        .bind(fact_key)
        .bind(fact_key)
        .bind(now_ms())
        .execute(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        Ok((id, None))
    }

    pub(super) async fn identity_view_inner(
        &mut self,
        identity: &crate::memory_identity::MemoryIdentity,
    ) -> StorageResult<Value> {
        if !identity_table_present(&mut self.tx).await? {
            return Err(StorageError::Unavailable(
                "memory identity storage unavailable".into(),
            ));
        }
        let key = identity
            .key(self.scope)
            .map_err(|_| StorageError::Conflict("invalid memory identity".into()))?;
        let asset: Option<Uuid> = native_turso::query_scalar(
            "SELECT asset_id FROM oc_memory_identities \
             WHERE tenant_id=? AND workspace_id=? AND identity_key=?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(key)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        let asset = asset.ok_or(StorageError::NotFound)?;
        if self.identity_for_asset(asset).await?.as_ref() != Some(identity) {
            return Err(StorageError::Unavailable(
                "memory identity binding mismatch".into(),
            ));
        }
        self.asset_view(asset, None).await
    }

    pub(super) async fn identity_slot_inner(
        &mut self,
        identity: &crate::memory_identity::MemoryIdentity,
    ) -> StorageResult<(Uuid, Option<i32>)> {
        if !identity_table_present(&mut self.tx).await? {
            return Err(StorageError::Unavailable(
                "memory identity storage unavailable".into(),
            ));
        }
        let key = identity
            .key(self.scope)
            .map_err(|_| StorageError::Conflict("invalid memory identity".into()))?;
        let bound_asset: Option<read_model::AssociationKey> = native_turso::query_scalar(
            "SELECT asset_id FROM oc_memory_identities WHERE tenant_id=? AND workspace_id=? AND identity_key=?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(&key)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        let existing: Option<(Uuid, Option<i32>, bool)> = if let Some(asset) = bound_asset {
            native_turso::query_as(
                "SELECT id,current_version,deleted FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND id=? AND kind='memory'",
            )
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .bind(asset)
            .fetch_optional(&mut *self.tx)
            .await
            .map_err(turso_err)?
        } else {
            None
        };
        if let Some((asset, version, deleted)) = existing {
            if self.identity_for_asset(asset).await?.as_ref() != Some(identity) {
                return Err(StorageError::Conflict(
                    "memory identity binding mismatch".into(),
                ));
            }
            if deleted {
                return Err(StorageError::Conflict(
                    "memory identity asset was deleted".into(),
                ));
            }
            return Ok((asset, version));
        }
        let collision: bool = native_turso::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND kind='memory' AND fact_key=?)",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(&key)
        .fetch_one(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        if collision {
            return Err(StorageError::Conflict(
                "memory identity conflicts with a legacy fact slot".into(),
            ));
        }
        let asset = Uuid::new_v4();
        native_turso::query(
            "INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,fact_key,created_at) VALUES (?,?,?,'memory',?,?,?)",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(asset)
        .bind(&identity.predicate)
        .bind(&key)
        .bind(now_ms())
        .execute(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        let encoded = serde_json::to_string(identity)
            .map_err(|_| StorageError::Unavailable("memory identity encoding failed".into()))?;
        native_turso::query(
            "INSERT INTO oc_memory_identities(tenant_id,workspace_id,identity_key,asset_id,identity_json,created_at) VALUES (?,?,?,?,?,?)",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(&key)
        .bind(asset)
        .bind(encoded)
        .bind(now_ms())
        .execute(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        Ok((asset, None))
    }
}
