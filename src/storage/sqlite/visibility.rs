//! Shared relational visibility layers, always evaluated in the caller's transaction.
//!
//! These trusted SQL fragments require fixed c/a/v/e aliases. They accept no
//! caller SQL. Current retrieval and exact historical reads are deliberately
//! different: historical archives must not acquire a current-version predicate.
//! Local support checks do not authorize public source reads, raw session access,
//! or the transitive Learning closure. Callers retain those specialized guards.
use super::*;

/// Current chunks with a live scoped version root. Bind tenant, workspace first.
pub(super) const CURRENT_CHUNK_ROWS: &str = "FROM oc_chunks c JOIN oc_assets a ON a.tenant_id=c.tenant_id AND a.workspace_id=c.workspace_id AND a.id=c.asset_id AND a.current_version=c.version JOIN oc_versions v ON v.tenant_id=c.tenant_id AND v.workspace_id=c.workspace_id AND v.asset_id=c.asset_id AND v.version=c.version JOIN oc_events e ON e.tenant_id=v.tenant_id AND e.workspace_id=v.workspace_id AND e.id=v.source_event_id WHERE c.tenant_id=? AND c.workspace_id=? AND NOT a.deleted AND e.state='active'";

/// Local S2 snapshot sources only. Preserve native Turso JSON behavior, including
/// missing/empty support arrays; stricter Learning provenance is a separate layer.
/// Keep the local anti-join one level deep: nested correlated NOT EXISTS
/// overflows native Turso 0.8.2 query creation on the Windows main stack.
/// oc_events.id is NOT NULL, so an unmatched row means a missing active source.
/// This predicate has no parameters and requires the fixed chunk alias c.
pub(super) const CHUNK_SUPPORT_ACTIVE: &str = "(json_extract(c.locator,'$.parser') IS NOT 'memory-semantics:v1' OR NOT EXISTS(SELECT 1 FROM json_each(c.locator,'$.support_sources') ss LEFT JOIN oc_events se ON se.tenant_id=c.tenant_id AND se.workspace_id=c.workspace_id AND hex(se.id)=upper(replace(ss.value,'-','')) AND se.state='active' WHERE se.id IS NULL))";

/// Active asset/version root, selecting exact history when a version is supplied.
/// Bind optional version, asset, tenant, workspace, in that order. No S2 member
/// source is substituted for the immutable version root.
pub(super) const ACTIVE_ASSET_VERSION: &str = "FROM oc_assets a JOIN oc_versions v ON v.asset_id = a.id AND v.tenant_id = a.tenant_id AND v.workspace_id = a.workspace_id AND v.version = COALESCE(?, a.current_version) JOIN oc_events e ON e.id = v.source_event_id AND e.tenant_id = v.tenant_id AND e.workspace_id = v.workspace_id WHERE a.id = ? AND a.tenant_id = ? AND a.workspace_id = ? AND NOT a.deleted AND e.state = 'active'";

impl TursoTx {
    /// Exact historical root plus complete local S2 support, without recursion or
    /// writer-only session permission. The Learning DFS uses this same local layer.
    pub(super) async fn require_active_version_local(
        &mut self,
        asset: Uuid,
        version: i32,
    ) -> StorageResult<Uuid> {
        let root: Option<Uuid> =
            native_turso::query_scalar(&format!("SELECT v.source_event_id {ACTIVE_ASSET_VERSION}"))
                .bind(version)
                .bind(asset)
                .bind(self.scope.tenant_id)
                .bind(self.scope.workspace_id)
                .fetch_optional(&mut *self.tx)
                .await
                .map_err(turso_err)?;
        let root = root.ok_or(StorageError::NotFound)?;
        if !self.semantic_version_visible_local(asset, version).await? {
            return Err(StorageError::NotFound);
        }
        Ok(root)
    }

    pub(super) async fn semantic_version_visible_local(
        &mut self,
        asset: Uuid,
        version: i32,
    ) -> StorageResult<bool> {
        // Each semantic chunk carries the full snapshot support set. A missing
        // source hides the entire version, not just that member's chunk.
        let missing: bool = native_turso::query_scalar(&format!(
            "SELECT EXISTS(SELECT 1 FROM oc_chunks c WHERE c.tenant_id=? AND c.workspace_id=? AND c.asset_id=? AND c.version=? AND NOT {CHUNK_SUPPORT_ACTIVE})"
        ))
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(asset)
        .bind(version)
        .fetch_one(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        Ok(!missing)
    }
}

#[cfg(test)]
#[path = "visibility/predicate_tests.rs"]
mod predicate_tests;
