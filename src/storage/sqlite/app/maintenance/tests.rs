use super::*;
use crate::storage::native_turso as db;
use crate::storage::RelationalStore;

#[derive(Clone, Copy)]
struct Record {
    asset: Uuid,
    source: Uuid,
    chunk: Uuid,
    version: i32,
}

impl Record {
    fn new(index: u128) -> Self {
        Self {
            asset: Uuid::from_u128(1000 + index),
            source: Uuid::from_u128(2000 + index),
            chunk: Uuid::from_u128(3000 + index),
            version: 1,
        }
    }
}

async fn database() -> (tempfile::TempDir, TursoStore, Scope) {
    let dir = tempfile::tempdir().unwrap();
    let store = TursoStore::open(dir.path().join("maintenance.db"))
        .await
        .unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "maintenance")
        .await
        .unwrap();
    (dir, store, scope)
}

async fn seed(conn: &mut Connection, scope: Scope, row: Record) {
    db::query("INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,current_version,created_at) VALUES(?,?,?,'knowledge','asset',2,0) ON CONFLICT DO NOTHING")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(row.asset)
        .execute(&mut *conn)
        .await
        .unwrap();
    db::query("INSERT INTO oc_events(tenant_id,workspace_id,id,content,kind,state,created_by,created_at) VALUES(?,?,?,'immutable source','knowledge','active',?,0) ON CONFLICT DO NOTHING")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(row.source)
        .bind(Uuid::nil())
        .execute(&mut *conn)
        .await
        .unwrap();
    db::query("INSERT INTO oc_versions(tenant_id,workspace_id,asset_id,version,content,content_hash,source_event_id,title,created_by,created_at) VALUES(?,?,?,?,'immutable version','hash',?,'title',?,0)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(row.asset)
        .bind(row.version)
        .bind(row.source)
        .bind(Uuid::nil())
        .execute(&mut *conn)
        .await
        .unwrap();
    db::query("INSERT INTO oc_chunks(tenant_id,workspace_id,id,asset_id,version,ordinal,content,locator,search_terms) VALUES(?,?,?,?,?,0,'text','{}','text')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(row.chunk)
        .bind(row.asset)
        .bind(row.version)
        .execute(&mut *conn)
        .await
        .unwrap();
}

async fn owner(
    conn: &mut Connection,
    scope: Scope,
    row: Record,
    kind: &str,
    artifact: Uuid,
    chunk: Option<Uuid>,
) {
    db::query("INSERT INTO oc_artifact_owners(tenant_id,workspace_id,source_id,version,artifact_type,artifact_id,chunk_id,created_at) VALUES(?,?,?,?,?,?,?,0)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(row.source)
        .bind(row.version)
        .bind(kind)
        .bind(artifact)
        .bind(chunk)
        .execute(&mut *conn)
        .await
        .unwrap();
}

async fn ledger(
    conn: &mut Connection,
    scope: Scope,
    row: Record,
    artifact: Uuid,
    surface: &str,
    generation: i64,
) {
    db::query("INSERT INTO oc_artifact_ledger(tenant_id,workspace_id,source_id,version,artifact_type,artifact_id,surface,generation,idempotency_key,state,created_at,updated_at) VALUES(?,?,?,?,'chunk',?,?,?,'test','committed',0,0)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(row.source)
        .bind(row.version)
        .bind(artifact)
        .bind(surface)
        .bind(generation)
        .execute(&mut *conn)
        .await
        .unwrap();
}

async fn index(conn: &mut Connection, scope: Scope, artifact: Uuid) {
    db::query("INSERT INTO oc_index_entries(tenant_id,workspace_id,artifact_id,field,model_id,dimension,generation,state,created_at) VALUES(?,?,?,'content','profile',2,9,'ready',0)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(artifact)
        .execute(&mut *conn)
        .await
        .unwrap();
}

#[tokio::test]
async fn maintenance_preserves_historical_shared_owners_across_reopen_and_finishes_fk_order() {
    let (dir, store, scope) = database().await;
    let old = Record::new(1);
    let new = Record {
        asset: old.asset,
        version: 2,
        ..Record::new(2)
    };
    let shared = Uuid::new_v4();
    let sole = Uuid::new_v4();
    let mut tx = store.pool.begin().await.unwrap();
    seed(&mut tx, scope, old).await;
    seed(&mut tx, scope, new).await;
    for row in [old, new] {
        owner(&mut tx, scope, row, "chunk", row.chunk, Some(row.chunk)).await;
        owner(&mut tx, scope, row, "entity", shared, None).await;
        ledger(&mut tx, scope, row, row.chunk, "vector", 9).await;
        ledger(&mut tx, scope, row, row.chunk, "full_text", 9).await;
        index(&mut tx, scope, row.chunk).await;
    }
    owner(&mut tx, scope, old, "relation", sole, None).await;
    db::query("INSERT INTO oc_summaries(tenant_id,workspace_id,id,chunk_id,text,search_terms) VALUES(?,?,?,?,'summary','summary')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(Uuid::new_v4())
        .bind(old.chunk)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (native, entities, relations) = store.cleanup_plan(scope).await.unwrap();
    assert!(native.is_empty());
    assert_eq!(entities, vec![shared]);
    assert_eq!(relations, vec![sole]);
    db::query("UPDATE oc_events SET state='retracted' WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(old.source)
        .execute(&store.pool)
        .await
        .unwrap();
    let plan = store.cleanup_plan(scope).await.unwrap();
    assert_eq!(plan, (vec![old.chunk], vec![shared], Vec::new()));
    store.shutdown().await.unwrap();
    drop(store);
    // The first authority phase committed. Native deletion can be retried
    // before a separate finish transaction; immutable source rows remain.
    let store = TursoStore::open(dir.path().join("maintenance.db"))
        .await
        .unwrap();
    assert_eq!(store.cleanup_plan(scope).await.unwrap(), plan);
    store.finish_cleanup(scope).await.unwrap();
    store.finish_cleanup(scope).await.unwrap();
    let chunks: Vec<Uuid> = db::query_scalar("SELECT id FROM oc_chunks WHERE tenant_id=? AND workspace_id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .fetch_all(&store.pool)
        .await
        .unwrap();
    assert_eq!(chunks, vec![new.chunk]);
    let summaries: i64 = db::query_scalar("SELECT count(*) FROM oc_summaries WHERE tenant_id=? AND workspace_id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(summaries, 0);
    let versions: i64 = db::query_scalar("SELECT count(*) FROM oc_versions WHERE tenant_id=? AND workspace_id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(versions, 2);
    let source: String = db::query_scalar("SELECT content FROM oc_events WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(old.source)
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(source, "immutable source");
    let states: Vec<(Uuid, String)> = db::query_as("SELECT artifact_id,state FROM oc_index_entries WHERE tenant_id=? AND workspace_id=? ORDER BY artifact_id")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .fetch_all(&store.pool)
        .await
        .unwrap();
    assert_eq!(states, vec![(old.chunk, "removed".into()), (new.chunk, "ready".into())]);
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn maintenance_keeps_loose_chunk_gate_and_cross_type_ledger_owner_match() {
    let (_dir, store, scope) = database().await;
    let root = Record::new(10);
    let chosen = Record {
        version: 7,
        ..Record::new(11)
    };
    let artifact = Uuid::new_v4();
    let absent = Uuid::new_v4();
    let mut tx = store.pool.begin().await.unwrap();
    seed(&mut tx, scope, root).await;
    seed(&mut tx, scope, chosen).await;
    // Different artifact, source, version and asset are allowed by the old
    // owner.chunk_id existence gate; adding any equality here is a regression.
    owner(&mut tx, scope, root, "chunk", artifact, Some(chosen.chunk)).await;
    owner(&mut tx, scope, root, "summary", absent, None).await;
    owner(&mut tx, scope, root, "entity", chosen.chunk, None).await;
    ledger(&mut tx, scope, root, artifact, "full_text", 4).await;
    ledger(&mut tx, scope, root, chosen.chunk, "vector", 5).await;
    ledger(&mut tx, scope, chosen, chosen.chunk, "graph", 6).await;
    index(&mut tx, scope, chosen.chunk).await;
    tx.commit().await.unwrap();
    let (native, entities, _) = store.cleanup_plan(scope).await.unwrap();
    assert_eq!(native, vec![chosen.chunk]);
    assert_eq!(entities, vec![chosen.chunk]);
    let retained: Vec<Uuid> = db::query_scalar("SELECT artifact_id FROM oc_artifact_owners WHERE tenant_id=? AND workspace_id=? AND artifact_type='chunk'")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .fetch_all(&store.pool)
        .await
        .unwrap();
    assert_eq!(retained, vec![artifact]);
    store.finish_cleanup(scope).await.unwrap();
    let states: Vec<(i64, String)> = db::query_as("SELECT generation,state FROM oc_artifact_ledger WHERE tenant_id=? AND workspace_id=? ORDER BY generation")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .fetch_all(&store.pool)
        .await
        .unwrap();
    assert_eq!(states, vec![(4, "committed".into()), (5, "committed".into()), (6, "orphan".into())]);
    store.shutdown().await.unwrap();
}

async fn raw_database() -> (tempfile::TempDir, Pool, Scope) {
    let dir = tempfile::tempdir().unwrap();
    let pool = Pool::open(dir.path().join("raw-maintenance.db"))
        .await
        .unwrap();
    for sql in [
        "CREATE TABLE oc_assets(tenant_id BLOB,workspace_id BLOB,id BLOB,deleted INTEGER)",
        "CREATE TABLE oc_versions(tenant_id BLOB,workspace_id BLOB,asset_id BLOB,version INTEGER,source_event_id BLOB)",
        "CREATE TABLE oc_events(tenant_id BLOB,workspace_id BLOB,id BLOB,state TEXT)",
        "CREATE TABLE oc_artifact_owners(tenant_id BLOB,workspace_id BLOB,source_id BLOB,version INTEGER,artifact_type TEXT,artifact_id BLOB,chunk_id BLOB)",
        "CREATE TABLE oc_chunks(tenant_id BLOB,workspace_id BLOB,id BLOB,asset_id BLOB)",
        "CREATE TABLE oc_summaries(tenant_id BLOB,workspace_id BLOB,chunk_id BLOB)",
        "CREATE TABLE oc_index_entries(tenant_id BLOB,workspace_id BLOB,artifact_id BLOB,state TEXT)",
        "CREATE TABLE oc_artifact_ledger(tenant_id BLOB,workspace_id BLOB,source_id BLOB,version INTEGER,artifact_id BLOB,surface TEXT,state TEXT,updated_at INTEGER)",
    ] {
        db::query(sql).execute(&pool).await.unwrap();
    }
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    (dir, pool, scope)
}

#[tokio::test]
async fn raw_historical_matches_defer_artifact_decoding_until_unowned_output() {
    let (_dir, pool, scope) = raw_database().await;
    let mut tx = pool.begin().await.unwrap();
    let raw = vec![0x31_u8];
    for deleted in [1_i64, 0] {
        let asset = vec![deleted as u8];
        db::query("INSERT INTO oc_assets VALUES(?,?,?,?)")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(&asset)
            .bind(deleted)
            .execute(&mut *tx)
            .await
            .unwrap();
        db::query("INSERT INTO oc_versions VALUES(?,?,?,7,?)")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(&asset)
            .bind(&raw)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    db::query("INSERT INTO oc_events VALUES(?,?,?,'active')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(&raw)
        .execute(&mut *tx)
        .await
        .unwrap();
    db::query("INSERT INTO oc_chunks VALUES(?,?,?,?)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(&raw)
        .bind(vec![0_u8])
        .execute(&mut *tx)
        .await
        .unwrap();
    db::query("INSERT INTO oc_artifact_owners VALUES(?,?,?,7,'chunk',?,?)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(&raw)
        .bind(&raw)
        .bind(&raw)
        .execute(&mut *tx)
        .await
        .unwrap();
    db::query("INSERT INTO oc_artifact_ledger VALUES(?,?,?,7,?,'vector','committed',0)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(&raw)
        .bind(&raw)
        .execute(&mut *tx)
        .await
        .unwrap();
    // Any one live historical asset suffices, without strict UUID decoding.
    assert_eq!(detach_invalid_roots(&mut tx, scope).await.unwrap(), 0);
    assert_eq!(detach_invalid_chunks(&mut tx, scope).await.unwrap(), 0);
    assert!(unowned_native_artifacts(&mut tx, scope).await.unwrap().is_empty());
    db::query("UPDATE oc_artifact_owners SET chunk_id=NULL")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(detach_invalid_chunks(&mut tx, scope).await.unwrap(), 1);
    assert!(matches!(
        unowned_native_artifacts(&mut tx, scope).await,
        Err(StorageError::Backend(_))
    ));
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn scoped_rowid_batches_report_actual_counts_repeat_and_rollback() {
    let (_dir, pool, scope) = raw_database().await;
    let other = Scope {
        workspace_id: Uuid::new_v4(),
        ..scope
    };
    let foreign_tenant = Scope {
        tenant_id: Uuid::new_v4(),
        ..scope
    };
    let source = Uuid::new_v4();
    let asset = Uuid::new_v4();
    let mut tx = pool.begin().await.unwrap();
    for here in [other, foreign_tenant] {
        db::query("INSERT INTO oc_assets VALUES(?,?,?,0)")
            .bind(here.tenant_id)
            .bind(here.workspace_id)
            .bind(asset)
            .execute(&mut *tx)
            .await
            .unwrap();
        db::query("INSERT INTO oc_versions VALUES(?,?,?,1,?)")
            .bind(here.tenant_id)
            .bind(here.workspace_id)
            .bind(asset)
            .bind(source)
            .execute(&mut *tx)
            .await
            .unwrap();
        db::query("INSERT INTO oc_events VALUES(?,?,?,'active')")
            .bind(here.tenant_id)
            .bind(here.workspace_id)
            .bind(source)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    for index in 1..=129 {
        for here in [scope, other, foreign_tenant] {
            db::query("INSERT INTO oc_artifact_owners VALUES(?,?,?,1,'entity',?,NULL)")
                .bind(here.tenant_id)
                .bind(here.workspace_id)
                .bind(source)
                .bind(Uuid::from_u128(index))
                .execute(&mut *tx)
                .await
                .unwrap();
        }
    }
    tx.commit().await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    assert_eq!(detach_invalid_roots(&mut tx, scope).await.unwrap(), 129);
    assert_eq!(detach_invalid_roots(&mut tx, scope).await.unwrap(), 0);
    assert_eq!(detach_invalid_roots(&mut tx, other).await.unwrap(), 0);
    assert_eq!(detach_invalid_roots(&mut tx, foreign_tenant).await.unwrap(), 0);
    for here in [other, foreign_tenant] {
        let retained: i64 = db::query_scalar("SELECT count(*) FROM oc_artifact_owners WHERE tenant_id=? AND workspace_id=?")
            .bind(here.tenant_id)
            .bind(here.workspace_id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(retained, 129);
    }
    tx.rollback().await.unwrap();
    let count: i64 = db::query_scalar("SELECT count(*) FROM oc_artifact_owners WHERE tenant_id=? AND workspace_id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 129);
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn finish_helpers_preserve_dangling_summary_and_loose_owner_equivalence() {
    let (_dir, pool, scope) = raw_database().await;
    let mut tx = pool.begin().await.unwrap();
    let source = Uuid::new_v4();
    let chunk = Uuid::new_v4();
    let missing = Uuid::new_v4();
    db::query("INSERT INTO oc_chunks VALUES(?,?,?,?)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(chunk)
        .bind(Uuid::new_v4())
        .execute(&mut *tx)
        .await
        .unwrap();
    for id in [chunk, missing] {
        db::query("INSERT INTO oc_summaries VALUES(?,?,?)")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(id)
            .execute(&mut *tx)
            .await
            .unwrap();
        db::query("INSERT INTO oc_index_entries VALUES(?,?,?,'ready')")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(id)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    db::query("INSERT INTO oc_artifact_owners VALUES(?,?,?,1,'relation',?,NULL)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(source)
        .bind(chunk)
        .execute(&mut *tx)
        .await
        .unwrap();
    for version in [1_i32, 2] {
        db::query("INSERT INTO oc_artifact_ledger VALUES(?,?,?,?,?,'full_text','committed',0)")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(source)
            .bind(version)
            .bind(chunk)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    assert_eq!(remove_unowned_chunks(&mut tx, scope).await.unwrap(), 1);
    assert_eq!(remove_unowned_chunks(&mut tx, scope).await.unwrap(), 0);
    let summaries: Vec<Uuid> = db::query_scalar("SELECT chunk_id FROM oc_summaries")
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    assert_eq!(summaries, vec![missing]);
    assert_eq!(remove_missing_indexes(&mut tx, scope).await.unwrap(), 2);
    assert_eq!(orphan_unowned_ledger(&mut tx, scope).await.unwrap(), 1);
    let states: Vec<String> = db::query_scalar("SELECT state FROM oc_artifact_ledger ORDER BY version")
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    assert_eq!(states, vec!["committed", "orphan"]);
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn empty_cleanup_still_prepares_original_relations() {
    let (_dir, pool, scope) = raw_database().await;
    db::query("DROP TABLE oc_events").execute(&pool).await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    assert!(matches!(
        detach_invalid_roots(&mut tx, scope).await,
        Err(StorageError::Backend(_))
    ));
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn empty_finish_prepares_summary_mutation_and_owner_relation() {
    for (sql, summaries) in [
        ("DROP TABLE oc_summaries", true),
        ("DROP TABLE oc_artifact_owners", false),
    ] {
        let (_dir, pool, scope) = raw_database().await;
        db::query(sql).execute(&pool).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        let result = if summaries {
            remove_unowned_chunks(&mut tx, scope).await
        } else {
            orphan_unowned_ledger(&mut tx, scope).await
        };
        assert!(matches!(result, Err(StorageError::Backend(_))));
        tx.rollback().await.unwrap();
        pool.shutdown().await.unwrap();
    }
}
