//! Shared relational visibility layers, always evaluated in the caller's transaction.
//!
//! Current retrieval and exact historical reads are deliberately different:
//! historical archives must not acquire a current-version predicate. The SQL
//! fragments below remain only for callers not yet converted to single-table
//! reads; the local guards use the shared batched read model instead.
//! Local support checks do not authorize public source reads, raw session access,
//! or the transitive Learning closure. Callers retain those specialized guards.
use super::*;

/// Current chunks with a live scoped version root. Bind tenant, workspace first.
pub(super) const CURRENT_CHUNK_ROWS: &str = "FROM oc_chunks c JOIN oc_assets a ON a.tenant_id=c.tenant_id AND a.workspace_id=c.workspace_id AND a.id=c.asset_id AND a.current_version=c.version JOIN oc_versions v ON v.tenant_id=c.tenant_id AND v.workspace_id=c.workspace_id AND v.asset_id=c.asset_id AND v.version=c.version JOIN oc_events e ON e.tenant_id=v.tenant_id AND e.workspace_id=v.workspace_id AND e.id=v.source_event_id WHERE c.tenant_id=? AND c.workspace_id=? AND NOT a.deleted AND e.state='active'";

/// Local S2 snapshot sources only. Preserve native Turso JSON behavior, including
/// missing/empty support arrays; stricter Learning provenance is a separate layer.
/// This predicate has no parameters and requires the fixed chunk alias c.
pub(super) const CHUNK_SUPPORT_ACTIVE: &str = "(json_extract(c.locator,'$.parser') IS NOT 'memory-semantics:v1' OR NOT EXISTS(SELECT 1 FROM json_each(c.locator,'$.support_sources') ss WHERE NOT EXISTS(SELECT 1 FROM oc_events se WHERE se.tenant_id=c.tenant_id AND se.workspace_id=c.workspace_id AND hex(se.id)=upper(replace(ss.value,'-','')) AND se.state='active')))";

impl TursoTx {
    pub(super) async fn semantic_version_visible_local(
        &mut self,
        asset: Uuid,
        version: i32,
    ) -> StorageResult<bool> {
        // Each semantic chunk carries the full snapshot support set. A missing
        // source hides the entire version, not just that member's chunk.
        self.local_support_visibility(&[(asset, version)])
            .await?
            .get(&(asset, version))
            .copied()
            .ok_or_else(|| {
                StorageError::Unavailable("inconsistent local support visibility".into())
            })
    }
}
