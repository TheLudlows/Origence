//! Shared relational visibility layers, always evaluated in the caller's transaction.
//!
//! Current retrieval and exact historical reads are deliberately different:
//! historical archives must not acquire a current-version predicate. Local
//! guards use the shared transaction-local batched read model.
//! Local support checks do not authorize public source reads, raw session access,
//! or the transitive Learning closure. Callers retain those specialized guards.
use super::*;

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
