//! Generic one-level workspace access and explicit key capability issuance.
use super::*;
use crate::storage::WorkspaceAccess;

fn authentication_error(error: StorageError) -> AppError {
    match error {
        StorageError::Forbidden => AppError::Unauthorized,
        other => other.into(),
    }
}

fn context(auth: AuthorizedScope) -> AuthContext {
    AuthContext {
        id: auth.principal_id,
        tenant_id: auth.scope.tenant_id,
        workspace_id: auth.scope.workspace_id,
        role: auth.role,
    }
}

impl Service {
    /// Authenticate a key and resolve an explicit workspace or its default.
    pub async fn auth_workspace(
        &self,
        token: &str,
        workspace: Option<Uuid>,
    ) -> Result<AuthContext> {
        self.engine
            .relational()
            .authenticate_workspace(token, workspace)
            .await
            .map_err(authentication_error)?
            .map(context)
            .ok_or_else(|| {
                if workspace.is_some() {
                    AppError::Forbidden
                } else {
                    AppError::Invalid("select an authorized workspace".into())
                }
            })
    }

    pub async fn workspace_access(&self, token: &str) -> Result<Vec<WorkspaceAccess>> {
        self.engine
            .relational()
            .workspace_access(token)
            .await
            .map_err(authentication_error)
    }

    /// Set or revoke a logical key's grant in the authenticated workspace.
    pub async fn set_workspace_key_grant(
        &self,
        auth: &AuthContext,
        logical_key_id: Uuid,
        role: Option<&str>,
    ) -> Result<()> {
        let mut tx = self.write(auth, Permission::WorkspaceAdmin).await?;
        tx.set_workspace_key_grant(logical_key_id, role).await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn list_managed_workspaces(
        &self,
        auth: &AuthContext,
        after: Uuid,
        limit: u32,
    ) -> Result<Value> {
        if !(1..=100).contains(&limit) {
            return Err(AppError::Invalid("limit must be 1..100".into()));
        }
        let mut rows = self
            .engine
            .relational()
            .list_managed_workspaces(authorized(auth), after, limit + 1)
            .await?;
        let has_next = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        let next_after = if has_next {
            rows.last().map(|(id, _, _)| *id)
        } else {
            None
        };
        let items: Vec<_> = rows
            .into_iter()
            .map(|(workspace_id, name, role)| json!({"workspace_id":workspace_id,"name":name,"role":role}))
            .collect();
        Ok(json!({"items":items,"next_after":next_after}))
    }

    pub(crate) async fn create_managed_workspace(
        &self,
        auth: &AuthContext,
        workspace_id: Uuid,
        name: &str,
    ) -> Result<Value> {
        if workspace_id.is_nil() {
            return Err(AppError::Invalid("workspace ID must not be nil".into()));
        }
        let scoped = self
            .engine
            .relational()
            .create_managed_workspace(authorized(auth), workspace_id, name.trim())
            .await?;
        Ok(json!({"workspace_id":scoped.scope.workspace_id}))
    }

    /// Create or validate an immutable child of the authenticated key's root.
    /// The store checks the live key, capability, role and root in one write
    /// transaction. Returned contexts remain subject to live checks on every
    /// operation; changing the workspace field does not grant authority.
    pub async fn ensure_owned_workspace(
        &self,
        auth: &AuthContext,
        workspace_id: Uuid,
        name: &str,
    ) -> Result<AuthContext> {
        self.engine
            .relational()
            .ensure_owned_workspace(authorized(auth), workspace_id, name)
            .await
            .map(context)
            .map_err(Into::into)
    }

    /// Look up an owned workspace without creating it. A missing target is
    /// returned only after the live root reader and capability are authorized.
    pub async fn lookup_owned_workspace(
        &self,
        auth: &AuthContext,
        workspace_id: Uuid,
    ) -> Result<Option<AuthContext>> {
        self.engine
            .relational()
            .lookup_owned_workspace(authorized(auth), workspace_id)
            .await
            .map(|found| found.map(context))
            .map_err(Into::into)
    }

    /// Explicitly issue a key with owned-workspace access. Such keys are trusted
    /// server credentials that can access every child of their root. They must
    /// not be distributed as end-user credentials. Ordinary issuance defaults
    /// to no capability, even for admins or rotation of an existing capable key.
    pub async fn issue_key_with_capabilities(
        &self,
        auth: &AuthContext,
        role: &str,
        capabilities: KeyCapabilities,
    ) -> Result<Value> {
        let mut tx = self.write(auth, Permission::WorkspaceAdmin).await?;
        let key = tx
            .issue_scoped_key_with_capabilities(role, capabilities)
            .await?;
        tx.commit().await?;
        Ok(json!(key))
    }

    pub async fn list_issued_keys(
        &self,
        auth: &AuthContext,
        after: Uuid,
        limit: u32,
    ) -> Result<Value> {
        let mut tx = self.read(auth, Permission::WorkspaceAdmin).await?;
        let page = tx.list_issued_keys(after, limit).await?;
        tx.commit().await?;
        Ok(page)
    }

    pub async fn list_workspace_key_grants(
        &self,
        auth: &AuthContext,
        after: Uuid,
        limit: u32,
    ) -> Result<Value> {
        let mut tx = self.read(auth, Permission::WorkspaceAdmin).await?;
        let page = tx.list_workspace_key_grants(after, limit).await?;
        tx.commit().await?;
        Ok(page)
    }
}
