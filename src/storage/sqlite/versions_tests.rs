//! Native title inheritance and historical restore contracts.
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
    db::query(
        "UPDATE oc_events SET state='retracted' WHERE tenant_id=? AND workspace_id=? AND id=?",
    )
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
    for (index, expected) in [
        (0, "inherited"),
        (1, ""),
        (2, "explicit"),
        (4, ""),
        (5, "42"),
    ] {
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
    let pool = Pool::open(dir.path().join("missing-assets.db"))
        .await
        .unwrap();
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
async fn restore_filters_retracted_roots_before_decoding_selected_fields() {
    let (_dir, store, scope) = database().await;
    let mut tx = transaction(&store.pool, scope).await;
    let asset = Uuid::new_v4();
    let source = Uuid::new_v4();
    seed_root(&mut tx.tx, scope, asset, source, "title").await;
    tx.insert_version_inner(asset, 1, "body", "hash", source, None, Some("title"))
        .await
        .unwrap();
    db::query("UPDATE oc_assets SET current_version=x'01' WHERE id=?")
        .bind(asset)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_versions SET title=x'02' WHERE asset_id=?")
        .bind(asset)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(matches!(
        tx.restore_source_inner(asset, 1).await,
        Err(StorageError::Backend(_))
    ));
    db::query("UPDATE oc_events SET state='retracted' WHERE id=?")
        .bind(source)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(matches!(
        tx.restore_source_inner(asset, 1).await,
        Err(StorageError::NotFound)
    ));
    db::query("UPDATE oc_events SET state='active' WHERE id=?")
        .bind(source)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_assets SET current_version=NULL WHERE id=?")
        .bind(asset)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(matches!(
        tx.restore_source_inner(asset, 1).await,
        Err(StorageError::Backend(_))
    ));
    tx.tx.rollback().await.unwrap();
    store.shutdown().await.unwrap();
}
