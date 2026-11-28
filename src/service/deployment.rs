//! Deployment administration. Workspace credentials never enter this path.
use super::*;

impl Service {
    /// Resolve an administrator and the explicitly selected workspace for the
    /// usual business transactions. Each transaction checks the live identity.
    pub async fn authorize_deployment_workspace(
        &self,
        token: &str,
        workspace: Uuid,
    ) -> Result<AuthContext> {
        let auth = self
            .engine
            .relational()
            .authorize_deployment_workspace(token, workspace)
            .await?;
        Ok(AuthContext {
            id: auth.principal_id,
            tenant_id: auth.scope.tenant_id,
            workspace_id: auth.scope.workspace_id,
            role: auth.role,
        })
    }

    pub async fn provision_deployment_workspace(
        &self,
        token: &str,
        workspace: Uuid,
        name: &str,
    ) -> Result<Value> {
        self.engine
            .relational()
            .provision_deployment_workspace(token, workspace, name)
            .await
            .map_err(Into::into)
    }

    pub async fn deployment_workspace_keys(
        &self,
        token: &str,
        workspace: Uuid,
        after: Uuid,
        limit: u32,
    ) -> Result<Value> {
        self.engine
            .relational()
            .deployment_workspace_keys(token, workspace, after, limit)
            .await
            .map_err(Into::into)
    }

    pub async fn rotate_deployment_workspace_key(
        &self,
        token: &str,
        workspace: Uuid,
    ) -> Result<Value> {
        let key = self
            .engine
            .relational()
            .rotate_deployment_workspace_key(token, workspace)
            .await?;
        Ok(json!(key))
    }

    pub async fn authenticate_deployment_admin(&self, token: &str) -> Result<()> {
        self.engine
            .relational()
            .authenticate_deployment_admin(token)
            .await
            .map_err(Into::into)
    }

    pub async fn inspect_workspaces(
        &self,
        token: &str,
        after: Uuid,
        limit: usize,
    ) -> Result<Value> {
        self.engine
            .relational()
            .deployment_workspaces(token, after, limit)
            .await
            .map_err(Into::into)
    }

    pub(super) async fn inspect_workspace(&self, token: &str, workspace: Uuid) -> Result<SqliteTx> {
        self.engine
            .relational()
            .begin_deployment_inspection(token, workspace)
            .await
            .map_err(Into::into)
    }

    pub async fn inspect_sessions(
        &self,
        token: &str,
        workspace: Uuid,
        after: Uuid,
        limit: usize,
    ) -> Result<Value> {
        let mut tx = self.inspect_workspace(token, workspace).await?;
        let result = tx.inspection_sessions(after, limit).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn inspect_turns(
        &self,
        token: &str,
        workspace: Uuid,
        session: Uuid,
        after: i64,
        limit: usize,
    ) -> Result<Value> {
        self.archived_turn_page(
            ReadAccess::Deployment { token, workspace },
            session,
            after,
            limit,
        )
        .await
    }

    pub async fn inspect_runs(
        &self,
        token: &str,
        workspace: Uuid,
        after: Uuid,
        limit: usize,
    ) -> Result<Value> {
        let mut tx = self.inspect_workspace(token, workspace).await?;
        let result = tx.run_catalog_in_scope(after, limit, None, true).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn inspect_run(
        &self,
        token: &str,
        workspace: Uuid,
        run: Uuid,
        after: i64,
        limit: usize,
    ) -> Result<Value> {
        let mut tx = self.inspect_workspace(token, workspace).await?;
        let result = tx.run_trace_in_scope(run, after, limit, None, true).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn inspect_assets(
        &self,
        token: &str,
        workspace: Uuid,
        kind: &str,
        after: Uuid,
        limit: usize,
    ) -> Result<Value> {
        let mut tx = self.inspect_workspace(token, workspace).await?;
        let result = tx.asset_catalog_in_scope(kind, after, limit).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn inspect_asset(
        &self,
        token: &str,
        workspace: Uuid,
        asset: Uuid,
        version: Option<i32>,
    ) -> Result<Value> {
        self.read_asset(ReadAccess::Deployment { token, workspace }, asset, version)
            .await
    }
}
