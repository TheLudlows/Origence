//! Fixed native Turso schema installation and literal-preserving admission.
use super::*;

const SCHEMA: &str = include_str!("../sqlite-schema.sql");
pub(super) const IDENTITY_SCHEMA: &str = include_str!("../memory-identity-schema.sql");

pub(super) async fn reject_legacy_aml_schema(conn: &mut Connection) -> StorageResult<()> {
    let exists: bool = native_turso::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name GLOB 'oc_aml_*')",
    )
    .fetch_one(conn)
    .await
    .map_err(turso_err)?;
    if exists {
        return Err(StorageError::Unavailable(
            "legacy AML schema requires a new database; no migration was attempted".into(),
        ));
    }
    Ok(())
}

pub(super) async fn schema_present(conn: &mut Connection) -> StorageResult<bool> {
    native_turso::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='oc_workspaces')",
    )
    .fetch_one(conn)
    .await
    .map_err(turso_err)
}

pub(super) async fn exec_schema(conn: &mut Connection) -> StorageResult<()> {
    for schema in [
        SCHEMA,
        IDENTITY_SCHEMA,
        semantics::SCHEMA,
        session::SCHEMA,
        improve::SCHEMA,
    ] {
        native_turso::raw_sql(schema)
            .execute(&mut *conn)
            .await
            .map_err(turso_err)?;
    }
    Ok(())
}

pub(super) async fn check_conn(conn: &mut Connection) -> StorageResult<()> {
    reject_legacy_aml_schema(conn).await?;
    native_turso::verify_schema(
        conn,
        &[
            SCHEMA,
            IDENTITY_SCHEMA,
            semantics::SCHEMA,
            session::SCHEMA,
            improve::SCHEMA,
        ],
    )
    .await
    .map_err(turso_err)
}

pub(super) async fn identity_table_present(conn: &mut Connection) -> StorageResult<bool> {
    native_turso::query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='oc_memory_identities')")
        .fetch_one(conn).await.map_err(turso_err)
}
