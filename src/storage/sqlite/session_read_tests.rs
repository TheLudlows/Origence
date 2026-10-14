//! Real native-engine archive reads. Expected results are asserted directly;
//! no multi-relation SQL oracle or public-permission bypass is introduced.
use super::*;
use crate::session::{ArchivedEvidence, EvidenceKind, UsedEvidence};
use crate::storage::native_turso as db;

async fn database() -> (tempfile::TempDir, Pool) {
    let dir = tempfile::tempdir().unwrap();
    let pool = Pool::open(dir.path().join("session-reads.db")).await.unwrap();
    // Deliberately omit unrelated asset/version metadata. These local reads
    // need exact historical provenance, not title, kind, or current content.
    for sql in [
        "CREATE TABLE oc_assets(tenant_id BLOB,workspace_id BLOB,id BLOB,deleted INTEGER,current_version INTEGER,PRIMARY KEY(tenant_id,workspace_id,id))",
        "CREATE TABLE oc_versions(tenant_id BLOB,workspace_id BLOB,asset_id BLOB,version INTEGER,source_event_id BLOB,PRIMARY KEY(tenant_id,workspace_id,asset_id,version))",
        "CREATE TABLE oc_events(tenant_id BLOB,workspace_id BLOB,id BLOB,state TEXT,kind TEXT,content TEXT,PRIMARY KEY(tenant_id,workspace_id,id))",
        "CREATE TABLE oc_chunks(tenant_id BLOB,workspace_id BLOB,id BLOB,asset_id BLOB,version INTEGER,locator TEXT,PRIMARY KEY(tenant_id,workspace_id,id))",
        "CREATE TABLE oc_artifact_owners(tenant_id BLOB,workspace_id BLOB,source_id BLOB,version INTEGER,artifact_type TEXT,artifact_id BLOB,chunk_id BLOB)",
        "CREATE TABLE oc_artifact_ledger(tenant_id BLOB,workspace_id BLOB,source_id BLOB,version INTEGER,artifact_type TEXT,artifact_id BLOB,surface TEXT,state TEXT,generation INTEGER)",
        "CREATE TABLE oc_session_turns(tenant_id BLOB,workspace_id BLOB,session_id BLOB,turn_id BLOB,source_event_id BLOB)",
        "CREATE TABLE oc_session_turn_evidence(tenant_id BLOB,workspace_id BLOB,session_id BLOB,turn_id BLOB,evidence_ref_id BLOB,ordinal INTEGER,evidence TEXT)",
    ] {
        db::query(sql)
            .execute(&pool)
            .await
            .unwrap();
    }
    (dir, pool)
}

fn scope() -> Scope {
    Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    }
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

fn reference(index: u128) -> UsedEvidence {
    let chunk_id = Uuid::from_u128(20_000 + index);
    UsedEvidence {
        kind: EvidenceKind::Chunk,
        id: chunk_id,
        asset_id: Uuid::from_u128(10_000 + index),
        asset_version: 1,
        chunk_id,
    }
}

