//! Issue #52 T0 probes: direct Rust SDK, synthetic temporary databases only.
//! These tests do not switch Origence's backend or prove its full domain contract.
use std::error::Error;
use tempfile::TempDir;
use turso::{Builder, Connection, Database, Value};

type Result<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

// Import the real DDL rather than maintaining a simplified compatibility copy.
const BASE: &str = include_str!("../../../src/storage/sqlite-schema.sql");
const GRAPH: &str = include_str!("../../../src/storage/sqlite-graph-schema.sql");

async fn database(path: &std::path::Path, fts: bool) -> Result<Database> {
    let path = path.to_str().ok_or("non-UTF8 temporary path")?;
    Ok(Builder::new_local(path)
        .experimental_index_method(fts)
        .build()
        .await?)
}

async fn fresh(fts: bool) -> Result<(TempDir, Database, Connection)> {
    let dir = tempfile::tempdir()?;
    let db = database(&dir.path().join("probe.db"), fts).await?;
    let conn = db.connect()?;
    conn.execute("PRAGMA foreign_keys = ON", ()).await?;
    Ok((dir, db, conn))
}

async fn count(conn: &Connection, sql: &str) -> Result<i64> {
    let mut rows = conn.query(sql, ()).await?;
    let value = rows.next().await?.ok_or("missing scalar row")?.get(0)?;
    assert!(rows.next().await?.is_none());
    Ok(value)
}

async fn install(conn: &Connection) -> Result {
    conn.execute("BEGIN IMMEDIATE", ()).await?;
    conn.execute_batch(BASE).await?;
    conn.execute_batch(GRAPH).await?;
    conn.execute("COMMIT", ()).await?;
    Ok(())
}

