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
}
