//! Opt-in AML namespaces and exact external-user scope mapping.
use super::*;

pub(super) const ADD_SCHEMA: &str = include_str!("../aml-add-schema.sql");

pub(super) const AML_SCHEMA: &str = include_str!("../aml-schema.sql");

pub(super) async fn schema_available(conn: &mut SqliteConnection) -> StorageResult<bool> {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_master WHERE name IN ('oc_aml_namespaces','oc_aml_users','oc_aml_adds')",
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(sqlite_err)?;
    Ok(count != 0)
}

pub(super) async fn check_schema(conn: &mut SqliteConnection) -> StorageResult<()> {
    let normalize = |sql: &str| {
        sql.split_whitespace()
            .collect::<String>()
            .to_ascii_lowercase()
    };
    for (name, expected) in ["oc_aml_namespaces", "oc_aml_users"]
        .into_iter()
        .zip(AML_SCHEMA.split(';').filter(|s| !s.trim().is_empty()))
    {
        let actual: Option<String> =
            sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type='table' AND name=?")
                .bind(name)
                .fetch_optional(&mut *conn)
                .await
                .map_err(sqlite_err)?;
        if actual.as_deref().map(&normalize) != Some(normalize(expected)) {
            return Err(StorageError::Unavailable(
                "incompatible AML scope schema".into(),
            ));
        }
    }
    if add_schema_present(conn).await? {
        let ddl: Option<String> = sqlx::query_scalar(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='oc_aml_adds'",
        )
        .fetch_optional(&mut *conn)
        .await
        .map_err(sqlite_err)?;
        if ddl.as_deref().map(&normalize)
            != Some(normalize(ADD_SCHEMA.trim().trim_end_matches(';')))
        {
            return Err(StorageError::Unavailable(
                "incompatible AML Add schema".into(),
            ));
        }
    }
    Ok(())
}

async fn add_schema_present(conn: &mut SqliteConnection) -> StorageResult<bool> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='oc_aml_adds')")
        .fetch_one(conn)
        .await
        .map_err(sqlite_err)
}

impl SqliteStore {
    /// Explicitly grant a native workspace the AML namespace capability.
    /// Only its live admin may enable it. Derived user scopes cannot enable it.
    pub async fn enable_aml_namespace(&self, auth: AuthorizedScope) -> StorageResult<()> {
        let mut tx = self.begin(auth).await?;
        tx.check_permission(Permission::Delete).await?;
        tx.require_aml_schema().await?;
        let inserted = sqlx::query("INSERT INTO oc_aml_namespaces(tenant_id,workspace_id,created_at) VALUES(?,?,?) ON CONFLICT DO NOTHING")
            .bind(tx.scope.tenant_id).bind(tx.scope.workspace_id).bind(now_ms())
            .execute(&mut *tx.tx).await.map_err(sqlite_err)?.rows_affected();
        if inserted != 0 {
            tx.audit("aml.namespace_enabled", tx.scope.workspace_id, json!({}))
                .await?;
        }
        tx.commit().await
    }

    /// Atomically create or reuse one user workspace within the key's namespace.
    pub async fn ensure_aml_user(
        &self,
        auth: AuthorizedScope,
        user_id: &str,
    ) -> StorageResult<AuthorizedScope> {
        self.aml_user(auth, user_id, true)
            .await?
            .ok_or(StorageError::NotFound)
    }

    /// Read-only lookup. Unknown users return None, never a default workspace.
    pub async fn lookup_aml_user(
        &self,
        auth: AuthorizedScope,
        user_id: &str,
    ) -> StorageResult<Option<AuthorizedScope>> {
        self.aml_user(auth, user_id, false).await
    }

    async fn aml_user(
        &self,
        auth: AuthorizedScope,
        user_id: &str,
        create: bool,
    ) -> StorageResult<Option<AuthorizedScope>> {
        if user_id.is_empty() || user_id.len() > 4096 || user_id.contains('\0') {
            return Err(StorageError::InvalidArgument(
                "user_id must contain 1..4096 UTF-8 bytes without NUL",
            ));
        }
        let mut tx = self.begin_scoped(auth, create).await?;
        tx.check_permission(if create {
            Permission::Write
        } else {
            Permission::Read
        })
        .await?;
        tx.require_aml_schema().await?;
        let enabled: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM oc_aml_namespaces WHERE tenant_id=? AND workspace_id=?)",
        )
        .bind(tx.scope.tenant_id)
        .bind(tx.scope.workspace_id)
        .fetch_one(&mut *tx.tx)
        .await
        .map_err(sqlite_err)?;
        if !enabled {
            return Err(StorageError::Forbidden);
        }
        let existing: Option<Uuid> = sqlx::query_scalar("SELECT workspace_id FROM oc_aml_users WHERE tenant_id=? AND namespace_workspace_id=? AND user_id=?")
            .bind(tx.scope.tenant_id).bind(tx.scope.workspace_id).bind(user_id)
            .fetch_optional(&mut *tx.tx).await.map_err(sqlite_err)?;
        let workspace = match existing {
            Some(id) => Some(id),
            None if create => {
                let id = Uuid::new_v4();
                sqlx::query("INSERT INTO oc_workspaces(tenant_id,id,name,created_at) VALUES(?,?,'AML user',?)")
                    .bind(tx.scope.tenant_id).bind(id).bind(now_ms())
                    .execute(&mut *tx.tx).await.map_err(sqlite_err)?;
                sqlx::query("INSERT INTO oc_aml_users(tenant_id,namespace_workspace_id,user_id,workspace_id,created_at) VALUES(?,?,?,?,?)")
                    .bind(tx.scope.tenant_id).bind(tx.scope.workspace_id).bind(user_id).bind(id).bind(now_ms())
                    .execute(&mut *tx.tx).await.map_err(sqlite_err)?;
                tx.audit("aml.user_created", id, json!({})).await?;
                Some(id)
            }
            None => None,
        };
        let role = delegated_role(&tx.live_role().await?).to_string();
        let result = workspace.map(|workspace_id| AuthorizedScope {
            scope: Scope {
                tenant_id: tx.scope.tenant_id,
                workspace_id,
            },
            principal_id: tx.principal_id,
            role,
        });
        tx.commit().await?;
        Ok(result)
    }
}

