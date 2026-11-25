//! Bounded, scoped catalog reads. Each visible asset reuses the normal read contract.
use super::*;

const COUNT_RUNS_CREATED: &str = "SELECT COUNT(*) FROM oc_runs INDEXED BY oc_runs_owner_created_at \
    WHERE tenant_id=? AND workspace_id=? AND owner_key_id=? AND created_at>=? AND created_at<?";

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
        let workspace_wide = self.live_role().await? == "admin";
        let sql = if workspace_wide {
            "SELECT run_id,owner_key_id,CASE WHEN owner_key_id=?3 THEN input END,created_at \
             FROM oc_runs WHERE tenant_id=?1 AND workspace_id=?2 AND run_id>?4 ORDER BY run_id LIMIT ?5"
        } else {
            "SELECT run_id,owner_key_id,input,created_at FROM oc_runs \
             WHERE tenant_id=?1 AND workspace_id=?2 AND owner_key_id=?3 AND run_id>?4 ORDER BY run_id LIMIT ?5"
        };
        let rows: Vec<(Uuid, Uuid, Option<String>, i64)> = sqlite_db::query_as(sql)
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
        for (id, run_owner, input, created) in rows.into_iter().take(limit) {
            cursor = id;
            let input: Option<crate::run::RunInput> = input
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .map_err(|_| StorageError::Unavailable("invalid run record".into()))?;
            let outcome: Option<String> = sqlite_db::query_scalar(
                "SELECT json_extract(payload,'$.event.outcome') FROM oc_run_events \
                 WHERE tenant_id=? AND workspace_id=? AND run_id=? \
                 AND json_extract(payload,'$.event.kind')='result_reported' ORDER BY sequence DESC LIMIT 1",
            )
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .bind(id)
            .fetch_optional(&mut *self.tx)
            .await
            .map_err(sqlite_err)?
            .flatten();
            let status = match outcome.as_deref() {
                Some("success") => "reported_success",
                Some("failure") => "reported_failure",
                _ => "unknown",
            };
            items.push(json!({
                "run_id":id, "owner_key_id":run_owner, "is_owner":owner==run_owner,
                "summary":input.map(|input| input.summary), "created_at_ms":created, "status":status
            }));
        }
        let scope = if workspace_wide {
            "workspace"
        } else {
            "current_logical_key"
        };
        Ok(json!({"items":items,"next_after":more.then_some(cursor),"scope":scope}))
    }

    /// Count authoritative Run records in the caller's catalog scope.
    pub async fn run_creation_overview_at(
        &mut self,
        window: &str,
        end_ms: i64,
    ) -> StorageResult<Value> {
        self.check_permission(Permission::Read).await?;
        let workspace_wide = self.live_role().await? == "admin";
        let (buckets, width_ms) = match window {
            "24h" => (24, 3_600_000),
            "7d" => (7, 86_400_000),
            _ => return Err(StorageError::InvalidArgument("invalid Run overview window")),
        };
        let start_ms = end_ms
            .checked_sub(i64::from(buckets) * width_ms)
            .filter(|start| *start >= 0)
            .ok_or(StorageError::InvalidArgument("invalid Run overview time"))?;
        let mut counts = vec![0_i64; buckets as usize];
        if workspace_wide {
            // Group in one covering-index scan of this workspace, regardless of the number of buckets.
            let groups: Vec<(i64, i64)> = sqlite_db::query_as(
                "SELECT CAST((created_at-?3)/?5 AS INTEGER) AS bucket,COUNT(*) \
                 FROM oc_runs INDEXED BY oc_runs_owner_created_at \
                 WHERE tenant_id=?1 AND workspace_id=?2 AND created_at>=?3 AND created_at<?4 \
                 GROUP BY bucket",
            )
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .bind(start_ms)
            .bind(end_ms)
            .bind(width_ms)
            .fetch_all(&mut *self.tx)
            .await
            .map_err(sqlite_err)?;
            for (bucket, count) in groups {
                counts[bucket as usize] = count;
            }
        } else {
            let owner = self.private_key_id().await?;
            for (bucket, count) in counts.iter_mut().enumerate() {
                let start = start_ms + bucket as i64 * width_ms;
                *count = sqlite_db::query_scalar(COUNT_RUNS_CREATED)
                    .bind(self.scope.tenant_id)
                    .bind(self.scope.workspace_id)
                    .bind(owner)
                    .bind(start)
                    .bind(start + width_ms)
                    .fetch_one(&mut *self.tx)
                    .await
                    .map_err(sqlite_err)?;
            }
        }
        let total: i64 = counts.iter().sum();
        let series: Vec<_> = counts
            .into_iter()
            .enumerate()
            .map(|(bucket, count)| {
                let start = start_ms + bucket as i64 * width_ms;
                json!({"start_ms":start, "end_ms":start+width_ms, "created_runs":count})
            })
            .collect();
        let scope = if workspace_wide {
            "workspace"
        } else {
            "current_logical_key"
        };
        Ok(json!({
            "scope":scope, "window":window, "timezone":"UTC",
            "start_ms":start_ms, "end_ms":end_ms, "created_runs":total, "series":series
        }))
    }
}
