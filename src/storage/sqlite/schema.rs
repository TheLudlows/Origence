//! Fixed schema installation and compatibility checks; no migration policy.
use super::*;

/// The complete schema for a new database, executed only when absent (A2.3).
const SCHEMA: &str = include_str!("../sqlite-schema.sql");
pub(super) const IDENTITY_SCHEMA: &str = include_str!("../memory-identity-schema.sql");

/// Complete column projections required by the fixed local schema.
const REQUIRED_PROJECTIONS: &[(&str, &str)] = &[
    ("oc_workspaces", "tenant_id,id,name,owner_workspace_id,created_at"),
    (
        "oc_api_keys",
        "id,tenant_id,workspace_id,token_hash,role,allow_owned_workspaces,revoked,created_at",
    ),
    (
        "oc_files",
        "tenant_id,workspace_id,id,name,media_type,hash,size,deleted,created_by,created_at",
    ),
    (
        "oc_events",
        "tenant_id,workspace_id,id,content,kind,file_id,state,created_by,created_at",
    ),
    (
        "oc_assets",
        "tenant_id,workspace_id,id,kind,title,fact_key,current_version,deleted,created_at",
    ),
    (
        "oc_versions",
        "tenant_id,workspace_id,asset_id,version,content,content_hash,source_event_id,restored_from,title,created_by,created_at",
    ),
    (
        "oc_jobs",
        "tenant_id,workspace_id,id,created_by,operation,payload,state,run_token,generation,attempt,next_retry_at,asset_id,source_event_id,cancel_requested,outcome,result,error_code,created_at,updated_at",
    ),
    (
        "oc_chunks",
        "tenant_id,workspace_id,id,asset_id,version,ordinal,content,locator,search_terms",
    ),
    (
        "oc_summaries",
        "tenant_id,workspace_id,id,chunk_id,text,model_revision,search_terms",
    ),
    (
        "oc_commands",
        "tenant_id,workspace_id,principal_id,dedup_scope,operation,key,request_hash,response,created_at",
    ),
    (
        "oc_audit",
        "tenant_id,workspace_id,id,actor,action,target,details,created_at",
    ),
    (
        "oc_artifact_owners",
        "tenant_id,workspace_id,source_id,version,artifact_type,artifact_id,chunk_id,created_at",
    ),
    (
        "oc_index_entries",
        "tenant_id,workspace_id,artifact_id,field,model_id,dimension,generation,state,created_at",
    ),
    (
        "oc_artifact_ledger",
        "tenant_id,workspace_id,source_id,version,artifact_type,artifact_id,surface,generation,idempotency_key,state,attempt,last_error,next_retry_at,created_at,updated_at",
    ),
];

pub(super) async fn schema_present(conn: &mut SqliteConnection) -> StorageResult<bool> {
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='oc_workspaces'",
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(sqlite_err)?;
    Ok(n > 0)
}

pub(super) async fn exec_schema(conn: &mut SqliteConnection) -> StorageResult<()> {
    for schema in [
        SCHEMA,
        IDENTITY_SCHEMA,
        aml::AML_SCHEMA,
        aml::ADD_SCHEMA,
        semantics::SCHEMA,
        session::SCHEMA,
        improve::SCHEMA,
    ] {
        sqlx::raw_sql(schema)
            .execute(&mut *conn)
            .await
            .map_err(sqlite_err)?;
    }
    Ok(())
}

pub(super) async fn check_conn(conn: &mut SqliteConnection) -> StorageResult<()> {
    for (table, columns) in REQUIRED_PROJECTIONS {
        sqlx::query(&format!("SELECT {columns} FROM {table} LIMIT 0"))
            .fetch_optional(&mut *conn)
            .await
            .map_err(|e| {
                StorageError::Unavailable(format!("incompatible SQLite schema in {table}: {e}"))
            })?;
    }
    check_workspace_command_schema(conn).await?;
    if aml::schema_available(conn).await? {
        aml::check_schema(conn).await?;
    }
    if semantics::available(conn).await? {
        semantics::check_schema(conn).await?;
    }
    if session::available(conn).await? {
        session::check_schema(conn).await?;
    }
    if improve::available(conn).await? {
        improve::check_schema(conn).await?;
    }
    // Optional for old databases; never installed by startup checks.
    if identity_table_present(conn).await? {
        check_identity_schema(conn).await?;
    }
    Ok(())
}

/// Columns alone cannot establish the owner FK, immutable one-level ownership,
/// capability default, or command uniqueness contract. Reject incomplete or
/// older definitions without attempting to migrate or repair any existing DB.
async fn check_workspace_command_schema(conn: &mut SqliteConnection) -> StorageResult<()> {
    let normalize = |sql: &str| {
        sql.trim_end_matches(';')
            .split_whitespace()
            .collect::<String>()
            .to_ascii_lowercase()
    };
    for (kind, name, terminator) in [
        ("TABLE", "oc_workspaces", ";"),
        ("TABLE", "oc_api_keys", ";"),
        ("TABLE", "oc_commands", ";"),
        ("TRIGGER", "oc_workspace_owner_immutable", "END;"),
        ("TRIGGER", "oc_workspace_owner_root", "END;"),
    ] {
        let prefix = format!("CREATE {kind} {name}");
        let start = SCHEMA.find(&prefix).ok_or_else(|| {
            StorageError::Unavailable("invalid embedded workspace schema".into())
        })?;
        let tail = &SCHEMA[start..];
        let end = tail.find(terminator).ok_or_else(|| {
            StorageError::Unavailable("invalid embedded workspace schema".into())
        })? + terminator.len();
        let actual: Option<String> = sqlx::query_scalar(
            "SELECT sql FROM sqlite_master WHERE type=? AND name=?",
        )
        .bind(kind.to_ascii_lowercase())
        .bind(name)
        .fetch_optional(&mut *conn)
        .await
        .map_err(sqlite_err)?;
        if actual.as_deref().map(&normalize) != Some(normalize(&tail[..end])) {
            return Err(StorageError::Unavailable(format!(
                "incompatible SQLite schema in {name}"
            )));
        }
    }
    Ok(())
}

pub(super) async fn check_identity_schema(conn: &mut SqliteConnection) -> StorageResult<()> {
    let ddl: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='oc_memory_identities'",
    )
    .fetch_one(conn)
    .await
    .map_err(sqlite_err)?;
    let normalize = |sql: &str| {
        sql.trim_end_matches(';')
            .split_whitespace()
            .collect::<String>()
            .to_ascii_lowercase()
    };
    if normalize(&ddl) != normalize(IDENTITY_SCHEMA.trim()) {
        return Err(StorageError::Unavailable(
            "incompatible memory identity schema".into(),
        ));
    }
    Ok(())
}

pub(super) async fn identity_table_present(conn: &mut SqliteConnection) -> StorageResult<bool> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='oc_memory_identities')",
    )
    .fetch_one(conn)
    .await
    .map_err(sqlite_err)?;
    Ok(exists)
}
