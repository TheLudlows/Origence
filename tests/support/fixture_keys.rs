//! Synthetic authority rows used by storage and protocol tests.
#![allow(dead_code)] // Each integration executable uses different fixture operations.
use origence::storage::{
    IssuedKey, Scope, SqliteStore, StorageError, StorageResult, hash, sqlite_db,
};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub trait FixtureKeys {
    async fn create_workspace(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
        name: &str,
    ) -> StorageResult<()>;
    async fn issue_key(&self, scope: Scope, role: &str) -> StorageResult<IssuedKey>;
    async fn revoke_key(&self, key_id: Uuid) -> StorageResult<()>;
}

fn fixture_error(error: impl std::fmt::Display) -> StorageError {
    StorageError::Unavailable(error.to_string())
}

fn fixture_time() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

impl FixtureKeys for SqliteStore {
    async fn create_workspace(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
        name: &str,
    ) -> StorageResult<()> {
        sqlite_db::query(
            "INSERT INTO oc_workspaces(tenant_id, id, name, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(name)
        .bind(fixture_time())
        .execute(self.pool())
        .await
        .map_err(fixture_error)?;
        Ok(())
    }

    async fn issue_key(&self, scope: Scope, role: &str) -> StorageResult<IssuedKey> {
        assert!(matches!(role, "reader" | "writer" | "admin"));
        let key_id = Uuid::new_v4();
        let token = format!("oc_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        sqlite_db::query(
            "INSERT INTO oc_api_keys(id, logical_key_id, tenant_id, workspace_id, token_hash, role, revoked, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, 0, ?)",
        )
        .bind(key_id)
        .bind(key_id)
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(hash(&token))
        .bind(role)
        .bind(fixture_time())
        .execute(self.pool())
        .await
        .map_err(fixture_error)?;
        Ok(IssuedKey { key_id, token })
    }

    async fn revoke_key(&self, key_id: Uuid) -> StorageResult<()> {
        sqlite_db::query("UPDATE oc_api_keys SET revoked = 1 WHERE id = ?")
            .bind(key_id)
            .execute(self.pool())
            .await
            .map_err(fixture_error)?;
        Ok(())
    }
}
