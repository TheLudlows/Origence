use super::*;

impl Service {
    pub async fn asset_runs(
        &self,
        auth: &AuthContext,
        asset: Uuid,
        version: i32,
        after: Uuid,
        limit: usize,
    ) -> Result<Value> {
        let mut tx = self.read(auth, Permission::Read).await?;
        let value = tx.asset_runs(asset, version, after, limit).await?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn list_assets(
        &self,
        auth: &AuthContext,
        kind: &str,
        after: Uuid,
        limit: usize,
    ) -> Result<Value> {
        let mut tx = self.read(auth, Permission::Read).await?;
        let value = tx.asset_catalog(kind, after, limit).await?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn list_memories(
        &self,
        auth: &AuthContext,
        filter: &crate::memory_identity::MemoryCatalogFilter,
        after: Uuid,
        limit: usize,
    ) -> Result<Value> {
        let mut tx = self.read(auth, Permission::Read).await?;
        let value = tx.memory_catalog(filter, after, limit).await?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn list_runs(&self, auth: &AuthContext, after: Uuid, limit: usize) -> Result<Value> {
        let mut tx = self.read(auth, Permission::Read).await?;
        let value = tx.run_catalog(after, limit).await?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn run_creation_overview(&self, auth: &AuthContext, window: &str) -> Result<Value> {
        let mut tx = self.read(auth, Permission::Read).await?;
        let value = tx
            .run_creation_overview_at(window, crate::storage::sqlite::now_ms())
            .await?;
        tx.commit().await?;
        Ok(value)
    }
}
