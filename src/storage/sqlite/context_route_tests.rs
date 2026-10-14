//! Native-engine fixtures for transaction-local Context route preparation.
//! Each fixture reads one relation at a time and asserts domain outcomes directly.
#![cfg(feature = "local-storage")]

use super::*;
use crate::context_retrieval::ContextRoute;
use crate::memory_identity::MemoryIdentity;
use crate::storage::native_turso as db;
use std::collections::HashSet;

#[derive(Clone, Copy)]
struct Record {
    asset: Uuid,
    chunk: Uuid,
    source: Uuid,
}

impl Record {
    fn new(index: u128) -> Self {
        Self {
            asset: Uuid::from_u128(100_000 + index),
            chunk: Uuid::from_u128(200_000 + index),
            source: Uuid::from_u128(300_000 + index),
        }
    }

    fn second_chunk(self) -> Uuid {
        Uuid::from_u128(self.chunk.as_u128() + 100_000)
    }
}

fn scope() -> Scope {
    Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    }
}

fn identity(name: &str) -> MemoryIdentity {
    serde_json::from_value(json!({
        "subject": {"kind": "service", "stable_id": name},
        "predicate": "release.approval",
        "context": {"environment": "production"}
    }))
    .unwrap()
}

async fn database() -> (tempfile::TempDir, Pool) {
    let dir = tempfile::tempdir().unwrap();
    let pool = Pool::open(dir.path().join("context-routes.db"))
        .await
        .unwrap();
    // No foreign keys: missing relations and malformed historical keys are
    // intentional fixtures for predicate-before-decode behavior.
    for sql in [
        "CREATE TABLE oc_assets(tenant_id BLOB NOT NULL,workspace_id BLOB NOT NULL,id BLOB NOT NULL,kind TEXT NOT NULL,title TEXT NOT NULL,current_version INTEGER,deleted INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(tenant_id,workspace_id,id))",
        "CREATE TABLE oc_versions(tenant_id BLOB NOT NULL,workspace_id BLOB NOT NULL,asset_id BLOB NOT NULL,version INTEGER NOT NULL,title TEXT NOT NULL,content TEXT NOT NULL,content_hash TEXT NOT NULL,source_event_id BLOB NOT NULL,restored_from INTEGER,PRIMARY KEY(tenant_id,workspace_id,asset_id,version))",
        "CREATE TABLE oc_events(tenant_id BLOB NOT NULL,workspace_id BLOB NOT NULL,id BLOB NOT NULL,state TEXT NOT NULL,kind TEXT NOT NULL,PRIMARY KEY(tenant_id,workspace_id,id))",
        "CREATE TABLE oc_chunks(tenant_id BLOB NOT NULL,workspace_id BLOB NOT NULL,id BLOB NOT NULL,asset_id BLOB NOT NULL,version INTEGER NOT NULL,ordinal INTEGER NOT NULL,content TEXT NOT NULL,locator TEXT NOT NULL,search_terms TEXT NOT NULL,PRIMARY KEY(tenant_id,workspace_id,id))",
        "CREATE TABLE oc_learnings(tenant_id BLOB NOT NULL,workspace_id BLOB NOT NULL,id BLOB NOT NULL,asset_id BLOB NOT NULL,source_event_id BLOB NOT NULL,status TEXT NOT NULL,session_id BLOB NOT NULL,improve_id BLOB NOT NULL,PRIMARY KEY(tenant_id,workspace_id,id))",
        "CREATE TABLE oc_improve_runs(tenant_id BLOB NOT NULL,workspace_id BLOB NOT NULL,improve_id BLOB NOT NULL,status TEXT NOT NULL,PRIMARY KEY(tenant_id,workspace_id,improve_id))",
        "CREATE TABLE oc_jobs(tenant_id BLOB NOT NULL,workspace_id BLOB NOT NULL,id BLOB NOT NULL,operation TEXT NOT NULL,PRIMARY KEY(tenant_id,workspace_id,id))",
        "CREATE TABLE oc_memory_identities(tenant_id BLOB NOT NULL,workspace_id BLOB NOT NULL,asset_id BLOB NOT NULL,identity_json TEXT NOT NULL,PRIMARY KEY(tenant_id,workspace_id,asset_id))",
    ] {
        db::query(sql).execute(&pool).await.unwrap();
    }
    (dir, pool)
}

async fn transaction(pool: &Pool, scope: Scope) -> TursoTx {
    TursoTx {
        tx: pool.begin_with("BEGIN IMMEDIATE").await.unwrap(),
        scope,
        principal_id: Uuid::new_v4(),
        required: Vec::new(),
        command: None,
    }
}

