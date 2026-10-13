//! Generic one-level workspace access and explicit key capability issuance.
use super::*;

fn context(auth: AuthorizedScope) -> AuthContext {
    AuthContext {
        id: auth.principal_id,
        tenant_id: auth.scope.tenant_id,
        workspace_id: auth.scope.workspace_id,
        role: auth.role,
    }
}

impl Service {
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
        let mut tx = self.write(auth, Permission::Delete).await?;
        let key = tx
            .issue_scoped_key_with_capabilities(role, capabilities)
            .await?;
        tx.commit().await?;
        Ok(json!(key))
    }
}