async fn event(tx: &mut TursoTx, scope: Scope, id: Uuid, content: &str) {
    db::query("INSERT INTO oc_events VALUES(?,?,?,'active','ordinary',?)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(id)
        .bind(content)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
}

async fn seed(tx: &mut TursoTx, scope: Scope, index: u128) -> ArchivedEvidence {
    let reference = reference(index);
    let root = Uuid::from_u128(30_000 + index);
    event(tx, scope, root, "root content").await;
    db::query("INSERT INTO oc_assets VALUES(?,?,?,0,2)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(reference.asset_id)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("INSERT INTO oc_versions VALUES(?,?,?,?,?)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(reference.asset_id)
        .bind(reference.asset_version)
        .bind(root)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("INSERT INTO oc_chunks VALUES(?,?,?,?,?,'{}')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(reference.chunk_id)
        .bind(reference.asset_id)
        .bind(reference.asset_version)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("INSERT INTO oc_artifact_owners VALUES(?,?,?,?,'chunk',?,?)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(root)
        .bind(reference.asset_version)
        .bind(reference.chunk_id)
        .bind(reference.chunk_id)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    ArchivedEvidence {
        evidence_ref_id: Uuid::from_u128(40_000 + index),
        reference,
        source_event_id: root,
        source_version: 1,
        version_source_event_id: root,
        locator: json!({}),
    }
}

async fn store_turn(tx: &mut TursoTx, evidence: &[ArchivedEvidence]) -> (Uuid, Uuid, Uuid) {
    let session = Uuid::new_v4();
    let turn = Uuid::new_v4();
    let root = Uuid::new_v4();
    let scope = tx.scope;
    event(tx, scope, root, "turn content").await;
    db::query("INSERT INTO oc_session_turns VALUES(?,?,?,?,?)")
        .bind(tx.scope.tenant_id)
        .bind(tx.scope.workspace_id)
        .bind(session)
        .bind(turn)
        .bind(root)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    for (ordinal, archived) in evidence.iter().enumerate() {
        db::query("INSERT INTO oc_session_turn_evidence VALUES(?,?,?,?,?,?,?)")
            .bind(tx.scope.tenant_id)
            .bind(tx.scope.workspace_id)
            .bind(session)
            .bind(turn)
            .bind(archived.evidence_ref_id)
            .bind(ordinal as i64)
            .bind(serde_json::to_string(archived)
            .unwrap())
            .execute(&mut *tx.tx)
            .await
            .unwrap();
    }
    (session, turn, root)
}

async fn set_locator(tx: &mut TursoTx, reference: &UsedEvidence, locator: &str) {
    db::query("UPDATE oc_chunks SET locator=? WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(locator)
        .bind(tx.scope.tenant_id)
        .bind(tx.scope.workspace_id)
        .bind(reference.chunk_id)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
}

#[tokio::test]
async fn session_history_batches_preserve_64_ordered_historical_references() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let mut evidence = Vec::new();
    for index in 0..64 {
        evidence.push(seed(&mut tx, scope, index).await);
    }
    let (session, turn, root) = store_turn(&mut tx, &evidence).await;
    assert_eq!(tx.require_turn_visible_local(session, turn).await.unwrap(), (root, evidence));
    // No current-version predicate: current=2 above and NULL here both retain
    // exact version 1. The next read observes this same transaction's changes.
    db::query("UPDATE oc_assets SET current_version=NULL")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert_eq!(tx.require_turn_visible_local(session, turn).await.unwrap().1.len(), 64);
    db::query("INSERT INTO oc_session_turn_evidence VALUES(?,?,?,?,?,64,'not JSON')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(session)
        .bind(turn)
        .bind(Uuid::new_v4())
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    // The 65-row probe rejects before JSON deserialization of any evidence.
    assert!(matches!(
        tx.require_turn_visible_local(session, turn).await,
        Err(StorageError::NotFound)
    ));
    tx.tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn session_history_native_scope_and_exact_chunk_keys_are_required() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let other = Scope {
        tenant_id: scope.tenant_id,
        workspace_id: Uuid::new_v4(),
    };
    let mut tx = transaction(&pool, scope).await;
    let wanted = seed(&mut tx, scope, 1).await;
    seed(&mut tx, other, 1).await;
    let other_tenant = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: scope.workspace_id,
    };
    seed(&mut tx, other_tenant, 1).await;
    let (session, turn, _) = store_turn(&mut tx, std::slice::from_ref(&wanted)).await;
    for sql in [
        "UPDATE oc_chunks SET version=2 WHERE tenant_id=? AND workspace_id=?",
        "UPDATE oc_chunks SET version=1,asset_id=zeroblob(16) WHERE tenant_id=? AND workspace_id=?",
    ] {
        db::query(sql)
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        assert!(matches!(
            tx.require_turn_visible_local(session, turn).await,
            Err(StorageError::NotFound)
        ));
    }
    db::query("UPDATE oc_chunks SET asset_id=? WHERE tenant_id=? AND workspace_id=?")
        .bind(wanted.reference.asset_id)
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(tx.require_turn_visible_local(session, turn).await.is_ok());
    db::query("UPDATE oc_events SET state='withdrawn' WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(wanted.source_event_id)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(matches!(
        tx.require_turn_visible_local(session, turn).await,
        Err(StorageError::NotFound)
    ));
    db::query("UPDATE oc_events SET state='active' WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(wanted.source_event_id)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_assets SET deleted=1 WHERE tenant_id=? AND workspace_id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(matches!(
        tx.require_turn_visible_local(session, turn).await,
        Err(StorageError::NotFound)
    ));
    db::query("UPDATE oc_assets SET deleted=0 WHERE tenant_id=? AND workspace_id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("DELETE FROM oc_events WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(wanted.source_event_id)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(matches!(
        tx.require_turn_visible_local(session, turn).await,
        Err(StorageError::NotFound)
    ));
    tx.tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn session_empty_and_invalid_archives_do_not_prepare_evidence_tables() {
    let (_dir, pool) = database().await;
    let mut tx = transaction(&pool, scope()).await;
    let (session, turn, root) = store_turn(&mut tx, &[]).await;
    for sql in ["DROP TABLE oc_assets", "DROP TABLE oc_versions", "DROP TABLE oc_chunks", "DROP TABLE oc_artifact_owners", "DROP TABLE oc_artifact_ledger"] {
        db::query(sql)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
    }
    // The local root requires active state, not the public session source kind.
    assert_eq!(tx.require_turn_visible_local(session, turn).await.unwrap(), (root, vec![]));
    let mut invalid = ArchivedEvidence {
        evidence_ref_id: Uuid::new_v4(),
        reference: reference(1),
        source_event_id: root,
        source_version: 1,
        version_source_event_id: root,
        locator: json!({}),
    };
    invalid.reference.asset_version = 0;
    let (session, turn, _) = store_turn(&mut tx, &[invalid]).await;
    assert!(matches!(
        tx.require_turn_visible_local(session, turn).await,
        Err(StorageError::InvalidArgument(_))
    ));
    tx.tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn session_missing_chunk_and_false_support_do_not_prepare_owners() {
    for missing_chunk in [true, false] {
        let (_dir, pool) = database().await;
        let scope = scope();
        let mut tx = transaction(&pool, scope).await;
        let mut evidence = seed(&mut tx, scope, 1).await;
        if missing_chunk {
            evidence.reference.chunk_id = Uuid::new_v4();
            evidence.reference.id = evidence.reference.chunk_id;
        } else {
            let locator = json!({"parser": crate::memory_semantics::PARSER, "support_sources": [Uuid::new_v4()]}).to_string();
            set_locator(&mut tx, &evidence.reference, &locator).await;
        }
        db::query("DROP TABLE oc_artifact_owners")
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        assert!(matches!(
            tx.archive_evidence_local(&evidence.reference, evidence.evidence_ref_id).await,
            Err(StorageError::NotFound)
        ));
        tx.tx.rollback().await.unwrap();
        pool.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn session_graph_owner_and_committed_ledger_preserve_complete_keys() {
    for kind in [EvidenceKind::Entity, EvidenceKind::Relation] {
        let (_dir, pool) = database().await;
        let scope = scope();
        let mut tx = transaction(&pool, scope).await;
        let mut evidence = seed(&mut tx, scope, 1).await;
        evidence.reference.kind = kind;
        evidence.reference.id = Uuid::new_v4();
        let artifact_type = if kind == EvidenceKind::Entity { "entity" } else { "relation" };
        db::query("INSERT INTO oc_artifact_owners VALUES(?,?,?,?,?,?,?)")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(evidence.source_event_id)
            .bind(1_i32)
            .bind(artifact_type)
            .bind(evidence.reference.id)
            .bind(Uuid::new_v4())
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        db::query("INSERT INTO oc_artifact_ledger VALUES(?,?,?,?,?,?,'graph','committed',937)")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(evidence.source_event_id)
            .bind(1_i32)
            .bind(artifact_type)
            .bind(evidence.reference.id)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        // Graph owner.chunk_id is deliberately non-NULL and not the chosen
        // chunk; generation is unrelated to any current publication counter.
        assert_eq!(tx.archive_evidence_local(&evidence.reference, evidence.evidence_ref_id).await.unwrap(), evidence);
        for sql in [
            "UPDATE oc_artifact_ledger SET state='pending'",
            "UPDATE oc_artifact_ledger SET state='withdrawn'",
            "UPDATE oc_artifact_ledger SET state='committed',version=2",
            "UPDATE oc_artifact_ledger SET version=1,source_id=zeroblob(16)",
            "UPDATE oc_artifact_ledger SET source_id=?,artifact_type='chunk'",
        ] {
            let query = db::query(sql);
            let query = if sql.contains('?') {
                query.bind(evidence.source_event_id)
            } else {
                query
            };
            query.execute(&mut *tx.tx).await.unwrap();
            assert!(matches!(
                tx.archive_evidence_local(&evidence.reference, evidence.evidence_ref_id).await,
                Err(StorageError::NotFound)
            ));
        }
        db::query("UPDATE oc_artifact_ledger SET artifact_type=?")
            .bind(artifact_type)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        assert!(tx.archive_evidence_local(&evidence.reference, evidence.evidence_ref_id).await.is_ok());
        db::query("UPDATE oc_artifact_owners SET chunk_id=zeroblob(16) WHERE artifact_type='chunk'")
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        assert!(matches!(
            tx.archive_evidence_local(&evidence.reference, evidence.evidence_ref_id).await,
            Err(StorageError::NotFound)
        ));
        tx.tx.rollback().await.unwrap();
        pool.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn session_json_errors_remain_ordered_after_metadata_prefetch() {
    for first_visible in [false, true] {
        let (_dir, pool) = database().await;
        let scope = scope();
        let mut tx = transaction(&pool, scope).await;
        let first = seed(&mut tx, scope, 1).await;
        let second = seed(&mut tx, scope, 2).await;
        set_locator(&mut tx, &second.reference, "{").await;
        if !first_visible {
            db::query("DELETE FROM oc_artifact_owners WHERE source_id=?")
                .bind(first.source_event_id)
                .execute(&mut *tx.tx)
                .await
                .unwrap();
        }
        let (session, turn, _) = store_turn(&mut tx, &[first, second]).await;
        let result = tx.require_turn_visible_local(session, turn).await;
        if first_visible {
            assert!(matches!(
                result,
                Err(StorageError::Backend(_))
            ));
        } else {
            assert!(matches!(
                result,
                Err(StorageError::NotFound)
            ));
        }
        // An engine JSON error interrupts this lease. Never issue another
        // statement on it before rollback; preserve the returned Backend.
        tx.tx.rollback().await.unwrap();
        pool.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn session_content_decoding_retains_member_before_root_order() {
    for member_exists in [false, true] {
        let (_dir, pool) = database().await;
        let scope = scope();
        let mut tx = transaction(&pool, scope).await;
        let evidence = seed(&mut tx, scope, 1).await;
        let member = Uuid::new_v4();
        if member_exists {
            event(&mut tx, scope, member, "member").await;
        }
        let locator = json!({"parser": crate::memory_semantics::PARSER, "source_event_id": member, "source_version": 7, "support_sources": []}).to_string();
        set_locator(&mut tx, &evidence.reference, &locator).await;
        db::query("UPDATE oc_events SET content=x'31' WHERE id=?")
            .bind(evidence.source_event_id)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        let result = tx.archive_evidence_local(&evidence.reference, evidence.evidence_ref_id).await;
        if member_exists {
            assert!(matches!(
                result,
                Err(StorageError::Backend(_))
            ));
        } else {
            assert!(matches!(
                result,
                Err(StorageError::NotFound)
            ));
        }
        tx.tx.rollback().await.unwrap();
        pool.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn session_available_retains_global_any_table_or_source_probe() {
    let (_dir, pool) = database().await;
    let mut conn = pool.acquire().await.unwrap();
    assert!(session::available(&mut conn).await.unwrap());
    for sql in ["DROP TABLE oc_session_turns", "DROP TABLE oc_session_turn_evidence"] {
        db::query(sql)
            .execute(&mut *conn)
            .await
            .unwrap();
    }
    assert!(!session::available(&mut conn).await.unwrap());
    db::query("INSERT INTO oc_events VALUES(?,?,?,'withdrawn','session_turn','historical')")
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .execute(&mut *conn)
        .await
        .unwrap();
    assert!(session::available(&mut conn).await.unwrap());
    db::query("CREATE TABLE oc_feedback(id INTEGER)")
        .execute(&mut *conn)
        .await
        .unwrap();
    db::query("DROP TABLE oc_events")
        .execute(&mut *conn)
        .await
        .unwrap();
    // A present archive table does not short-circuit the event-table prepare.
    assert!(matches!(
        session::available(&mut conn).await,
        Err(StorageError::Backend(_))
    ));
    drop(conn);
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn session_turn_root_native_filter_precedes_uuid_decode() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let (session, turn, _) = store_turn(&mut tx, &[]).await;
    db::query("INSERT INTO oc_events VALUES(?,?,x'31','withdrawn','ordinary','short root')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_session_turns SET source_event_id=x'31'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(matches!(
        tx.require_turn_visible_local(session, turn).await,
        Err(StorageError::NotFound)
    ));
    db::query("UPDATE oc_events SET state='active' WHERE id=x'31'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(matches!(
        tx.require_turn_visible_local(session, turn).await,
        Err(StorageError::Backend(_))
    ));
    tx.tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}