async fn seed(
    tx: &mut TursoTx,
    scope: Scope,
    row: Record,
    kind: &str,
    current: Option<i32>,
    root_state: &str,
    root_kind: &str,
) {
    db::query("INSERT INTO oc_assets VALUES(?,?,?,?,'asset',?,0)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(row.asset)
        .bind(kind)
        .bind(current)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("INSERT INTO oc_events VALUES(?,?,?,?,?)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(row.source)
        .bind(root_state)
        .bind(root_kind)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    version(tx, scope, row.asset, 1, row.source).await;
    chunk(tx, scope, row.asset, row.chunk, 1, 0, "{}").await;
}

async fn version(tx: &mut TursoTx, scope: Scope, asset: Uuid, version: i32, source: Uuid) {
    db::query("INSERT INTO oc_versions VALUES(?,?,?,?,'version','content','hash',?,NULL)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(asset)
        .bind(version)
        .bind(source)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
}

async fn chunk(
    tx: &mut TursoTx,
    scope: Scope,
    asset: Uuid,
    id: Uuid,
    version: i32,
    ordinal: i64,
    locator: &str,
) {
    db::query("INSERT INTO oc_chunks VALUES(?,?,?,?,?,?,'content',?,'needle')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(id)
        .bind(asset)
        .bind(version)
        .bind(ordinal)
        .bind(locator)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
}

async fn registration(tx: &mut TursoTx, scope: Scope, row: Record, source: Uuid, status: &str) {
    let improve = Uuid::new_v4();
    db::query("INSERT INTO oc_improve_runs VALUES(?,?,?,'publishing')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(improve)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("INSERT INTO oc_learnings VALUES(?,?,?,?,?,?,?,?)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(Uuid::new_v4())
        .bind(row.asset)
        .bind(source)
        .bind(status)
        .bind(Uuid::new_v4())
        .bind(improve)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
}

async fn binding(tx: &mut TursoTx, scope: Scope, asset: Uuid, identity: &MemoryIdentity) {
    db::query("INSERT INTO oc_memory_identities VALUES(?,?,?,?) ON CONFLICT(tenant_id,workspace_id,asset_id) DO UPDATE SET identity_json=excluded.identity_json")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(asset)
        .bind(serde_json::to_string(identity).unwrap())
        .execute(&mut *tx.tx)
        .await
        .unwrap();
}

async fn set_locator(tx: &mut TursoTx, id: Uuid, locator: &str) {
    db::query("UPDATE oc_chunks SET locator=? WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(locator)
        .bind(tx.scope.tenant_id)
        .bind(tx.scope.workspace_id)
        .bind(id)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
}

fn ids(rows: Vec<(Uuid, String)>) -> HashSet<Uuid> {
    let count = rows.len();
    let result: HashSet<_> = rows
        .into_iter()
        .map(|(id, terms)| {
            assert_eq!(terms, "needle");
            id
        })
        .collect();
    assert_eq!(
        result.len(),
        count,
        "route rows must preserve distinct chunks"
    );
    result
}

#[tokio::test]
async fn route_rows_cross_32_and_128_asset_boundaries_without_losing_chunks() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let wanted = identity("Atlas");
    let mut knowledge = HashSet::new();
    let mut memory = HashSet::new();
    let mut assets = Vec::new();
    for index in 0..130 {
        let row = Record::new(index);
        assets.push(row.asset);
        seed(
            &mut tx,
            scope,
            row,
            "knowledge",
            Some(1),
            "active",
            "knowledge",
        )
        .await;
        chunk(&mut tx, scope, row.asset, row.second_chunk(), 1, 1, "{}").await;
        knowledge.extend([row.chunk, row.second_chunk()]);
        let row = Record::new(1_000 + index);
        assets.push(row.asset);
        seed(&mut tx, scope, row, "memory", Some(1), "active", "memory").await;
        // Stored identity is sufficient even for unnormalized semantic status.
        let locator =
            json!({"parser": crate::memory_semantics::PARSER, "semantic_status": "proposed"})
                .to_string();
        set_locator(&mut tx, row.chunk, &locator).await;
        chunk(
            &mut tx,
            scope,
            row.asset,
            row.second_chunk(),
            1,
            1,
            &locator,
        )
        .await;
        binding(&mut tx, scope, row.asset, &wanted).await;
        memory.extend([row.chunk, row.second_chunk()]);
    }
    // Old malformed locator bytes must never be parsed by the exact-current
    // version batches, including the first and last batch boundary assets.
    for index in [0, 31, 32, 127, 128, 129] {
        let row = Record::new(1_000 + index);
        chunk(&mut tx, scope, row.asset, Uuid::new_v4(), 99, 0, "{").await;
    }
    assert_eq!(
        ids(tx
            .context_route_rows(ContextRoute::Knowledge, None)
            .await
            .unwrap()),
        knowledge
    );
    assert_eq!(
        ids(tx
            .context_route_rows(ContextRoute::Memory, Some(&wanted))
            .await
            .unwrap()),
        memory
    );
    assert!(
        tx.context_route_rows(ContextRoute::Learning, None)
            .await
            .unwrap()
            .is_empty()
    );
    assets.extend([Record::new(0).asset, Record::new(128).asset]);
    let membership = tx
        .context_route_memberships(&assets, ContextRoute::Memory)
        .await
        .unwrap();
    assert_eq!(membership.len(), 260);
    for index in 0..130 {
        assert!(!membership[&Record::new(index).asset]);
        assert!(membership[&Record::new(1_000 + index).asset]);
        let row = Record::new(index);
        registration(&mut tx, scope, row, row.source, "published").await;
    }
    // Re-read after in-transaction catalog writes: no cached ordinary route
    // membership survives, and every published source tuple crosses the batches.
    assert!(
        tx.context_route_rows(ContextRoute::Knowledge, None)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        ids(tx
            .context_route_rows(ContextRoute::Learning, None)
            .await
            .unwrap()),
        knowledge
    );
    assert!(
        tx.context_route_memberships(&[], ContextRoute::Memory)
            .await
            .unwrap()
            .is_empty()
    );
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn routes_keep_registered_and_historical_learning_separate_from_ordinary_knowledge() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let rows: Vec<_> = (0..12).map(Record::new).collect();
    for (index, row) in rows.iter().enumerate() {
        let (root_kind, root_state) = match index {
            1..=4 => ("learning", "active"),
            5 | 9 => ("learning", "retracted"),
            7 => ("knowledge", "retracted"),
            _ => ("knowledge", "active"),
        };
        let current = if index == 11 { None } else { Some(1) };
        seed(
            &mut tx,
            scope,
            *row,
            "knowledge",
            current,
            root_state,
            root_kind,
        )
        .await;
    }
    db::query("UPDATE oc_assets SET deleted=1 WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(rows[10].asset)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    registration(&mut tx, scope, rows[1], rows[1].source, "accepted").await;
    registration(&mut tx, scope, rows[2], rows[2].source, "published").await;
    registration(&mut tx, scope, rows[9], rows[9].source, "published").await;
    // Published catalog membership with a different current root is excluded.
    registration(&mut tx, scope, rows[4], Uuid::new_v4(), "published").await;
    // Retraction cannot reclassify an orphaned historical Learning as Knowledge.
    for index in [3, 5] {
        let source = Uuid::new_v4();
        db::query("INSERT INTO oc_events VALUES(?,?,?,'active','knowledge')")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(source)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        version(&mut tx, scope, rows[index].asset, 2, source).await;
        chunk(
            &mut tx,
            scope,
            rows[index].asset,
            rows[index].second_chunk(),
            2,
            0,
            "{}",
        )
        .await;
        db::query(
            "UPDATE oc_assets SET current_version=2 WHERE tenant_id=? AND workspace_id=? AND id=?",
        )
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(rows[index].asset)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    }
    // A registration in another workspace cannot capture an ordinary asset.
    let foreign = Scope {
        workspace_id: Uuid::new_v4(),
        ..scope
    };
    registration(&mut tx, foreign, rows[6], rows[6].source, "published").await;
    // A same-id foreign Learning event cannot classify this scope's source.
    db::query("INSERT INTO oc_events VALUES(?,?,?,'active','learning')")
        .bind(foreign.tenant_id)
        .bind(foreign.workspace_id)
        .bind(rows[8].source)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(
            tx.context_route_matches(row.asset, ContextRoute::Knowledge)
                .await
                .unwrap(),
            matches!(index, 0 | 6 | 7 | 8),
            "Knowledge {index}"
        );
        assert_eq!(
            tx.context_route_matches(row.asset, ContextRoute::Learning)
                .await
                .unwrap(),
            matches!(index, 2 | 9),
            "Learning {index}"
        );
        assert!(
            !tx.context_route_matches(row.asset, ContextRoute::Memory)
                .await
                .unwrap()
        );
    }
    assert_eq!(
        ids(tx
            .context_route_rows(ContextRoute::Knowledge, None)
            .await
            .unwrap()),
        HashSet::from([rows[0].chunk, rows[6].chunk, rows[8].chunk])
    );
    assert_eq!(
        ids(tx
            .context_route_rows(ContextRoute::Learning, None)
            .await
            .unwrap()),
        HashSet::from([rows[2].chunk])
    );
    assert!(
        !tx.context_route_matches(Uuid::new_v4(), ContextRoute::Knowledge)
            .await
            .unwrap()
    );
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn identity_fast_path_skips_locator_json_only_when_identity_filter_does_not_need_it() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let memory = Record::new(0);
    let knowledge = Record::new(1);
    let wanted = identity("Atlas");
    seed(
        &mut tx,
        scope,
        memory,
        "memory",
        Some(1),
        "active",
        "memory",
    )
    .await;
    seed(
        &mut tx,
        scope,
        knowledge,
        "knowledge",
        Some(1),
        "active",
        "knowledge",
    )
    .await;
    set_locator(&mut tx, memory.chunk, "{").await;
    set_locator(&mut tx, knowledge.chunk, "{").await;
    binding(&mut tx, scope, memory.asset, &wanted).await;
    assert_eq!(
        ids(tx
            .context_route_rows(ContextRoute::Memory, Some(&wanted))
            .await
            .unwrap()),
        HashSet::from([memory.chunk])
    );
    assert_eq!(
        ids(tx
            .context_route_rows(ContextRoute::Memory, None)
            .await
            .unwrap()),
        HashSet::from([memory.chunk])
    );
    assert_eq!(
        ids(tx
            .context_route_rows(ContextRoute::Knowledge, Some(&wanted))
            .await
            .unwrap()),
        HashSet::from([knowledge.chunk])
    );
    // The raw identity read must not run a semantic_status or JSON validity guard.
    set_locator(
        &mut tx,
        memory.chunk,
        &json!({"parser": crate::memory_semantics::PARSER, "semantic_status": 7}).to_string(),
    )
    .await;
    assert_eq!(
        ids(tx
            .context_route_rows(ContextRoute::Memory, Some(&wanted))
            .await
            .unwrap()),
        HashSet::from([memory.chunk])
    );
    set_locator(&mut tx, memory.chunk, "{").await;
    db::query(
        "DELETE FROM oc_memory_identities WHERE tenant_id=? AND workspace_id=? AND asset_id=?",
    )
    .bind(scope.tenant_id)
    .bind(scope.workspace_id)
    .bind(memory.asset)
    .execute(&mut *tx.tx)
    .await
    .unwrap();
    assert!(matches!(
        tx.context_route_rows(ContextRoute::Memory, Some(&wanted))
            .await,
        Err(StorageError::Unavailable(_))
    ));
    binding(&mut tx, scope, memory.asset, &identity("Other")).await;
    assert!(matches!(
        tx.context_route_rows(ContextRoute::Memory, Some(&wanted))
            .await,
        Err(StorageError::Unavailable(_))
    ));
    let conflict = json!({"parser": crate::memory_semantics::PARSER, "semantic_status": "conflict", "suggested_identity": wanted});
    set_locator(&mut tx, memory.chunk, &conflict.to_string()).await;
    assert_eq!(
        ids(tx
            .context_route_rows(ContextRoute::Memory, Some(&wanted))
            .await
            .unwrap()),
        HashSet::from([memory.chunk])
    );
    for status in ["confirmed", "proposed", "ambiguous", "negated"] {
        let mut locator = conflict.clone();
        locator["semantic_status"] = json!(status);
        set_locator(&mut tx, memory.chunk, &locator.to_string()).await;
        assert!(
            tx.context_route_rows(ContextRoute::Memory, Some(&wanted))
                .await
                .unwrap()
                .is_empty()
        );
    }
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn root_kind_and_current_filters_exclude_bad_payloads_before_decoding() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let good = Record::new(0);
    seed(
        &mut tx,
        scope,
        good,
        "knowledge",
        Some(1),
        "active",
        "knowledge",
    )
    .await;
    for index in 1..=6 {
        let row = Record::new(index);
        let kind = if index == 3 { "memory" } else { "knowledge" };
        let current = if index == 2 { Some(2) } else { Some(1) };
        let state = if index == 1 { "retracted" } else { "active" };
        seed(&mut tx, scope, row, kind, current, state, kind).await;
        if index == 4 {
            db::query(
                "UPDATE oc_assets SET deleted=1 WHERE tenant_id=? AND workspace_id=? AND id=?",
            )
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(row.asset)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        }
        if index == 5 {
            db::query(
                "DELETE FROM oc_versions WHERE tenant_id=? AND workspace_id=? AND asset_id=?",
            )
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(row.asset)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        }
        if index == 6 {
            db::query("DELETE FROM oc_events WHERE tenant_id=? AND workspace_id=? AND id=?")
                .bind(scope.tenant_id)
                .bind(scope.workspace_id)
                .bind(row.source)
                .execute(&mut *tx.tx)
                .await
                .unwrap();
        }
        db::query("UPDATE oc_chunks SET id=?,locator=x'01',search_terms=x'02' WHERE tenant_id=? AND workspace_id=? AND id=?")
            .bind(vec![index as u8]).bind(scope.tenant_id).bind(scope.workspace_id).bind(row.chunk)
            .execute(&mut *tx.tx).await.unwrap();
    }
    assert_eq!(
        ids(tx
            .context_route_rows(ContextRoute::Knowledge, None)
            .await
            .unwrap()),
        HashSet::from([good.chunk])
    );
    // Once the same malformed fields belong to a matching tuple, decoding is
    // required and must fail rather than silently dropping the candidate.
    db::query("UPDATE oc_chunks SET locator=x'01' WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(good.chunk)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(matches!(
        tx.context_route_rows(ContextRoute::Knowledge, None).await,
        Err(StorageError::Backend(_))
    ));
    db::query("UPDATE oc_chunks SET locator='{}',search_terms=x'02' WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(good.chunk)
        .execute(&mut *tx.tx).await.unwrap();
    assert!(matches!(
        tx.context_route_rows(ContextRoute::Knowledge, None).await,
        Err(StorageError::Backend(_))
    ));
    db::query("UPDATE oc_chunks SET search_terms='needle',id=x'ff' WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(good.chunk)
        .execute(&mut *tx.tx).await.unwrap();
    assert!(matches!(
        tx.context_route_rows(ContextRoute::Knowledge, None).await,
        Err(StorageError::Backend(_))
    ));
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn raw_identity_batches_do_not_add_current_or_semantic_guards() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let wanted = identity("Atlas");
    let body = serde_json::to_string(&wanted).unwrap();
    let records: Vec<_> = (0..130).map(Record::new).collect();
    for (index, row) in records.iter().enumerate() {
        let current = if index % 2 == 0 { None } else { Some(2) };
        seed(
            &mut tx,
            scope,
            *row,
            "memory",
            current,
            "retracted",
            "memory",
        )
        .await;
        set_locator(&mut tx, row.chunk, "{").await;
        binding(&mut tx, scope, row.asset, &wanted).await;
    }
    let foreign = Scope {
        tenant_id: Uuid::new_v4(),
        ..scope
    };
    binding(&mut tx, foreign, records[128].asset, &identity("Other")).await;
    let missing = Uuid::new_v4();
    let mut assets: Vec<_> = records.iter().map(|row| row.asset).collect();
    assets.extend([records[0].asset, records[128].asset, missing]);
    let found = tx.identity_rows_for_assets(&assets).await.unwrap();
    assert_eq!(found.len(), 130);
    assert!(!found.contains_key(&missing));
    for row in records {
        assert_eq!(
            found[&row.asset]
                .try_get::<String>("identity_json")
                .unwrap(),
            body
        );
    }
    assert!(tx.identity_rows_for_assets(&[]).await.unwrap().is_empty());
    db::query("DROP TABLE oc_memory_identities")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(
        tx.identity_rows_for_assets(&assets)
            .await
            .unwrap()
            .is_empty()
    );
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn improve_availability_preserves_each_independent_signal() {
    // Connection construction is intentionally private. Isolated temporary
    // pools provide the same native engine without changing production schema.
    for signal in 0..=6 {
        let dir = tempfile::tempdir().unwrap();
        let pool = Pool::open(dir.path().join("availability.db"))
            .await
            .unwrap();
        let mut conn = pool.acquire().await.unwrap();
        db::query("CREATE TABLE oc_events(kind TEXT NOT NULL)")
            .execute(&mut *conn)
            .await
            .unwrap();
        db::query("CREATE TABLE oc_jobs(operation TEXT NOT NULL)")
            .execute(&mut *conn)
            .await
            .unwrap();
        db::query("INSERT INTO oc_events VALUES('knowledge')")
            .execute(&mut *conn)
            .await
            .unwrap();
        db::query("INSERT INTO oc_jobs VALUES('ingest')")
            .execute(&mut *conn)
            .await
            .unwrap();
        match signal {
            0 => {}
            1 => {
                db::query("CREATE TABLE oc_improve_cursors(id INTEGER)")
                    .execute(&mut *conn)
                    .await
                    .unwrap();
            }
            2 => {
                db::query("CREATE TABLE oc_improve_runs(id INTEGER)")
                    .execute(&mut *conn)
                    .await
                    .unwrap();
            }
            3 => {
                db::query("CREATE TABLE oc_learnings(id INTEGER)")
                    .execute(&mut *conn)
                    .await
                    .unwrap();
            }
            4 => {
                db::query("CREATE TABLE oc_learning_support(id INTEGER)")
                    .execute(&mut *conn)
                    .await
                    .unwrap();
            }
            5 => {
                db::query("INSERT INTO oc_events VALUES('learning')")
                    .execute(&mut *conn)
                    .await
                    .unwrap();
            }
            6 => {
                db::query("INSERT INTO oc_jobs VALUES('improve')")
                    .execute(&mut *conn)
                    .await
                    .unwrap();
            }
            _ => unreachable!(),
        }
        assert_eq!(
            super::improve::available(&mut conn).await.unwrap(),
            signal != 0,
            "signal {signal}"
        );
        drop(conn);
        pool.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn improve_availability_does_not_hide_backend_errors_after_a_true_signal() {
    for missing in ["events", "jobs"] {
        let dir = tempfile::tempdir().unwrap();
        let pool = Pool::open(dir.path().join("missing-availability-table.db"))
            .await
            .unwrap();
        let mut conn = pool.acquire().await.unwrap();
        db::query("CREATE TABLE oc_learnings(id INTEGER)")
            .execute(&mut *conn)
            .await
            .unwrap();
        if missing == "events" {
            db::query("CREATE TABLE oc_jobs(operation TEXT NOT NULL)")
                .execute(&mut *conn)
                .await
                .unwrap();
        } else {
            db::query("CREATE TABLE oc_events(kind TEXT NOT NULL)")
                .execute(&mut *conn)
                .await
                .unwrap();
            db::query("INSERT INTO oc_events VALUES('learning')")
                .execute(&mut *conn)
                .await
                .unwrap();
        }
        assert!(matches!(
            super::improve::available(&mut conn).await,
            Err(StorageError::Backend(_))
        ));
        drop(conn);
        pool.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn missing_learning_catalog_keeps_historical_classification_and_backend_error() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let row = Record::new(0);
    seed(
        &mut tx,
        scope,
        row,
        "knowledge",
        Some(2),
        "retracted",
        "learning",
    )
    .await;
    let current_source = Uuid::new_v4();
    db::query("INSERT INTO oc_events VALUES(?,?,?,'active','knowledge')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(current_source)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    version(&mut tx, scope, row.asset, 2, current_source).await;
    chunk(&mut tx, scope, row.asset, row.second_chunk(), 2, 0, "{}").await;
    db::query("DROP TABLE oc_learnings")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(
        !tx.context_route_matches(row.asset, ContextRoute::Knowledge)
            .await
            .unwrap()
    );
    assert!(
        tx.context_route_rows(ContextRoute::Knowledge, None)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        tx.context_route_matches(row.asset, ContextRoute::Learning)
            .await,
        Err(StorageError::Backend(_))
    ));
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn empty_route_chunks_do_not_hide_missing_association_tables() {
    for missing in ["assets", "versions", "events", "chunks"] {
        let (_dir, pool) = database().await;
        let mut tx = transaction(&pool, scope()).await;
        match missing {
            "assets" => {
                db::query("DROP TABLE oc_assets")
                    .execute(&mut *tx.tx)
                    .await
                    .unwrap();
            }
            "versions" => {
                db::query("DROP TABLE oc_versions")
                    .execute(&mut *tx.tx)
                    .await
                    .unwrap();
            }
            "events" => {
                db::query("DROP TABLE oc_events")
                    .execute(&mut *tx.tx)
                    .await
                    .unwrap();
            }
            "chunks" => {
                db::query("DROP TABLE oc_chunks")
                    .execute(&mut *tx.tx)
                    .await
                    .unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            matches!(
                tx.context_route_rows(ContextRoute::Knowledge, None).await,
                Err(StorageError::Backend(_))
            ),
            "missing {missing}"
        );
        tx.rollback().await.unwrap();
        pool.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn learning_history_ignores_unmatched_raw_sources_across_page_boundaries() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let first = Record::new(0);
    let later = Record::new(1);
    assert!(first.asset < later.asset);
    seed(
        &mut tx,
        scope,
        first,
        "knowledge",
        Some(1),
        "active",
        "learning",
    )
    .await;
    seed(
        &mut tx,
        scope,
        later,
        "knowledge",
        Some(1),
        "active",
        "knowledge",
    )
    .await;
    for number in 2..=256 {
        version(&mut tx, scope, first.asset, number, later.source).await;
    }
    for number in 2..=255 {
        version(&mut tx, scope, later.asset, number, later.source).await;
    }
    db::query("INSERT INTO oc_versions VALUES(?,?,?,257,'version','content','hash',x'01',NULL)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(first.asset)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("INSERT INTO oc_versions VALUES(?,?,?,256,'version','content','hash',x'02',NULL)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(later.asset)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    // Fixed-main classification was a native existence predicate. Unmatched
    // short BLOB sources are ordinary keys, including at page boundaries.
    assert!(tx.is_learning_asset(first.asset).await.unwrap());
    assert!(!tx.is_learning_asset(later.asset).await.unwrap());
    assert!(
        !tx.context_route_matches(first.asset, ContextRoute::Knowledge)
            .await
            .unwrap()
    );
    assert!(
        tx.context_route_matches(later.asset, ContextRoute::Knowledge)
            .await
            .unwrap()
    );
    let membership = tx
        .context_route_memberships(&[first.asset, later.asset], ContextRoute::Knowledge)
        .await
        .unwrap();
    assert_eq!(membership.len(), 2);
    assert!(!membership[&first.asset]);
    assert!(membership[&later.asset]);
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn learning_history_uses_native_raw_source_and_version_keys() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let mut assets = Vec::new();
    for index in 0..8 {
        let row = Record::new(index);
        seed(
            &mut tx,
            scope,
            row,
            "knowledge",
            Some(1),
            "active",
            "knowledge",
        )
        .await;
        let raw_source = vec![index as u8 + 1];
        if index < 4 {
            db::query("INSERT INTO oc_events VALUES(?,?,?,'retracted','learning')")
                .bind(scope.tenant_id)
                .bind(scope.workspace_id)
                .bind(raw_source.clone())
                .execute(&mut *tx.tx)
                .await
                .unwrap();
        }
        // These are valid native values in this non-STRICT INTEGER column.
        // Historical classification does not project a domain i32 or UUID.
        match index % 4 {
            0 => {
                db::query("INSERT INTO oc_versions VALUES(?,?,?,2147483648,'version','content','hash',?,NULL)")
                    .bind(scope.tenant_id).bind(scope.workspace_id).bind(row.asset).bind(raw_source)
                    .execute(&mut *tx.tx).await.unwrap();
            }
            1 => {
                db::query(
                    "INSERT INTO oc_versions VALUES(?,?,?,2.5,'version','content','hash',?,NULL)",
                )
                .bind(scope.tenant_id)
                .bind(scope.workspace_id)
                .bind(row.asset)
                .bind(raw_source)
                .execute(&mut *tx.tx)
                .await
                .unwrap();
            }
            2 => {
                db::query("INSERT INTO oc_versions VALUES(?,?,?,'historical-version','version','content','hash',?,NULL)")
                    .bind(scope.tenant_id).bind(scope.workspace_id).bind(row.asset).bind(raw_source)
                    .execute(&mut *tx.tx).await.unwrap();
            }
            3 => {
                db::query(
                    "INSERT INTO oc_versions VALUES(?,?,?,x'08','version','content','hash',?,NULL)",
                )
                .bind(scope.tenant_id)
                .bind(scope.workspace_id)
                .bind(row.asset)
                .bind(raw_source)
                .execute(&mut *tx.tx)
                .await
                .unwrap();
            }
            _ => unreachable!(),
        }
        assets.push(row.asset);
    }
    let membership = tx
        .context_route_memberships(&assets, ContextRoute::Knowledge)
        .await
        .unwrap();
    for (index, asset) in assets.iter().enumerate() {
        assert_eq!(membership[asset], index >= 4);
        assert_eq!(tx.is_learning_asset(*asset).await.unwrap(), index < 4);
    }
    assert_eq!(
        ids(tx
            .context_route_rows(ContextRoute::Knowledge, None)
            .await
            .unwrap()),
        (4..8)
            .map(|index| Record::new(index).chunk)
            .collect::<HashSet<_>>()
    );
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn learning_membership_preserves_raw_improve_keys_and_native_status_predicates() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let mut assets = Vec::new();
    for index in 0..6 {
        let row = Record::new(index);
        let published = index % 2 == 0;
        seed(
            &mut tx,
            scope,
            row,
            "knowledge",
            Some(1),
            "active",
            "learning",
        )
        .await;
        registration(
            &mut tx,
            scope,
            row,
            row.source,
            if published { "published" } else { "accepted" },
        )
        .await;
        let raw_improve = vec![index as u8 + 1];
        db::query("UPDATE oc_learnings SET improve_id=? WHERE tenant_id=? AND workspace_id=? AND asset_id=?")
            .bind(raw_improve.clone()).bind(scope.tenant_id).bind(scope.workspace_id).bind(row.asset)
            .execute(&mut *tx.tx).await.unwrap();
        match index / 2 {
            0 => {
                db::query("INSERT INTO oc_improve_runs VALUES(?,?,?,'accepted')")
                    .bind(scope.tenant_id)
                    .bind(scope.workspace_id)
                    .bind(raw_improve)
                    .execute(&mut *tx.tx)
                    .await
                    .unwrap();
            }
            1 => {
                db::query("INSERT INTO oc_improve_runs VALUES(?,?,?,'publishing')")
                    .bind(scope.tenant_id)
                    .bind(scope.workspace_id)
                    .bind(raw_improve)
                    .execute(&mut *tx.tx)
                    .await
                    .unwrap();
            }
            2 => {
                db::query("INSERT INTO oc_improve_runs VALUES(?,?,?,x'01')")
                    .bind(scope.tenant_id)
                    .bind(scope.workspace_id)
                    .bind(raw_improve)
                    .execute(&mut *tx.tx)
                    .await
                    .unwrap();
            }
            _ => unreachable!(),
        }
        assets.push(row.asset);
    }
    // Improve IDs and run statuses only participate in native SQL equality
    // and EXISTS. Neither becomes a newly decoded UUID or String here.
    let knowledge = tx
        .context_route_memberships(&assets, ContextRoute::Knowledge)
        .await
        .unwrap();
    let learning = tx
        .context_route_memberships(&assets, ContextRoute::Learning)
        .await
        .unwrap();
    for (index, asset) in assets.iter().enumerate() {
        assert!(!knowledge[asset]);
        assert_eq!(learning[asset], index % 2 == 0);
    }
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn current_route_reads_only_current_payloads_once_across_key_batches() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let mut expected = HashSet::new();
    for index in 0..9 {
        let row = Record::new(20_000 + index);
        seed(
            &mut tx,
            scope,
            row,
            "knowledge",
            Some(1),
            "active",
            "knowledge",
        )
        .await;
        chunk(&mut tx, scope, row.asset, row.second_chunk(), 1, 1, "{}").await;
        expected.extend([row.chunk, row.second_chunk()]);
        // Historical payloads are large and deliberately invalid as text.
        // They must neither decode nor enter the current payload read set.
        for historical in 2..=130 {
            db::query("INSERT INTO oc_chunks VALUES(?,?,?,?,?,0,'historical',zeroblob(4096),zeroblob(4096))")
                .bind(scope.tenant_id)
                .bind(scope.workspace_id)
                .bind(Uuid::new_v4())
                .bind(row.asset)
                .bind(historical)
                .execute(&mut *tx.tx)
                .await
                .unwrap();
        }
    }
    let (rows, counts) =
        super::read_model::context_current_chunk_rows_measured(&mut tx.tx, scope, "knowledge")
            .await
            .unwrap();
    assert_eq!(counts, [9, 9, 18]);
    assert_eq!(rows.len(), 18);
    assert_eq!(
        rows.into_iter().map(|row| row.0).collect::<HashSet<_>>(),
        expected
    );
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn current_route_keeps_native_non_uuid_roots_and_text_versions() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let row = Record::new(30_000);
    seed(
        &mut tx,
        scope,
        row,
        "knowledge",
        Some(1),
        "active",
        "knowledge",
    )
    .await;
    db::query("UPDATE oc_assets SET current_version='revision-x'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_versions SET version='revision-x',source_event_id=x'31'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_events SET id=x'31'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_chunks SET version='revision-x'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    let (rows, counts) =
        super::read_model::context_current_chunk_rows_measured(&mut tx.tx, scope, "knowledge")
            .await
            .unwrap();
    assert_eq!(counts, [1, 1, 1]);
    assert_eq!(rows[0].0, row.chunk);
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_context_routes_filter_before_loading_other_route_content() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    db::query("CREATE TABLE oc_index_entries(tenant_id BLOB,workspace_id BLOB,artifact_id BLOB,model_id TEXT,dimension INTEGER,generation INTEGER,state TEXT)")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("CREATE TABLE oc_artifact_ledger(tenant_id BLOB,workspace_id BLOB,artifact_id BLOB,source_id BLOB,version INTEGER,generation INTEGER,surface TEXT,state TEXT)")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    let good = Record::new(40_000);
    let other = Record::new(40_001);
    for (row, kind) in [(good, "knowledge"), (other, "memory")] {
        seed(&mut tx, scope, row, kind, Some(1), "active", kind).await;
        db::query("INSERT INTO oc_index_entries VALUES(?,?,?,'profile',2,7,'ready')")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(row.chunk)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        for surface in ["vector", "full_text"] {
            db::query("INSERT INTO oc_artifact_ledger VALUES(?,?,?,?,1,7,?,'committed')")
                .bind(scope.tenant_id)
                .bind(scope.workspace_id)
                .bind(row.chunk)
                .bind(row.source)
                .bind(surface)
                .execute(&mut *tx.tx)
                .await
                .unwrap();
        }
    }
    db::query("UPDATE oc_chunks SET content=zeroblob(65536) WHERE id=?")
        .bind(other.chunk)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert_eq!(
        tx.context_vector_candidates("profile", 2, 7, ContextRoute::Knowledge, None)
            .await
            .unwrap(),
        vec![good.chunk]
    );
    assert_eq!(
        tx.context_full_text_candidates(7, ContextRoute::Knowledge, None)
            .await
            .unwrap(),
        vec![good.chunk]
    );
    // Candidate IDs never consume content, including the requested route.
    assert_eq!(
        tx.context_vector_candidates("profile", 2, 7, ContextRoute::Memory, None)
            .await
            .unwrap(),
        vec![other.chunk]
    );
    assert_eq!(
        tx.context_full_text_candidates(7, ContextRoute::Memory, None)
            .await
            .unwrap(),
        vec![other.chunk]
    );
    // A selected full hit still safely rejects the invalid content.
    assert!(matches!(
        tx.visible_hit(other.chunk).await,
        Err(StorageError::Backend(_))
    ));
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}