// User scopes may read/write evidence, but cannot issue keys or grant namespaces.
fn delegated_role(role: &str) -> &str {
    if Permission::Write.granted_by(role) {
        "writer"
    } else {
        "reader"
    }
}

impl SqliteTx {
    async fn require_aml_schema(&mut self) -> StorageResult<()> {
        if !schema_available(&mut self.tx).await? {
            return Err(StorageError::Unavailable(
                "AML requires a new dedicated evaluation database".into(),
            ));
        }
        Ok(())
    }

    pub(super) async fn aml_role(&mut self) -> StorageResult<String> {
        if !schema_available(&mut self.tx).await? {
            return Err(StorageError::Forbidden);
        }
        let role: Option<String> = sqlx::query_scalar(
            "SELECT k.role FROM oc_api_keys k JOIN oc_aml_namespaces n ON n.tenant_id=k.tenant_id AND n.workspace_id=k.workspace_id JOIN oc_aml_users u ON u.tenant_id=n.tenant_id AND u.namespace_workspace_id=n.workspace_id WHERE k.id=? AND k.tenant_id=? AND u.workspace_id=? AND NOT k.revoked",
        )
        .bind(self.principal_id).bind(self.scope.tenant_id).bind(self.scope.workspace_id)
        .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        role.map(|r| delegated_role(&r).to_string())
            .ok_or(StorageError::Forbidden)
    }
}

impl SqliteTx {
    pub async fn aml_add_record(
        &mut self,
        request_id: &str,
    ) -> StorageResult<Option<crate::aml::AddRecord>> {
        if !add_schema_present(&mut self.tx).await? {
            return Err(StorageError::Unavailable(
                "AML Add requires a new dedicated evaluation database".into(),
            ));
        }
        sqlx::query_as("SELECT request_id,session_id,request_hash,embedding_profile,asset_id,source_event_id,job_id FROM oc_aml_adds WHERE tenant_id=? AND workspace_id=? AND request_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(request_id)
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)
    }

    pub async fn record_aml_add(&mut self, record: &crate::aml::AddRecord) -> StorageResult<()> {
        sqlx::query("INSERT INTO oc_aml_adds(tenant_id,workspace_id,request_id,session_id,request_hash,embedding_profile,asset_id,source_event_id,job_id,created_at) VALUES(?,?,?,?,?,?,?,?,?,?)")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id)
            .bind(&record.request_id).bind(&record.session_id).bind(&record.request_hash).bind(&record.embedding_profile)
            .bind(record.asset_id).bind(record.source_event_id).bind(record.job_id).bind(now_ms())
            .execute(&mut *self.tx).await.map_err(sqlite_err)?;
        Ok(())
    }

    /// The original version and source must still be live, with every chunk indexed.
    pub async fn aml_add_visible(&mut self, record: &crate::aml::AddRecord) -> StorageResult<bool> {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_assets a JOIN oc_versions v ON v.tenant_id=a.tenant_id AND v.workspace_id=a.workspace_id AND v.asset_id=a.id AND v.version=a.current_version JOIN oc_events e ON e.tenant_id=v.tenant_id AND e.workspace_id=v.workspace_id AND e.id=v.source_event_id WHERE a.tenant_id=? AND a.workspace_id=? AND a.id=? AND a.current_version=1 AND NOT a.deleted AND e.id=? AND e.state='active' AND EXISTS(SELECT 1 FROM oc_chunks c WHERE c.tenant_id=a.tenant_id AND c.workspace_id=a.workspace_id AND c.asset_id=a.id AND c.version=1) AND NOT EXISTS(SELECT 1 FROM oc_chunks c WHERE c.tenant_id=a.tenant_id AND c.workspace_id=a.workspace_id AND c.asset_id=a.id AND c.version=1 AND NOT EXISTS(SELECT 1 FROM oc_index_entries i WHERE i.tenant_id=c.tenant_id AND i.workspace_id=c.workspace_id AND i.artifact_id=c.id AND i.model_id=? AND i.state='ready' AND EXISTS(SELECT 1 FROM oc_artifact_ledger l WHERE l.tenant_id=c.tenant_id AND l.workspace_id=c.workspace_id AND l.artifact_id=c.id AND l.source_id=e.id AND l.version=1 AND l.generation=i.generation AND l.surface='vector' AND l.state='committed'))))")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(record.asset_id).bind(record.source_event_id).bind(&record.embedding_profile)
            .fetch_one(&mut *self.tx).await.map_err(sqlite_err)
    }
}
