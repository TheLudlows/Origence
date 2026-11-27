//! Provisioning for workspaces with one credential and one stable identity.
use super::*;

pub(super) async fn has_workspace_key(conn: &mut Connection, scope: Scope) -> StorageResult<bool> {
    sqlite_db::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM oc_api_keys WHERE tenant_id=? AND workspace_id=? AND role='workspace')",
    )
    .bind(scope.tenant_id)
    .bind(scope.workspace_id)
    .fetch_one(conn)
    .await
    .map_err(sqlite_err)
}

#[cfg(feature = "local-storage")]
pub(super) async fn active_workspace_key(
    conn: &mut Connection,
    scope: Scope,
    original: Uuid,
) -> StorageResult<Uuid> {
    let logical: Uuid = sqlite_db::query_scalar(
        "SELECT logical_key_id FROM oc_api_keys WHERE tenant_id=? AND workspace_id=? AND id=? AND role='workspace'",
    )
    .bind(scope.tenant_id)
    .bind(scope.workspace_id)
    .bind(original)
    .fetch_optional(&mut *conn)
    .await
    .map_err(sqlite_err)?
    .ok_or(StorageError::Forbidden)?;
    sqlite_db::query_scalar(
        "SELECT id FROM oc_api_keys WHERE tenant_id=? AND workspace_id=? AND logical_key_id=? AND role='workspace' AND NOT revoked",
    )
    .bind(scope.tenant_id)
    .bind(scope.workspace_id)
    .bind(logical)
    .fetch_optional(conn)
    .await
    .map_err(sqlite_err)?
    .ok_or(StorageError::Forbidden)
}

async fn insert_workspace_key(
    conn: &mut Connection,
    scope: Scope,
    logical_id: Uuid,
    key_id: Uuid,
) -> StorageResult<IssuedKey> {
    let token = format!("oc_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    sqlite_db::query(
        "INSERT INTO oc_api_keys(id,logical_key_id,tenant_id,workspace_id,token_hash,role,created_at)
         VALUES(?,?,?,?,?,'workspace',?)",
    )
    .bind(key_id)
    .bind(logical_id)
    .bind(scope.tenant_id)
    .bind(scope.workspace_id)
    .bind(hash(&token))
    .bind(now_ms())
    .execute(conn)
    .await
    .map_err(sqlite_err)?;
    Ok(IssuedKey { key_id, token })
}

impl SqliteStore {
    #[cfg(feature = "local-storage")]
    pub(crate) async fn archive_worker_key(
        &self,
        scope: Scope,
        creator: Uuid,
    ) -> StorageResult<Uuid> {
        let mut tx = self.pool.begin().await.map_err(sqlite_err)?;
        let current = active_workspace_key(&mut tx, scope, creator).await?;
        tx.commit().await.map_err(sqlite_err)?;
        Ok(current)
    }

    /// Atomically create an isolated workspace and its first credential.
    pub async fn create_key_workspace(&self, scope: Scope, name: &str) -> StorageResult<IssuedKey> {
        let name = name.trim();
        if scope.workspace_id.is_nil()
            || scope.tenant_id.is_nil()
            || name.is_empty()
            || name.len() > 256
        {
            return Err(StorageError::InvalidArgument(
                "invalid workspace identity or name",
            ));
        }
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(sqlite_err)?;
        sqlite_db::query("INSERT INTO oc_workspaces(tenant_id,id,name,created_at) VALUES(?,?,?,?)")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(name)
            .bind(now_ms())
            .execute(&mut *tx)
            .await
            .map_err(sqlite_err)?;
        let key_id = Uuid::new_v4();
        let key = insert_workspace_key(&mut tx, scope, key_id, key_id).await?;
        tx.commit().await.map_err(sqlite_err)?;
        Ok(key)
    }

    /// Rotate an existing workspace credential, including recovery after revocation.
    pub async fn rotate_workspace_key(&self, scope: Scope) -> StorageResult<IssuedKey> {
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(sqlite_err)?;
        let logical_id: Uuid = sqlite_db::query_scalar(
            "SELECT logical_key_id FROM oc_api_keys WHERE tenant_id=? AND workspace_id=? AND role='workspace'
             ORDER BY created_at DESC,id DESC LIMIT 1",
        )
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(sqlite_err)?
        .ok_or(StorageError::NotFound)?;
        sqlite_db::query(
            "UPDATE oc_api_keys SET revoked=1 WHERE tenant_id=? AND workspace_id=? AND role='workspace' AND NOT revoked",
        )
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .execute(&mut *tx)
        .await
        .map_err(sqlite_err)?;
        let key = insert_workspace_key(&mut tx, scope, logical_id, Uuid::new_v4()).await?;
        tx.commit().await.map_err(sqlite_err)?;
        Ok(key)
    }
}
