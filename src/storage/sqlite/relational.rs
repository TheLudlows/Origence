//! RelationalStore entry points: authentication, workspaces and API keys.
use super::*;

impl RelationalStore for SqliteStore {
    type Tx = SqliteTx;

    async fn begin(&self, authorized: AuthorizedScope) -> StorageResult<Self::Tx> {
        self.begin_scoped(authorized, true, None).await
    }

    async fn authenticate(&self, token: &str) -> StorageResult<AuthorizedScope> {
        self.authenticate_workspace(token, None)
            .await?
            .ok_or(StorageError::Forbidden)
    }

    async fn reconcile_ledger(&self) -> StorageResult<Value> {
        self.reconcile_ledger_inner().await
    }
}
