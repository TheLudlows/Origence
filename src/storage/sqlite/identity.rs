//! Legacy fact slots and asset-level structured identity bindings.
use super::*;
use crate::memory_identity::MemoryIdentity;

/// Decode only at the caller's existing visibility/route boundary. The schema
/// enforces the pair and memory kind; recheck them for corrupt/custom fixtures.
pub(super) fn identity_body(
    row: &native_turso::Row,
) -> StorageResult<Option<(String, MemoryIdentity)>> {
    let key: Option<String> = row.try_get("identity_key").map_err(turso_err)?;
    let body: Option<String> = row.try_get("identity_json").map_err(turso_err)?;
    match (key, body) {
        (None, None) => Ok(None),
        (Some(key), Some(body)) => {
            if row.try_get::<String>("kind").map_err(turso_err)? != "memory" {
                return Err(StorageError::Unavailable(
                    "memory identity binding mismatch".into(),
                ));
            }
            let identity = serde_json::from_str(&body)
                .map_err(|_| StorageError::Unavailable("invalid stored memory identity".into()))?;
            Ok(Some((key, identity)))
        }
        _ => Err(StorageError::Unavailable(
            "memory identity binding mismatch".into(),
        )),
    }
}

pub(super) fn checked_identity(
    scope: Scope,
    row: &native_turso::Row,
) -> StorageResult<Option<MemoryIdentity>> {
    identity_body(row)?
        .map(|(key, identity)| {
            let expected = identity
                .key(scope)
                .map_err(|_| StorageError::Unavailable("invalid stored memory identity".into()))?;
            if key != expected {
                return Err(StorageError::Unavailable(
                    "memory identity binding mismatch".into(),
                ));
            }
            Ok(identity)
        })
        .transpose()
}

impl TursoTx {
    pub(super) async fn identity_for_asset(
        &mut self,
        asset: Uuid,
    ) -> StorageResult<Option<MemoryIdentity>> {
        let row = native_turso::query(
            "SELECT kind,identity_key,identity_json FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND id=?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(asset)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        row.map(|row| checked_identity(self.scope, &row))
            .transpose()
            .map(Option::flatten)
    }

    pub(super) async fn slot_inner(
        &mut self,
        fact_key: &str,
    ) -> StorageResult<(Uuid, Option<i32>)> {
        let existing = native_turso::query(
            "SELECT id,current_version,deleted,kind,identity_key,identity_json FROM oc_assets \
             WHERE tenant_id=? AND workspace_id=? AND kind='memory' AND fact_key=?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(fact_key)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        if let Some(row) = existing {
            let id = row.try_get("id").map_err(turso_err)?;
            let version = row.try_get("current_version").map_err(turso_err)?;
            let deleted: bool = row.try_get("deleted").map_err(turso_err)?;
            // This endpoint never renders or reuses structured identities. Keep
            // its established conflict before tombstone/hash checks.
            if identity_body(&row)?.is_some() {
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
        identity: &MemoryIdentity,
    ) -> StorageResult<Value> {
        let key = identity
            .key(self.scope)
            .map_err(|_| StorageError::Conflict("invalid memory identity".into()))?;
        let row = native_turso::query(
            "SELECT id,kind,identity_key,identity_json FROM oc_assets \
             WHERE tenant_id=? AND workspace_id=? AND identity_key=?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(key)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?
        .ok_or(StorageError::NotFound)?;
        let asset = row.try_get("id").map_err(turso_err)?;
        if checked_identity(self.scope, &row)?.as_ref() != Some(identity) {
            return Err(StorageError::Unavailable(
                "memory identity binding mismatch".into(),
            ));
        }
        self.asset_view(asset, None).await
    }

    pub(super) async fn identity_slot_inner(
        &mut self,
        identity: &MemoryIdentity,
    ) -> StorageResult<(Uuid, Option<i32>)> {
        let key = identity
            .key(self.scope)
            .map_err(|_| StorageError::Conflict("invalid memory identity".into()))?;
        // Exact schema admission guarantees this scoped identity index. Turso
        // 0.8.2 otherwise chooses the fact-slot scope prefix for this query.
        // State the partial-index predicate explicitly; canonical keys are non-null.
        let existing = native_turso::query(
            "SELECT id,current_version,deleted,kind,identity_key,identity_json FROM oc_assets \
             INDEXED BY oc_memory_identity_slot WHERE tenant_id=? AND workspace_id=? \
             AND identity_key=? AND kind='memory' AND identity_key IS NOT NULL",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(&key)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        if let Some(row) = existing {
            let asset = row.try_get("id").map_err(turso_err)?;
            let version = row.try_get("current_version").map_err(turso_err)?;
            let deleted: bool = row.try_get("deleted").map_err(turso_err)?;
            // The requested identity is validated and selected by its canonical
            // key. Comparing the body establishes key/body agreement while
            // preserving Conflict before the tombstone check.
            if identity_body(&row)?.as_ref().map(|(_, body)| body) != Some(identity) {
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
        let encoded = serde_json::to_string(identity)
            .map_err(|_| StorageError::Unavailable("memory identity encoding failed".into()))?;
        native_turso::query(
            "INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,fact_key,created_at,identity_key,identity_json) VALUES (?,?,?,'memory',?,?,?,?,?)",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(asset)
        .bind(&identity.predicate)
        .bind(&key)
        .bind(now_ms())
        .bind(&key)
        .bind(encoded)
        .execute(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        Ok((asset, None))
    }
}

#[cfg(feature = "local-storage")]
impl TursoTx {
    /// Raw asset identity columns, including unidentified assets. No current or
    /// semantic-status guard: consumers decode after their route filtering.
    pub(super) async fn identity_rows_for_assets(
        &mut self,
        assets: &[Uuid],
    ) -> StorageResult<HashMap<Uuid, native_turso::Row>> {
        if assets.is_empty() {
            return Ok(HashMap::new());
        }
        super::read_model::rows_by_ids(
            &mut self.tx,
            self.scope,
            "oc_assets",
            "id,kind,identity_key,identity_json",
            "id",
            assets,
        )
        .await?
        .into_iter()
        .map(|row| Ok((row.try_get("id").map_err(turso_err)?, row)))
        .collect()
    }
}
