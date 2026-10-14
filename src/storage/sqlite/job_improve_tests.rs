//! Native metadata predicates for job fencing and Learning transitions.
//! No multi-relation SQL oracle: assert the original state and key contracts.
use super::improve::{
    cancelled_improve_job, learning_ingest_valid, learning_publication_ready,
    unfinished_learning_jobs,
};
use super::*;
use crate::storage::native_turso as db;

#[derive(Clone, Copy)]
struct Record {
    asset: Uuid,
    source: Uuid,
    job: Uuid,
    learning: Uuid,
    improve: Uuid,
}

impl Record {
    fn new(index: u128) -> Self {
        Self {
            asset: Uuid::from_u128(10_000 + index),
            source: Uuid::from_u128(20_000 + index),
            job: Uuid::from_u128(30_000 + index),
            learning: Uuid::from_u128(40_000 + index),
            improve: Uuid::from_u128(50_000 + index),
        }
    }
}

fn scope() -> Scope {
    Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    }
}

async fn database() -> (tempfile::TempDir, Pool) {
    let dir = tempfile::tempdir().unwrap();
    let pool = Pool::open(dir.path().join("job-improve.db")).await.unwrap();
    // Omit foreign keys so NULL/missing/raw associations can be tested without
    // changing the production schema or decoding fields the old EXISTS ignored.
    for sql in [
        "CREATE TABLE oc_assets(tenant_id BLOB,workspace_id BLOB,id BLOB,kind TEXT,deleted INTEGER,current_version INTEGER,PRIMARY KEY(tenant_id,workspace_id,id))",
        "CREATE TABLE oc_events(tenant_id BLOB,workspace_id BLOB,id BLOB,kind TEXT,state TEXT,PRIMARY KEY(tenant_id,workspace_id,id))",
        "CREATE TABLE oc_jobs(tenant_id BLOB,workspace_id BLOB,id BLOB,asset_id BLOB,source_event_id BLOB,operation TEXT,state TEXT,cancel_requested INTEGER,run_token INTEGER,generation INTEGER,attempt INTEGER,outcome TEXT,error_code TEXT,next_retry_at INTEGER,updated_at INTEGER,PRIMARY KEY(tenant_id,workspace_id,id))",
        "CREATE TABLE oc_learnings(tenant_id BLOB,workspace_id BLOB,id BLOB,asset_id BLOB,source_event_id BLOB,ingest_job_id BLOB,improve_id BLOB,status TEXT,PRIMARY KEY(tenant_id,workspace_id,id))",
        "CREATE TABLE oc_improve_runs(tenant_id BLOB,workspace_id BLOB,improve_id BLOB,job_id BLOB,status TEXT,PRIMARY KEY(tenant_id,workspace_id,improve_id))",
        "CREATE TABLE oc_versions(tenant_id BLOB,workspace_id BLOB,asset_id BLOB,version INTEGER,source_event_id BLOB,PRIMARY KEY(tenant_id,workspace_id,asset_id,version))",
        "CREATE TABLE oc_chunks(tenant_id BLOB,workspace_id BLOB,id BLOB,asset_id BLOB,version INTEGER,PRIMARY KEY(tenant_id,workspace_id,id))",
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

async fn seed(conn: &mut Connection, scope: Scope, r: Record) {
    db::query("INSERT INTO oc_assets VALUES(?,?,?,'knowledge',0,1)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(r.asset)
        .execute(&mut *conn)
        .await
        .unwrap();
    db::query("INSERT INTO oc_events VALUES(?,?,?,'learning','active')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(r.source)
        .execute(&mut *conn)
        .await
        .unwrap();
    db::query("INSERT INTO oc_jobs VALUES(?,?,?,?,?,'ingest','pending',0,7,3,2,'old-outcome','old-error',99,123)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(r.job)
        .bind(r.asset)
        .bind(r.source)
        .execute(&mut *conn)
        .await
        .unwrap();
    db::query("INSERT INTO oc_learnings VALUES(?,?,?,?,?,?,?,'accepted')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(r.learning)
        .bind(r.asset)
        .bind(r.source)
        .bind(r.job)
        .bind(r.improve)
        .execute(&mut *conn)
        .await
        .unwrap();
    db::query("INSERT INTO oc_improve_runs VALUES(?,?,?,?,'publishing')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(r.improve)
        .bind(Uuid::new_v4())
        .execute(&mut *conn)
        .await
        .unwrap();
    db::query("INSERT INTO oc_versions VALUES(?,?,?,1,?)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(r.asset)
        .bind(r.source)
        .execute(&mut *conn)
        .await
        .unwrap();
    db::query("INSERT INTO oc_chunks VALUES(?,?,?,?,1)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(Uuid::new_v4())
        .bind(r.asset)
        .execute(conn)
        .await
        .unwrap();
}

#[tokio::test]
async fn job_cancellation_preserves_scope_fences_and_other_fields() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let other = Scope {
        workspace_id: Uuid::new_v4(),
        ..scope
    };
    let mut tx = transaction(&pool, scope).await;
    let mut expected = Vec::new();
    for (state_index, state) in [
        "pending",
        "processing",
        "failed",
        "retry_wait",
        "completed",
        "cancelled",
        "superseded",
    ]
    .into_iter()
    .enumerate()
    {
        for mask in 0..4 {
            let r = Record::new((state_index * 4 + mask) as u128);
            seed(&mut tx.tx, scope, r).await;
            db::query("UPDATE oc_jobs SET state=? WHERE id=?")
                .bind(state)
                .bind(r.job)
                .execute(&mut *tx.tx)
                .await
                .unwrap();
            db::query("UPDATE oc_assets SET deleted=? WHERE id=?")
                .bind(if mask & 1 != 0 { 2 } else { 0 })
                .bind(r.asset)
                .execute(&mut *tx.tx)
                .await
                .unwrap();
            db::query("UPDATE oc_events SET state=? WHERE id=?")
                .bind(if mask & 2 != 0 { "retracted" } else { "active" })
                .bind(r.source)
                .execute(&mut *tx.tx)
                .await
                .unwrap();
            expected.push((r.job, state, state_index < 4 && mask != 0));
        }
    }
    let cleanup = Record::new(100);
    seed(&mut tx.tx, scope, cleanup).await;
    db::query("UPDATE oc_jobs SET operation='cleanup' WHERE id=?")
        .bind(cleanup.job)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_assets SET deleted=1 WHERE id=?")
        .bind(cleanup.asset)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    expected.push((cleanup.job, "pending", false));
    let foreign = Record::new(0);
    seed(&mut tx.tx, other, foreign).await;
    db::query(
        "UPDATE oc_events SET state='retracted' WHERE tenant_id=? AND workspace_id=? AND id=?",
    )
    .bind(other.tenant_id)
    .bind(other.workspace_id)
    .bind(foreign.source)
    .execute(&mut *tx.tx)
    .await
    .unwrap();
    let missing = Record::new(102);
    seed(&mut tx.tx, scope, missing).await;
    db::query("UPDATE oc_jobs SET asset_id=NULL,source_event_id=? WHERE id=?")
        .bind(Uuid::new_v4())
        .bind(missing.job)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    expected.push((missing.job, "pending", false));
    tx.tx.commit().await.unwrap();

    let mut tx = transaction(&pool, scope).await;
    tx.cancel_affected_jobs_inner().await.unwrap();
    tx.cancel_affected_jobs_inner().await.unwrap();
    for (job, state, cancelled) in expected {
        let actual: (String, i64, i64, i64, i64, String, String, i64, i64) =
            db::query_as("SELECT state,cancel_requested,run_token,generation,attempt,outcome,error_code,next_retry_at,updated_at FROM oc_jobs WHERE tenant_id=? AND workspace_id=? AND id=?")
                .bind(scope.tenant_id)
                .bind(scope.workspace_id)
                .bind(job)
                .fetch_one(&mut *tx.tx)
                .await
                .unwrap();
        assert_eq!(actual.0, if cancelled { "cancelled" } else { state });
        assert_eq!(actual.1, i64::from(cancelled));
        assert_eq!(actual.2, if cancelled { 8 } else { 7 });
        assert_eq!((actual.3, actual.4, actual.7), (3, 2, 99));
        assert_eq!(
            (actual.5.as_str(), actual.6.as_str()),
            ("old-outcome", "old-error")
        );
        assert_eq!(actual.8 == 123, !cancelled);
    }
    let state: String =
        db::query_scalar("SELECT state FROM oc_jobs WHERE tenant_id=? AND workspace_id=? AND id=?")
            .bind(other.tenant_id)
            .bind(other.workspace_id)
            .bind(foreign.job)
            .fetch_one(&mut *tx.tx)
            .await
            .unwrap();
    assert_eq!(state, "pending");
    tx.tx.rollback().await.unwrap();
    let changed: i64 =
        db::query_scalar("SELECT count(*) FROM oc_jobs WHERE run_token<>7 OR updated_at<>123")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(changed, 0, "all cancellation writes roll back together");
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn job_cancellation_uses_raw_keys_and_sql_truth() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    db::query("INSERT INTO oc_assets VALUES(?,?,'raw-asset','knowledge',2,1)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("INSERT INTO oc_jobs VALUES(?,?,'raw-job','raw-asset',NULL,'ingest','pending',0,7,3,2,NULL,NULL,NULL,123)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("INSERT INTO oc_jobs VALUES(?,?,'blob-job',x'7261772d6173736574',NULL,'ingest','pending',0,7,3,2,NULL,NULL,NULL,123)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    tx.cancel_affected_jobs_inner().await.unwrap();
    let rows: Vec<(String, String, i64)> =
        db::query_as("SELECT id,state,run_token FROM oc_jobs ORDER BY id")
            .fetch_all(&mut *tx.tx)
            .await
            .unwrap();
    assert_eq!(
        rows,
        [
            (String::from("blob-job"), String::from("pending"), 7),
            (String::from("raw-job"), String::from("cancelled"), 8),
        ]
    );
    tx.tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn learning_registration_keeps_each_exact_ingestion_predicate() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let r = Record::new(1);
    for (sql, expected) in [
        ("UPDATE oc_jobs SET state='pending'", true),
        ("UPDATE oc_jobs SET state='processing'", true),
        ("UPDATE oc_jobs SET state='retry_wait'", true),
        ("UPDATE oc_jobs SET state='failed'", false),
        ("UPDATE oc_jobs SET state='completed'", false),
        ("UPDATE oc_jobs SET operation='publish'", false),
        ("UPDATE oc_jobs SET cancel_requested=2", false),
        ("UPDATE oc_jobs SET asset_id=NULL", false),
        ("UPDATE oc_jobs SET source_event_id=x'31'", false),
        ("UPDATE oc_assets SET kind='memory'", false),
        ("UPDATE oc_assets SET deleted=2", false),
        ("UPDATE oc_events SET kind='ordinary'", false),
        ("UPDATE oc_events SET state='retracted'", false),
    ] {
        let mut tx = transaction(&pool, scope).await;
        seed(&mut tx.tx, scope, r).await;
        db::query(sql).execute(&mut *tx.tx).await.unwrap();
        let valid = learning_ingest_valid(&mut tx.tx, scope, r.asset, r.source, r.job)
            .await
            .unwrap();
        assert_eq!(valid, expected, "{sql}");
        let other = Scope {
            workspace_id: Uuid::new_v4(),
            ..scope
        };
        assert!(
            !learning_ingest_valid(&mut tx.tx, other, r.asset, r.source, r.job)
                .await
                .unwrap()
        );
        tx.tx.rollback().await.unwrap();
    }
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn learning_publication_requires_current_root_child_and_chunk() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let r = Record::new(1);
    for (sql, expected) in [
        ("UPDATE oc_assets SET current_version=1", true),
        ("UPDATE oc_assets SET current_version=2", false),
        ("UPDATE oc_assets SET current_version=NULL", false),
        ("UPDATE oc_versions SET source_event_id=x'31'", false),
        ("UPDATE oc_versions SET version=2", false),
        ("UPDATE oc_chunks SET version=2", false),
        ("UPDATE oc_jobs SET state='processing'", false),
        ("UPDATE oc_jobs SET outcome='ok'", false),
        ("UPDATE oc_jobs SET cancel_requested=2", false),
        ("UPDATE oc_learnings SET ingest_job_id=NULL", false),
        ("DELETE FROM oc_assets", false),
        ("DELETE FROM oc_versions", false),
        ("DELETE FROM oc_chunks", false),
        ("DELETE FROM oc_jobs", false),
    ] {
        let mut tx = transaction(&pool, scope).await;
        seed(&mut tx.tx, scope, r).await;
        db::query("UPDATE oc_jobs SET state='completed',outcome='published'")
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        db::query(sql).execute(&mut *tx.tx).await.unwrap();
        let ready = learning_publication_ready(&mut tx.tx, scope, r.learning)
            .await
            .unwrap();
        assert_eq!(ready, expected, "{sql}");
        assert!(
            !learning_publication_ready(&mut tx.tx, scope, Uuid::new_v4())
                .await
                .unwrap()
        );
        tx.tx.rollback().await.unwrap();
    }
    // These association values were never domain UUID outputs of the old EXISTS.
    let mut tx = transaction(&pool, scope).await;
    seed(&mut tx.tx, scope, r).await;
    for sql in [
        "UPDATE oc_assets SET id='raw-asset'",
        "UPDATE oc_versions SET asset_id='raw-asset',source_event_id=x'31'",
        "UPDATE oc_chunks SET asset_id='raw-asset'",
        "UPDATE oc_jobs SET id='raw-job',state='completed',outcome='published'",
        "UPDATE oc_learnings SET asset_id='raw-asset',source_event_id=x'31',ingest_job_id='raw-job'",
    ] {
        db::query(sql).execute(&mut *tx.tx).await.unwrap();
    }
    assert!(
        learning_publication_ready(&mut tx.tx, scope, r.learning)
            .await
            .unwrap()
    );
    tx.tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn learning_child_cancellation_filters_before_uuid_decode() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let improve = Uuid::new_v4();
    let mut expected = Vec::new();
    for (index, state) in [
        "pending",
        "processing",
        "failed",
        "retry_wait",
        "completed",
        "cancelled",
        "superseded",
    ]
    .into_iter()
    .enumerate()
    {
        let r = Record::new(index as u128);
        seed(&mut tx.tx, scope, r).await;
        db::query("UPDATE oc_learnings SET improve_id=? WHERE id=?")
            .bind(improve)
            .bind(r.learning)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        db::query("UPDATE oc_jobs SET state=? WHERE id=?")
            .bind(state)
            .bind(r.job)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        if index < 4 {
            expected.push(r.job);
        }
    }
    let published = Record::new(100);
    seed(&mut tx.tx, scope, published).await;
    db::query("UPDATE oc_learnings SET improve_id=?,status='published' WHERE id=?")
        .bind(improve)
        .bind(published.learning)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    let mut actual = unfinished_learning_jobs(&mut tx.tx, scope, improve)
        .await
        .unwrap();
    actual.sort_unstable();
    expected.sort_unstable();
    assert_eq!(actual, expected);
    db::query("INSERT INTO oc_jobs VALUES(?,?,x'31',NULL,NULL,'ingest','completed',0,7,3,2,NULL,NULL,NULL,123)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("INSERT INTO oc_learnings VALUES(?,?,?,NULL,NULL,x'31',?,'accepted')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(Uuid::new_v4())
        .bind(improve)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert_eq!(
        unfinished_learning_jobs(&mut tx.tx, scope, improve)
            .await
            .unwrap()
            .len(),
        4
    );
    db::query("UPDATE oc_jobs SET state='failed' WHERE id=x'31'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(matches!(
        unfinished_learning_jobs(&mut tx.tx, scope, improve).await,
        Err(StorageError::Backend(_))
    ));
    tx.tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn learning_retry_checks_parent_child_and_native_run_keys() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let r = Record::new(1);
    let mut tx = transaction(&pool, scope).await;
    seed(&mut tx.tx, scope, r).await;
    let parent = Uuid::new_v4();
    db::query("UPDATE oc_improve_runs SET job_id=?,status='cancelled'")
        .bind(parent)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    for job in [parent, r.job] {
        assert!(cancelled_improve_job(&mut tx.tx, scope, job).await.unwrap());
        let other = Scope {
            workspace_id: Uuid::new_v4(),
            ..scope
        };
        assert!(!cancelled_improve_job(&mut tx.tx, other, job).await.unwrap());
    }
    assert!(
        !cancelled_improve_job(&mut tx.tx, scope, Uuid::new_v4())
            .await
            .unwrap()
    );
    db::query("UPDATE oc_improve_runs SET improve_id='raw-run'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_learnings SET improve_id='raw-run'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(
        cancelled_improve_job(&mut tx.tx, scope, r.job)
            .await
            .unwrap()
    );
    db::query("UPDATE oc_learnings SET improve_id=x'7261772d72756e'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(
        !cancelled_improve_job(&mut tx.tx, scope, r.job)
            .await
            .unwrap()
    );
    db::query("UPDATE oc_improve_runs SET status='completed'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(
        !cancelled_improve_job(&mut tx.tx, scope, parent)
            .await
            .unwrap()
    );
    tx.tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn job_improve_empty_inputs_do_not_hide_relation_prepare_errors() {
    for case in 0..5 {
        let (_dir, pool) = database().await;
        let scope = scope();
        let mut tx = transaction(&pool, scope).await;
        let r = Record::new(1);
        if case == 4 {
            seed(&mut tx.tx, scope, r).await;
            db::query("UPDATE oc_improve_runs SET job_id=?,status='cancelled'")
                .bind(r.job)
                .execute(&mut *tx.tx)
                .await
                .unwrap();
        }
        match case {
            0 => db::query("DROP TABLE oc_events"),
            1 => db::query("DROP TABLE oc_jobs"),
            2 => db::query("DROP TABLE oc_chunks"),
            3 => db::query("DROP TABLE oc_jobs"),
            _ => db::query("DROP TABLE oc_learnings"),
        }
        .execute(&mut *tx.tx)
        .await
        .unwrap();
        let result = match case {
            0 => tx.cancel_affected_jobs_inner().await,
            1 => learning_ingest_valid(&mut tx.tx, scope, r.asset, r.source, r.job)
                .await
                .map(|_| ()),
            2 => learning_publication_ready(&mut tx.tx, scope, r.learning)
                .await
                .map(|_| ()),
            3 => unfinished_learning_jobs(&mut tx.tx, scope, r.improve)
                .await
                .map(|_| ()),
            _ => cancelled_improve_job(&mut tx.tx, scope, r.job)
                .await
                .map(|_| ()),
        };
        assert!(
            matches!(result, Err(StorageError::Backend(_))),
            "case {case}: {result:?}"
        );
        tx.tx.rollback().await.unwrap();
        pool.shutdown().await.unwrap();
    }
}
