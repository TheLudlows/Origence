//! Native title inheritance and exact historical cleanup contracts.
use super::*;
use crate::storage::native_turso as db;

async fn database() -> (tempfile::TempDir, TursoStore, Scope) {
    let dir = tempfile::tempdir().unwrap();
    let store = TursoStore::open(dir.path().join("versions.db"))
        .await
        .unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "versions")
        .await
        .unwrap();
    (dir, store, scope)
}

async fn transaction(pool: &Pool, scope: Scope) -> TursoTx {
    TursoTx {
        tx: pool.begin_with("BEGIN IMMEDIATE").await.unwrap(),
        scope,
        principal_id: Uuid::nil(),
        required: Vec::new(),
        command: None,
    }
}

async fn seed_root(conn: &mut Connection, scope: Scope, asset: Uuid, source: Uuid, title: &str) {
    db::query("INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,current_version,created_at) VALUES(?,?,?,'knowledge',?,2,0)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(asset)
        .bind(title)
        .execute(&mut *conn)
        .await
        .unwrap();
    db::query("INSERT INTO oc_events(tenant_id,workspace_id,id,content,kind,created_by,created_at) VALUES(?,?,?,'immutable source','knowledge',?,0)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(source)
        .bind(Uuid::nil())
        .execute(&mut *conn)
        .await
        .unwrap();
}

async fn chunk(tx: &mut TursoTx, asset: Uuid, version: i32, id: Uuid) {
    tx.insert_chunk_inner(id, asset, version, 0, "chunk", &json!({}), "chunk")
        .await
        .unwrap();
}

#[tokio::test]
async fn version_titles_preserve_empty_explicit_and_native_inheritance() {
    let (_dir, store, scope) = database().await;
    let foreign = Scope {
        tenant_id: scope.tenant_id,
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(foreign.tenant_id, foreign.workspace_id, "foreign")
        .await
        .unwrap();
    let asset = Uuid::new_v4();
    let source = Uuid::new_v4();
    let mut tx = transaction(&store.pool, scope).await;
    seed_root(&mut tx.tx, scope, asset, source, "inherited").await;
    seed_root(&mut tx.tx, foreign, asset, source, "wrong scope").await;
    for (version, title) in [(1, None), (2, Some("")), (3, Some("explicit"))] {
        tx.insert_version_inner(asset, version, "body", "hash", source, Some(1), title)
            .await
            .unwrap();
    }
    // A BLOB stored in a TEXT-affinity title must stay a BLOB, not become a
    // String decode error or a lossy text conversion before the INSERT.
    db::query("UPDATE oc_assets SET title=? WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(vec![0_u8, 255, 65])
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(asset)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    tx.insert_version_inner(asset, 4, "body", "hash", source, None, None)
        .await
        .unwrap();
    tx.insert_version_inner(asset, 5, "body", "hash", source, None, Some(""))
        .await
        .unwrap();
    // Insertion historically has no live/deleted/current-version read guard.
    db::query("UPDATE oc_assets SET title=42,deleted=2,current_version=NULL WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(asset)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_events SET state='retracted' WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(source)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    tx.insert_version_inner(asset, 6, "body", "hash", source, None, None)
        .await
        .unwrap();
    let rows = db::query("SELECT title,typeof(title) AS storage_type,restored_from,source_event_id,content FROM oc_versions WHERE tenant_id=? AND workspace_id=? AND asset_id=? ORDER BY version")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(asset)
        .fetch_all(&mut *tx.tx)
        .await
        .unwrap();
    assert_eq!(rows.len(), 6);
    for (index, expected) in [(0, "inherited"), (1, ""), (2, "explicit"), (4, ""), (5, "42")] {
        assert_eq!(rows[index].get::<String>("title"), expected);
        assert_eq!(rows[index].get::<String>("storage_type"), "text");
    }
    assert_eq!(rows[3].get::<Vec<u8>>("title"), vec![0, 255, 65]);
    assert_eq!(rows[3].get::<String>("storage_type"), "blob");
    assert_eq!(rows[2].get::<Option<i32>>("restored_from"), Some(1));
    for row in rows {
        assert_eq!(row.get::<Uuid>("source_event_id"), source);
        assert_eq!(row.get::<String>("content"), "body");
    }
    tx.tx.rollback().await.unwrap();
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn version_insert_retains_native_missing_row_constraints() {
    let (_dir, store, scope) = database().await;
    let asset = Uuid::new_v4();
    let source = Uuid::new_v4();
    let missing = Uuid::new_v4();
    let mut seed = transaction(&store.pool, scope).await;
    seed_root(&mut seed.tx, scope, asset, source, "title").await;
    seed.tx.commit().await.unwrap();
    for (asset, source, version, title) in [
        (missing, source, 1, None),
        (missing, source, 1, Some("")),
        (asset, missing, 1, Some("explicit")),
        (asset, source, 0, None),
    ] {
        let mut tx = transaction(&store.pool, scope).await;
        assert!(matches!(
            tx.insert_version_inner(asset, version, "body", "hash", source, None, title)
                .await,
            Err(StorageError::Backend(_))
        ));
        tx.tx.rollback().await.unwrap();
    }
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM oc_versions")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn explicit_title_still_prepares_the_asset_table() {
    let dir = tempfile::tempdir().unwrap();
    let pool = Pool::open(dir.path().join("missing-assets.db")).await.unwrap();
    db::query("CREATE TABLE oc_versions(tenant_id BLOB,workspace_id BLOB,asset_id BLOB,version INTEGER,content TEXT,content_hash TEXT,source_event_id BLOB,restored_from INTEGER,title TEXT NOT NULL,created_by BLOB,created_at INTEGER)")
        .execute(&pool).await.unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    let mut tx = transaction(&pool, scope).await;
    assert!(matches!(
        tx.insert_version_inner(
            Uuid::new_v4(),
            1,
            "body",
            "hash",
            Uuid::new_v4(),
            None,
            Some("explicit"),
        )
        .await,
        Err(StorageError::Backend(_))
    ));
    tx.tx.rollback().await.unwrap();
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM oc_versions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    db::query("CREATE TABLE oc_assets(tenant_id BLOB,workspace_id BLOB,id BLOB)")
        .execute(&pool)
        .await
        .unwrap();
    let mut tx = transaction(&pool, scope).await;
    // The table exists, but the original title-column reference is invalid.
    assert!(matches!(
        tx.insert_version_inner(
            Uuid::new_v4(),
            1,
            "body",
            "hash",
            Uuid::new_v4(),
            None,
            Some(""),
        )
        .await,
        Err(StorageError::Backend(_))
    ));
    tx.tx.rollback().await.unwrap();
    // This relaxed fixture can represent a damaged NULL asset title. Explicit
    // strings still win; missing input reaches the version's native NOT NULL.
    db::query("ALTER TABLE oc_assets ADD COLUMN title TEXT")
        .execute(&pool)
        .await
        .unwrap();
    let asset = Uuid::new_v4();
    db::query("INSERT INTO oc_assets VALUES(?,?,?,NULL)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(asset)
        .execute(&pool)
        .await
        .unwrap();
    let mut tx = transaction(&pool, scope).await;
    for (version, title) in [(1, ""), (2, "explicit")] {
        tx.insert_version_inner(asset, version, "body", "hash", asset, None, Some(title))
            .await
            .unwrap();
    }
    tx.tx.commit().await.unwrap();
    let mut tx = transaction(&pool, scope).await;
    assert!(matches!(
        tx.insert_version_inner(asset, 3, "body", "hash", asset, None, None)
            .await,
        Err(StorageError::Backend(_))
    ));
    tx.tx.rollback().await.unwrap();
    let titles: Vec<String> = db::query_scalar("SELECT title FROM oc_versions ORDER BY version")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(titles, vec!["", "explicit"]);
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn cleanup_keeps_exact_history_scope_counts_and_rollback() {
    let (_dir, store, scope) = database().await;
    let foreign = Scope {
        tenant_id: scope.tenant_id,
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(foreign.tenant_id, foreign.workspace_id, "foreign")
        .await
        .unwrap();
    let asset = Uuid::from_u128(1);
    let old_source = Uuid::from_u128(2);
    let new_source = Uuid::from_u128(3);
    let old_chunk = Uuid::from_u128(4);
    let new_chunk = Uuid::from_u128(5);
    let mut tx = transaction(&store.pool, scope).await;
    seed_root(&mut tx.tx, scope, asset, old_source, "history").await;
    seed_root(&mut tx.tx, foreign, asset, old_source, "foreign").await;
    db::query("INSERT INTO oc_events(tenant_id,workspace_id,id,content,kind,state,created_by,created_at) VALUES(?,?,?,'immutable new source','knowledge','retracted',?,0)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(new_source)
        .bind(Uuid::nil())
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    for (version, source, id) in [(1, old_source, old_chunk), (2, new_source, new_chunk)] {
        tx.insert_version_inner(asset, version, "immutable version", "hash", source, None, None)
            .await
            .unwrap();
        chunk(&mut tx, asset, version, id).await;
    }
    // More than two association and mutation batches. Every chunk matches
    // both OR branches, but its physical row is removed only once.
    for index in 0..17 {
        let dead_asset = Uuid::from_u128(100 + index);
        let dead_source = Uuid::from_u128(200 + index);
        seed_root(&mut tx.tx, scope, dead_asset, dead_source, "dead").await;
        tx.insert_version_inner(dead_asset, 1, "immutable version", "hash", dead_source, None, None)
            .await
            .unwrap();
        chunk(&mut tx, dead_asset, 1, Uuid::from_u128(300 + index)).await;
    }
    db::query("UPDATE oc_assets SET deleted=2 WHERE tenant_id=? AND workspace_id=? AND id<>?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(asset)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_events SET state='retracted' WHERE tenant_id=? AND workspace_id=? AND id<>?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(old_source)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    tx.tx.commit().await.unwrap();
    let mut other = transaction(&store.pool, foreign).await;
    other
        .insert_version_inner(asset, 1, "foreign version", "hash", old_source, None, None)
        .await
        .unwrap();
    chunk(&mut other, asset, 1, new_chunk).await;
    other.tx.commit().await.unwrap();

    let mut tx = transaction(&store.pool, scope).await;
    assert_eq!(tx.cleanup_derived_chunks_inner().await.unwrap(), 18);
    assert_eq!(tx.cleanup_derived_chunks_inner().await.unwrap(), 0);
    let retained: Vec<Uuid> = db::query_scalar("SELECT id FROM oc_chunks WHERE tenant_id=? AND workspace_id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .fetch_all(&mut *tx.tx)
        .await
        .unwrap();
    assert_eq!(retained, vec![old_chunk]);
    // A write in this same transaction changes the next cleanup result.
    db::query("UPDATE oc_events SET state='retracted' WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(old_source)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert_eq!(tx.cleanup_derived_chunks_inner().await.unwrap(), 1);
    tx.tx.rollback().await.unwrap();
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM oc_chunks WHERE tenant_id=? AND workspace_id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(count, 19);
    let mut tx = transaction(&store.pool, scope).await;
    assert_eq!(tx.cleanup_derived_chunks_inner().await.unwrap(), 18);
    tx.tx.commit().await.unwrap();
    for sql in [
        "SELECT COUNT(*) FROM oc_events WHERE tenant_id=? AND workspace_id=?",
        "SELECT COUNT(*) FROM oc_versions WHERE tenant_id=? AND workspace_id=?",
    ] {
        let count: i64 = db::query_scalar(sql)
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(count, 19);
    }
    let foreign_chunks: Vec<Uuid> = db::query_scalar("SELECT id FROM oc_chunks WHERE tenant_id=? AND workspace_id=?")
        .bind(foreign.tenant_id)
        .bind(foreign.workspace_id)
        .fetch_all(&store.pool)
        .await
        .unwrap();
    assert_eq!(foreign_chunks, vec![new_chunk]);
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn cleanup_preserves_native_raw_matches_and_missing_associations() {
    let dir = tempfile::tempdir().unwrap();
    let pool = Pool::open(dir.path().join("raw-cleanup.db")).await.unwrap();
    // Relax only this fixture's foreign keys/nullability to represent missing
    // relations and raw stored keys. No current-version, locator or Learning
    // columns/tables are supplied: those are not this cleanup's predicates.
    for sql in [
        "CREATE TABLE oc_assets(tenant_id BLOB,workspace_id BLOB,id BLOB,deleted INTEGER)",
        "CREATE TABLE oc_versions(tenant_id BLOB,workspace_id BLOB,asset_id BLOB,version INTEGER,source_event_id BLOB)",
        "CREATE TABLE oc_events(tenant_id BLOB,workspace_id BLOB,id BLOB,state TEXT)",
        "CREATE TABLE oc_chunks(tenant_id BLOB,workspace_id BLOB,id BLOB,asset_id BLOB,version INTEGER,marker TEXT)",
    ] {
        db::query(sql).execute(&pool).await.unwrap();
    }
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    let mut tx = transaction(&pool, scope).await;
    for sql in [
        "INSERT INTO oc_assets VALUES(?,?,'deleted',2)",
        "INSERT INTO oc_assets VALUES(?,?,'missing-version',0)",
        "INSERT INTO oc_assets VALUES(?,?,'missing-event',0)",
        "INSERT INTO oc_assets VALUES(?,?,'typed',2)",
        "INSERT INTO oc_assets VALUES(?,?,NULL,2)",
        "INSERT INTO oc_events VALUES(?,?,'gone','retracted')",
        "INSERT INTO oc_events VALUES(?,?,'live','active')",
        "INSERT INTO oc_events VALUES(?,?,x'01','retracted')",
        "INSERT INTO oc_events VALUES(?,?,'nontext',x'726574726163746564')",
        "INSERT INTO oc_versions VALUES(?,?,'retracted-no-asset',1,'gone')",
        "INSERT INTO oc_versions VALUES(?,?,'missing-asset',1,'live')",
        "INSERT INTO oc_versions VALUES(?,?,'missing-event',1,'absent')",
        "INSERT INTO oc_versions VALUES(?,?,'wrong-version',2,'gone')",
        "INSERT INTO oc_versions VALUES(?,?,'raw',2,x'01')",
        "INSERT INTO oc_versions VALUES(?,?,'typed',1,'gone')",
        "INSERT INTO oc_versions VALUES(?,?,'nontext-state',1,'nontext')",
        "INSERT INTO oc_chunks VALUES(?,?,'not-uuid','deleted',1,'deleted')",
        "INSERT INTO oc_chunks VALUES(?,?,NULL,'deleted',NULL,'deleted-null-id')",
        "INSERT INTO oc_chunks VALUES(?,?,'a','retracted-no-asset',1,'retracted-no-asset')",
        "INSERT INTO oc_chunks VALUES(?,?,'b','missing-asset',1,'missing-asset')",
        "INSERT INTO oc_chunks VALUES(?,?,'c','missing-version',1,'missing-version')",
        "INSERT INTO oc_chunks VALUES(?,?,'d','missing-event',1,'missing-event')",
        "INSERT INTO oc_chunks VALUES(?,?,'e','wrong-version',1,'wrong-version')",
        "INSERT INTO oc_chunks VALUES(?,?,'f',NULL,NULL,'null-key')",
        "INSERT INTO oc_chunks VALUES(?,?,x'01','raw','002','raw')",
        "INSERT INTO oc_chunks VALUES(?,?,'g',x'7479706564',1,'blob-vs-text')",
        "INSERT INTO oc_chunks VALUES(?,?,'h','nontext-state',1,'nontext-state')",
        "INSERT INTO oc_chunks VALUES(?,?,'i','foreign-only',1,'foreign-only')",
    ] {
        db::query(sql)
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
    }
    let foreign = Uuid::new_v4();
    for sql in [
        "INSERT INTO oc_assets VALUES(?,?,'foreign-only',2)",
        "INSERT INTO oc_versions VALUES(?,?,'foreign-only',1,'gone')",
        "INSERT INTO oc_events VALUES(?,?,'gone','retracted')",
    ] {
        db::query(sql)
            .bind(scope.tenant_id)
            .bind(foreign)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
    }
    assert_eq!(tx.cleanup_derived_chunks_inner().await.unwrap(), 4);
    assert_eq!(tx.cleanup_derived_chunks_inner().await.unwrap(), 0);
    let remaining: Vec<String> = db::query_scalar("SELECT marker FROM oc_chunks WHERE tenant_id=? AND workspace_id=? ORDER BY marker")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .fetch_all(&mut *tx.tx)
        .await
        .unwrap();
    assert_eq!(
        remaining,
        vec![
            "blob-vs-text",
            "foreign-only",
            "missing-asset",
            "missing-event",
            "missing-version",
            "nontext-state",
            "null-key",
            "wrong-version",
        ]
    );
    tx.tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn cleanup_empty_input_still_prepares_all_relations() {
    for missing in ["oc_assets", "oc_versions", "oc_events"] {
        let dir = tempfile::tempdir().unwrap();
        let pool = Pool::open(dir.path().join("missing.db")).await.unwrap();
        for (table, sql) in [
            ("oc_assets", "CREATE TABLE oc_assets(tenant_id BLOB,workspace_id BLOB,id BLOB,deleted INTEGER)"),
            ("oc_versions", "CREATE TABLE oc_versions(tenant_id BLOB,workspace_id BLOB,asset_id BLOB,version INTEGER,source_event_id BLOB)"),
            ("oc_events", "CREATE TABLE oc_events(tenant_id BLOB,workspace_id BLOB,id BLOB,state TEXT)"),
            ("oc_chunks", "CREATE TABLE oc_chunks(tenant_id BLOB,workspace_id BLOB,asset_id BLOB,version INTEGER)"),
        ] {
            if table != missing {
                db::query(sql).execute(&pool).await.unwrap();
            }
        }
        let scope = Scope {
            tenant_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
        };
        let mut tx = transaction(&pool, scope).await;
        assert!(matches!(
            tx.cleanup_derived_chunks_inner().await,
            Err(StorageError::Backend(_))
        ));
        tx.tx.rollback().await.unwrap();
        pool.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn cleanup_does_not_bypass_summary_foreign_keys() {
    let (_dir, store, scope) = database().await;
    let asset = Uuid::new_v4();
    let source = Uuid::new_v4();
    let id = Uuid::new_v4();
    let mut tx = transaction(&store.pool, scope).await;
    seed_root(&mut tx.tx, scope, asset, source, "title").await;
    tx.insert_version_inner(asset, 1, "version", "hash", source, None, None)
        .await
        .unwrap();
    chunk(&mut tx, asset, 1, id).await;
    tx.insert_summary_inner(Uuid::new_v4(), id, "summary", "fixture", "summary")
        .await
        .unwrap();
    db::query("UPDATE oc_assets SET deleted=1 WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(asset)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    tx.tx.commit().await.unwrap();
    let mut tx = transaction(&store.pool, scope).await;
    assert!(matches!(
        tx.cleanup_derived_chunks_inner().await,
        Err(StorageError::Backend(_))
    ));
    tx.tx.rollback().await.unwrap();
    for sql in [
        "SELECT COUNT(*) FROM oc_chunks",
        "SELECT COUNT(*) FROM oc_summaries",
    ] {
        let count: i64 = db::query_scalar(sql)
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }
    store.shutdown().await.unwrap();
}
