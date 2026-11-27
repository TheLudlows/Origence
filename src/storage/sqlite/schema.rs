//! Fixed native SQLite schema installation and literal-preserving admission.
use super::*;

const SCHEMA: &str = include_str!("../sqlite-schema.sql");
#[cfg(feature = "local-storage")]
const LEGACY_LEARNING_CATALOG: &str = include_str!("../learning-catalog-v1.sql");

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
        private_archive::SCHEMA,
        private_archive::NATIVE_SCHEMA,
        improve::SCHEMA,
        improve::CATALOG_SCHEMA,
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
            private_archive::SCHEMA,
            private_archive::NATIVE_SCHEMA,
            improve::SCHEMA,
            improve::CATALOG_SCHEMA,
            runs::SCHEMA,
            skill::SCHEMA,
        ],
    )
    .await
    .map_err(sqlite_err)
}

#[cfg(feature = "local-storage")]
pub(super) async fn check_legacy_archive_schema(conn: &mut Connection) -> StorageResult<()> {
    check_legacy_archive_tables(conn, None).await
}

#[cfg(feature = "local-storage")]
pub(super) async fn check_legacy_v1_archive_schema(conn: &mut Connection) -> StorageResult<()> {
    check_legacy_archive_tables(conn, Some(private_archive::SCHEMA)).await
}

#[cfg(feature = "local-storage")]
async fn check_legacy_archive_tables(
    conn: &mut Connection,
    archive_schema: Option<&str>,
) -> StorageResult<()> {
    sqlite_db::check_format(conn, "authority")
        .await
        .map_err(sqlite_err)?;
    let mut schema = vec![
        SCHEMA,
        semantics::SCHEMA,
        session::SCHEMA,
        improve::SCHEMA,
        LEGACY_LEARNING_CATALOG,
        runs::SCHEMA,
        skill::SCHEMA,
    ];
    schema.extend(archive_schema);
    sqlite_db::verify_schema(conn, &schema)
        .await
        .map_err(sqlite_err)
}

/// The explicit archive conversion runs under the caller's exclusive
/// transaction. Existing facts and the old Improve-to-Learning relationships
/// retain their original IDs, bytes, and support ordinals.
#[cfg(feature = "local-storage")]
pub(super) async fn convert_legacy_learning_catalog(conn: &mut Connection) -> StorageResult<()> {
    for ddl in [
        "ALTER TABLE oc_learning_support RENAME TO legacy_learning_support;
         ALTER TABLE oc_learnings RENAME TO legacy_learnings;
         DROP INDEX oc_learning_turn_support;
         DROP INDEX oc_learning_fingerprint;",
        improve::CATALOG_SCHEMA,
        "INSERT INTO oc_learnings(tenant_id,workspace_id,id,improve_id,session_id,candidate_index,asset_id,source_event_id,ingest_job_id,candidate,fingerprint,status,created_by,created_at,published_at)
         SELECT tenant_id,workspace_id,id,improve_id,session_id,candidate_index,asset_id,source_event_id,ingest_job_id,candidate,fingerprint,status,created_by,created_at,published_at FROM legacy_learnings;
         INSERT INTO oc_learning_support(tenant_id,workspace_id,learning_id,session_id,turn_id,support_ordinal,principal_id,feedback_id)
         SELECT tenant_id,workspace_id,learning_id,session_id,turn_id,support_ordinal,principal_id,feedback_id FROM legacy_learning_support;
         DROP TABLE legacy_learning_support;
         DROP TABLE legacy_learnings;",
    ] {
        sqlite_db::raw_sql(ddl)
            .execute(&mut *conn)
            .await
            .map_err(sqlite_err)?;
    }
    Ok(())
}
