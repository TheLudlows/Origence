//! Bounded, scoped catalog reads. Each visible asset reuses the normal read contract.
use super::*;

impl SqliteTx {
    pub async fn asset_catalog(
        &mut self,
        kind: &str,
        after: Uuid,
        limit: usize,
    ) -> StorageResult<Value> {
        self.check_permission(Permission::Read).await?;
        if !matches!(kind, "memory" | "knowledge") || !(1..=100).contains(&limit) {
            return Err(StorageError::InvalidArgument("invalid catalog page"));
        }
        let ids: Vec<Uuid> = sqlite_db::query_scalar(
            "SELECT id FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND kind=? \
             AND NOT deleted AND current_version IS NOT NULL AND id>? ORDER BY id LIMIT ?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(kind)
        .bind(after)
        .bind((limit + 1) as i64)
        .fetch_all(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        let more = ids.len() > limit;
        let mut items = Vec::new();
        let mut cursor = after;
        for id in ids.into_iter().take(limit) {
            cursor = id;
            match self.asset_view(id, None).await {
                Ok(mut value) => {
                    // Catalogs expose metadata, never entire document bodies or quotes.
                    let map = value
                        .as_object_mut()
                        .ok_or_else(|| StorageError::Unavailable("invalid asset record".into()))?;
                    map.remove("semantic_values");
                    items.push(value);
                }
                Err(StorageError::NotFound) => {} // Withdrawn/invalid sources are not visible.
                Err(error) => return Err(error),
            }
        }
        Ok(json!({"items":items,"next_after":more.then_some(cursor)}))
    }

    pub async fn memory_catalog(
        &mut self,
        filter: &crate::memory_identity::MemoryCatalogFilter,
        after: Uuid,
        limit: usize,
    ) -> StorageResult<Value> {
        self.check_permission(Permission::Read).await?;
        filter
            .validate()
            .map_err(|_| StorageError::InvalidArgument("invalid memory filter"))?;
        if !(1..=100).contains(&limit) {
            return Err(StorageError::InvalidArgument("invalid catalog page"));
        }
        // Seek the scoped primary key, with no unindexed predicate before LIMIT:
        // at most 100 candidates plus one lookahead, including hidden/non-memory rows.
        let rows: Vec<(Uuid, String, bool, Option<i32>, Option<String>)> = sqlite_db::query_as(
            "SELECT id,kind,deleted,current_version,identity_json FROM oc_assets \
             WHERE tenant_id=? AND workspace_id=? AND id>? ORDER BY id LIMIT 101",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(after)
        .fetch_all(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        let count = rows.len();
        let mut scanned = 0;
        let mut cursor = after;
        let mut items = Vec::new();
        for (id, kind, deleted, version, identity) in rows.into_iter().take(100) {
            scanned += 1;
            cursor = id;
            if kind == "memory" && !deleted && version.is_some() {
                let identity: Option<crate::memory_identity::MemoryIdentity> = identity
                    .map(|value| serde_json::from_str(&value))
                    .transpose()
                    .map_err(|_| StorageError::Unavailable("invalid memory identity".into()))?;
                if filter.matches(identity.as_ref()) {
                    match self.asset_view(id, None).await {
                        Ok(mut value) => {
                            let map = value.as_object_mut().ok_or_else(|| {
                                StorageError::Unavailable("invalid asset record".into())
                            })?;
                            map.remove("semantic_values");
                            items.push(value);
                        }
                        Err(StorageError::NotFound) => {}
                        Err(error) => return Err(error),
                    }
                }
            }
            if items.len() == limit {
                break;
            }
        }
        Ok(json!({"items":items,"next_after":(scanned < count).then_some(cursor)}))
    }

    pub async fn run_catalog(&mut self, after: Uuid, limit: usize) -> StorageResult<Value> {
        self.check_permission(Permission::Read).await?;
        if !(1..=100).contains(&limit) {
            return Err(StorageError::InvalidArgument("invalid catalog page"));
        }
        let owner = self.private_key_id().await?;
        let rows: Vec<(Uuid, String, i64)> = sqlite_db::query_as(
            "SELECT run_id,input,created_at FROM oc_runs WHERE tenant_id=? AND workspace_id=? \
             AND owner_key_id=? AND run_id>? ORDER BY run_id LIMIT ?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(owner)
        .bind(after)
        .bind((limit + 1) as i64)
        .fetch_all(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        let more = rows.len() > limit;
        let mut cursor = after;
        let mut items = Vec::new();
        for (id, input, created) in rows.into_iter().take(limit) {
            cursor = id;
            let input: crate::run::RunInput = serde_json::from_str(&input)
                .map_err(|_| StorageError::Unavailable("invalid run record".into()))?;
            items.push(json!({"run_id":id,"summary":input.summary,"created_at_ms":created}));
        }
        Ok(json!({"items":items,"next_after":more.then_some(cursor),"scope":"current_logical_key"}))
    }
}