async fn root_workspace(conn: &Connection) -> Result {
    conn.execute(
        "INSERT INTO oc_workspaces(tenant_id,id,name,created_at) VALUES(?1,?2,'root',1)",
        vec![Value::Blob(vec![1; 16]), Value::Blob(vec![2; 16])],
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn real_base_and_graph_ddl_commit_and_reopen() -> Result {
    let (dir, db, conn) = fresh(false).await?;
    install(&conn).await?;
    root_workspace(&conn).await?;
    assert_eq!(count(&conn, "SELECT count(*) FROM oc_workspaces").await?, 1);
    drop(conn);
    drop(db);
    let reopened = database(&dir.path().join("probe.db"), false).await?;
    let conn = reopened.connect()?;
    assert_eq!(count(&conn, "SELECT count(*) FROM oc_workspaces").await?, 1);
    assert_eq!(count(&conn, "SELECT count(*) FROM oc_graph_entities").await?, 0);
    assert_eq!(count(&conn, "SELECT count(*) FROM oc_graph_relations").await?, 0);
    Ok(())
}

#[tokio::test]
async fn blob_json_and_epoch_round_trip() -> Result {
    let (_dir, _db, conn) = fresh(false).await?;
    conn.execute("CREATE TABLE probe_values(id BLOB PRIMARY KEY, payload TEXT, ts INTEGER)", ())
        .await?;
    let blob = vec![0, 255, 128, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
    conn.execute(
        "INSERT INTO probe_values VALUES(?1,?2,?3)",
        vec![
            Value::Blob(blob.clone()),
            Value::Text(r#"{"text":"生产发布 café","enabled":true}"#.into()),
            Value::Integer(1_791_802_801_123),
        ],
    )
    .await?;
    let mut rows = conn
        .query("SELECT id,json_extract(payload,'$.text'),json_extract(payload,'$.enabled'),ts FROM probe_values", ())
        .await?;
    let row = rows.next().await?.ok_or("missing encoded row")?;
    assert_eq!(row.get_value(0)?, Value::Blob(blob));
    assert_eq!(row.get::<String>(1)?, "生产发布 café");
    assert_eq!(row.get::<i64>(2)?, 1);
    assert_eq!(row.get::<i64>(3)?, 1_791_802_801_123);
    assert!(rows.next().await?.is_none());
    Ok(())
}

#[tokio::test]
async fn real_workspace_ownership_trigger_rejects_update() -> Result {
    let (_dir, _db, conn) = fresh(false).await?;
    install(&conn).await?;
    root_workspace(&conn).await?;
    conn.execute(
        "INSERT INTO oc_workspaces(tenant_id,id,name,owner_workspace_id,created_at) VALUES(?1,?2,'child',?3,1)",
        vec![Value::Blob(vec![1; 16]), Value::Blob(vec![3; 16]), Value::Blob(vec![2; 16])],
    )
    .await?;
    let error = conn
        .execute("UPDATE oc_workspaces SET owner_workspace_id=NULL WHERE name='child'", ())
        .await
        .expect_err("immutable ownership trigger must reject the update");
    assert!(error.to_string().contains("workspace ownership is immutable"), "{error}");
    assert_eq!(count(&conn, "SELECT count(*) FROM oc_workspaces WHERE owner_workspace_id IS NOT NULL").await?, 1);
    Ok(())
}

#[tokio::test]
async fn real_graph_foreign_key_cascade() -> Result {
    let (_dir, _db, conn) = fresh(false).await?;
    install(&conn).await?;
    conn.execute_batch(
        "INSERT INTO oc_graph_entities VALUES('t|w|a','t','w','a','A','type','','s',1);
         INSERT INTO oc_graph_entities VALUES('t|w|b','t','w','b','B','type','','s',1);
         INSERT INTO oc_graph_relations VALUES('r','t','w','t|w|a','t|w|b','knows','s',1);",
    )
    .await?;
    assert_eq!(count(&conn, "SELECT count(*) FROM oc_graph_relations").await?, 1);
    conn.execute("DELETE FROM oc_graph_entities WHERE id='a'", ()).await?;
    assert_eq!(count(&conn, "SELECT count(*) FROM oc_graph_relations").await?, 0);
    assert_eq!(count(&conn, "SELECT count(*) FROM oc_graph_entities").await?, 1);
    Ok(())
}

#[tokio::test]
async fn relation_and_graph_writes_share_rollback() -> Result {
    let (_dir, _db, conn) = fresh(false).await?;
    install(&conn).await?;
    conn.execute("BEGIN IMMEDIATE", ()).await?;
    root_workspace(&conn).await?;
    conn.execute("INSERT INTO oc_graph_entities VALUES('t|w|a','t','w','a','A','type','','s',1)", ())
        .await?;
    conn.execute("ROLLBACK", ()).await?;
    assert_eq!(count(&conn, "SELECT count(*) FROM oc_workspaces").await?, 0);
    assert_eq!(count(&conn, "SELECT count(*) FROM oc_graph_entities").await?, 0);
    Ok(())
}

#[tokio::test]
async fn json_table_function_materialized_cte_and_returning() -> Result {
    let (_dir, _db, conn) = fresh(false).await?;
    conn.execute("CREATE TABLE probe_jobs(id INTEGER PRIMARY KEY,payload TEXT,generation INTEGER)", ())
        .await?;
    conn.execute("INSERT INTO probe_jobs VALUES(1,'{\"publication\":{\"chunks\":[{\"id\":\"a\"},{\"id\":\"b\"}]}}',1)", ())
        .await?;
    assert_eq!(count(&conn,
        "WITH expected AS MATERIALIZED (SELECT json_extract(chunk.value,'$.id') AS id FROM probe_jobs j,json_each(j.payload,'$.publication.chunks') chunk) SELECT count(DISTINCT id) FROM expected"
    ).await?, 2);
    let mut rows = conn
        .query("UPDATE probe_jobs SET generation=generation+1,payload=json_set(payload,'$.ready',json('true')) WHERE id=1 RETURNING generation", ())
        .await?;
    assert_eq!(rows.next().await?.ok_or("missing RETURNING row")?.get::<i64>(0)?, 2);
    assert!(rows.next().await?.is_none());
    drop(rows);
    assert_eq!(count(&conn, "SELECT json_extract(payload,'$.ready') FROM probe_jobs").await?, 1);
    Ok(())
}

#[tokio::test]
async fn dropped_returning_can_be_rolled_back_and_connection_reused() -> Result {
    let (_dir, _db, conn) = fresh(false).await?;
    conn.execute("CREATE TABLE probe_returning(id INTEGER PRIMARY KEY)", ()).await?;
    conn.execute("BEGIN IMMEDIATE", ()).await?;
    let mut rows = conn
        .query("INSERT INTO probe_returning VALUES(1),(2),(3) RETURNING id", ())
        .await?;
    assert!(rows.next().await?.is_some());
    drop(rows);
    conn.execute("ROLLBACK", ()).await?;
    assert_eq!(count(&conn, "SELECT count(*) FROM probe_returning").await?, 0);
    conn.execute("INSERT INTO probe_returning VALUES(4)", ()).await?;
    assert_eq!(count(&conn, "SELECT count(*) FROM probe_returning").await?, 1);
    Ok(())
}

async fn fts_fixture(conn: &Connection) -> Result {
    // Composite BLOB PK mirrors chunks; do not assume an INTEGER domain ID.
    conn.execute("CREATE TABLE probe_docs(tenant BLOB NOT NULL, workspace BLOB NOT NULL,id BLOB NOT NULL,terms TEXT NOT NULL, PRIMARY KEY(tenant,workspace,id))", ())
        .await?;
    conn.execute("CREATE INDEX probe_docs_fts ON probe_docs USING fts(terms) WITH (tokenizer='whitespace')", ())
        .await?;
    Ok(())
}

async fn doc(conn: &Connection, scope: u8, id: u8, terms: &str) -> Result {
    conn.execute(
        "INSERT INTO probe_docs VALUES(?1,?2,?3,?4)",
        vec![Value::Blob(vec![scope; 16]), Value::Blob(vec![scope; 16]), Value::Blob(vec![id; 16]), Value::Text(terms.into())],
    )
    .await?;
    Ok(())
}

async fn matches(conn: &Connection, query: &str) -> Result<Vec<Vec<u8>>> {
    let mut rows = conn.query("SELECT id FROM probe_docs WHERE fts_match(terms,?1)", [query]).await?;
    let mut ids = Vec::new();
    while let Some(row) = rows.next().await? {
        match row.get_value(0)? {
            Value::Blob(id) => ids.push(id),
            other => return Err(format!("expected BLOB identifier, got {other:?}").into()),
        }
    }
    ids.sort();
    Ok(ids)
}

#[tokio::test]
async fn fts_and_multilingual_commit_update_delete_reopen() -> Result {
    let (dir, db, conn) = fresh(true).await?;
    fts_fixture(&conn).await?;
    conn.execute("BEGIN IMMEDIATE", ()).await?;
    doc(&conn, 1, 1, "生产 发布 审批 café release_v2").await?;
    doc(&conn, 1, 2, "生产 审批").await?;
    conn.execute("COMMIT", ()).await?;
    assert_eq!(matches(&conn, "生产 AND 发布").await?, vec![vec![1; 16]]);
    assert_eq!(matches(&conn, "café").await?, vec![vec![1; 16]]);
    assert_eq!(matches(&conn, "release_v2").await?, vec![vec![1; 16]]);
    conn.execute("UPDATE probe_docs SET terms='updated' WHERE id=?1", vec![Value::Blob(vec![1; 16])]).await?;
    assert!(matches(&conn, "发布").await?.is_empty());
    assert_eq!(matches(&conn, "updated").await?, vec![vec![1; 16]]);
    conn.execute("DELETE FROM probe_docs WHERE id=?1", vec![Value::Blob(vec![1; 16])]).await?;
    assert!(matches(&conn, "updated").await?.is_empty());
    conn.execute("OPTIMIZE INDEX probe_docs_fts", ()).await?;
    drop(conn);
    drop(db);
    let reopened = database(&dir.path().join("probe.db"), true).await?;
    let conn = reopened.connect()?;
    assert_eq!(matches(&conn, "审批").await?, vec![vec![2; 16]]);
    assert!(matches(&conn, "updated").await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn fts_rollback_does_not_publish_or_remove_committed_text() -> Result {
    let (_dir, _db, conn) = fresh(true).await?;
    fts_fixture(&conn).await?;
    doc(&conn, 1, 1, "committed").await?;
    conn.execute("BEGIN IMMEDIATE", ()).await?;
    doc(&conn, 1, 2, "uncommitted").await?;
    conn.execute("UPDATE probe_docs SET terms='replaced' WHERE id=?1", vec![Value::Blob(vec![1; 16])]).await?;
    conn.execute("ROLLBACK", ()).await?;
    assert_eq!(matches(&conn, "committed").await?, vec![vec![1; 16]]);
    assert!(matches(&conn, "uncommitted").await?.is_empty());
    assert!(matches(&conn, "replaced").await?.is_empty());
    assert_eq!(count(&conn, "SELECT count(*) FROM probe_docs").await?, 1);
    Ok(())
}

#[tokio::test]
async fn fts_scope_filter_before_limit_and_score_observation() -> Result {
    let (_dir, _db, conn) = fresh(true).await?;
    fts_fixture(&conn).await?;
    conn.execute("BEGIN IMMEDIATE", ()).await?;
    for id in 2..22 {
        doc(&conn, 2, id, "needle").await?;
    }
    doc(&conn, 1, 1, "needle padded longer document").await?;
    conn.execute("COMMIT", ()).await?;
    let mut rows = conn.query(
        "SELECT id,fts_score(terms,?1) FROM probe_docs WHERE fts_match(terms,?1) AND tenant=?2 AND workspace=?3 ORDER BY fts_score(terms,?1) DESC LIMIT 1",
        vec![Value::Text("needle".into()), Value::Blob(vec![1; 16]), Value::Blob(vec![1; 16])],
    ).await?;
    let row = rows.next().await?.ok_or("other tenants consumed the candidate limit")?;
    assert_eq!(row.get_value(0)?, Value::Blob(vec![1; 16]));
    assert!(row.get::<f64>(1)?.is_finite());
    assert!(rows.next().await?.is_none());
    drop(rows);
    let mut rows = conn.query(
        "SELECT id,fts_score(terms,'needle') FROM probe_docs WHERE fts_match(terms,'needle') ORDER BY id",
        (),
    ).await?;
    while let Some(row) = rows.next().await? {
        // Synthetic-only diagnostic. Record actual engine scores; do not infer
        // the ranking direction from contradictory documentation examples.
        println!("FTS_SCORE {:?} {}", row.get_value(0)?, row.get::<f64>(1)?);
    }
    Ok(())
}
