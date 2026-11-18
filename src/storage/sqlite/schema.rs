//! Fixed native SQLite schema installation and literal-preserving admission.
use super::*;

const SCHEMA: &str = include_str!("../sqlite-schema.sql");

pub(super) async fn schema_present(conn: &mut Connection) -> StorageResult<bool> {
    sqlite_db::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='oc_workspaces')",
    )
    .fetch_one(conn)
    .await
    .map_err(sqlite_err)
}

pub(super) async fn exec_schema(conn: &mut Connection) -> StorageResult<()> {
    for schema in [
        SCHEMA,
        semantics::SCHEMA,
        session::SCHEMA,
        improve::SCHEMA,
        runs::SCHEMA,
        skill::SCHEMA,
    ] {
        sqlite_db::raw_sql(schema)
            .execute(&mut *conn)
            .await
            .map_err(sqlite_err)?;
    }
    Ok(())
}

pub(super) async fn check_conn(conn: &mut Connection) -> StorageResult<()> {
    sqlite_db::verify_schema(
        conn,
        &[
            SCHEMA,
            semantics::SCHEMA,
            session::SCHEMA,
            improve::SCHEMA,
            runs::SCHEMA,
            skill::SCHEMA,
        ],
    )
    .await
    .map_err(sqlite_err)
}
