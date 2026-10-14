//! Native session windows and ordered raw metadata regression coverage.
use super::*;
use crate::storage::native_turso as db;
use crate::storage::{AuthorizedScope, RelationalStore};

async fn database() -> (tempfile::TempDir, TursoStore, AuthorizedScope) {
    let dir = tempfile::tempdir().unwrap();
    let store = TursoStore::open(dir.path().join("context-session.db"))
        .await
        .unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "context")
        .await
        .unwrap();
    let key = store.issue_key(scope, "admin").await.unwrap();
    let auth = store.authenticate(&key.token).await.unwrap();
    (dir, store, auth)
}

async fn turn(
    tx: &mut TursoTx,
    session: Uuid,
    id: Uuid,
    created: i64,
    kind: &str,
) -> Uuid {
    let source = tx
        .create_event_inner(kind, r#"{"question":"needle","answer":"answer"}"#, None)
        .await
        .unwrap();
    db::query("INSERT INTO oc_session_turns(tenant_id,workspace_id,session_id,turn_id,source_event_id,request_hash,created_by,created_at) VALUES(?,?,?,?,?,'test',?,?)")
        .bind(tx.scope.tenant_id)
        .bind(tx.scope.workspace_id)
        .bind(session)
        .bind(id)
        .bind(source)
        .bind(tx.principal_id)
        .bind(created)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    source
}

async fn body(tx: &mut TursoTx, source: Uuid, value: &str) {
    db::query("UPDATE oc_events SET content=? WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(value)
        .bind(tx.scope.tenant_id)
        .bind(tx.scope.workspace_id)
        .bind(source)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
}

async fn evidence(
    tx: &mut TursoTx,
    session: Uuid,
    turn: Uuid,
    ordinal: i64,
    value: &str,
) {
    db::query("INSERT INTO oc_session_turn_evidence(tenant_id,workspace_id,session_id,turn_id,evidence_ref_id,ordinal,evidence) VALUES(?,?,?,?,?,?,?)")
        .bind(tx.scope.tenant_id)
        .bind(tx.scope.workspace_id)
        .bind(session)
        .bind(turn)
        .bind(Uuid::new_v4())
        .bind(ordinal)
        .bind(value)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
}

#[tokio::test]
async fn recent_filters_sources_before_window_and_preserves_tie_order() {
    let (_dir, store, auth) = database().await;
    let mut tx = store.begin(auth).await.unwrap();
    let session = Uuid::from_u128(1);
    for index in 1..=514 {
        let kind = if index % 3 == 0 {
            "knowledge"
        } else {
            "session_turn"
        };
        let source = turn(&mut tx, session, Uuid::from_u128(index), 99, kind).await;
        body(&mut tx, source, "malformed").await;
        if index % 3 == 1 {
            db::query("UPDATE oc_events SET state='retracted' WHERE tenant_id=? AND workspace_id=? AND id=?")
                .bind(tx.scope.tenant_id)
                .bind(tx.scope.workspace_id)
                .bind(source)
                .execute(&mut *tx.tx)
                .await
                .unwrap();
        } else if index % 3 == 2 {
            db::query("DELETE FROM oc_events WHERE tenant_id=? AND workspace_id=? AND id=?")
                .bind(tx.scope.tenant_id)
                .bind(tx.scope.workspace_id)
                .bind(source)
                .execute(&mut *tx.tx)
                .await
                .unwrap();
        }
    }
    let second = Uuid::from_u128(1002);
    let first = Uuid::from_u128(1001);
    turn(&mut tx, session, second, 1, "session_turn").await;
    turn(&mut tx, session, first, 1, "session_turn").await;
    let result = tx
        .context_recent_turns("needle", None, None, 10)
        .await
        .unwrap();
    assert_eq!(result.scanned, 2);
    assert!(!result.scan_exhausted);
    assert_eq!(
        result.turns.iter().map(|turn| turn.turn_id).collect::<Vec<_>>(),
        vec![first, second]
    );
    let empty = tx
        .context_recent_turns("needle", Some(session), Some(session), 10)
        .await
        .unwrap();
    assert_eq!(empty.scanned, 0);
    tx.rollback().await.unwrap();
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn recent_decodes_lookahead_keys_but_not_body_and_checks_after_result_limit() {
    let (_dir, store, auth) = database().await;
    let mut tx = store.begin(auth).await.unwrap();
    let session = Uuid::from_u128(1);
    let mut sources = Vec::new();
    for index in 1..=513 {
        sources.push(turn(&mut tx, session, Uuid::from_u128(index), 1, "session_turn").await);
    }
    body(&mut tx, sources[512], "malformed lookahead body").await;
    let result = tx
        .context_recent_turns("needle", None, None, 1)
        .await
        .unwrap();
    assert_eq!(result.scanned, 512);
    assert!(result.scan_exhausted);
    assert_eq!(result.turns.len(), 1);
    let chunk = Uuid::new_v4();
    let root = Uuid::new_v4();
    let missing = json!({
        "evidence_ref_id":Uuid::new_v4(),
        "reference":{"kind":"chunk","id":chunk,"asset_id":Uuid::new_v4(),"asset_version":1,"chunk_id":chunk},
        "source_event_id":root,"source_version":1,"version_source_event_id":root,"locator":{}
    })
    .to_string();
    evidence(&mut tx, session, Uuid::from_u128(1), 0, &missing).await;
    let hidden = tx
        .context_recent_turns("needle", None, None, 512)
        .await
        .unwrap();
    assert_eq!(hidden.scanned, 512);
    assert_eq!(hidden.turns.len(), 511);
    // The hidden dependency consumes a slot; malformed lookahead stays unread.
    body(&mut tx, sources[511], "malformed after full response").await;
    assert!(matches!(
        tx.context_recent_turns("needle", None, None, 1).await,
        Err(StorageError::Unavailable(_))
    ));
    body(
        &mut tx,
        sources[511],
        r#"{"question":"needle","answer":"answer"}"#,
    )
    .await;
    db::query("UPDATE oc_session_turns SET turn_id=? WHERE tenant_id=? AND workspace_id=? AND source_event_id=?")
        .bind(vec![0xff_u8])
        .bind(tx.scope.tenant_id)
        .bind(tx.scope.workspace_id)
        .bind(sources[512])
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(matches!(
        tx.context_recent_turns("needle", None, None, 1).await,
        Err(StorageError::Backend(_))
    ));
    tx.rollback().await.unwrap();
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn context_turn_batch_preserves_wrong_kind_and_evidence_decode_order() {
    let (_dir, store, auth) = database().await;
    let mut tx = store.begin(auth).await.unwrap();
    let session = Uuid::new_v4();
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    turn(&mut tx, session, first, 1, "knowledge").await;
    turn(&mut tx, session, second, 2, "session_turn").await;
    evidence(&mut tx, session, first, 0, "bad JSON").await;
    // Direct Context validates archive JSON before its additional kind gate.
    assert!(matches!(
        tx.context_session_turns(&[(session, first)]).await,
        Err(StorageError::Unavailable(_))
    ));
    // Recent excludes the wrong-kind turn before opening its archive JSON.
    assert_eq!(tx.context_recent_turns("needle", None, None, 2).await.unwrap().turns.len(), 1);
    for ordinal in 0..65 {
        evidence(&mut tx, session, second, ordinal, "not JSON").await;
    }
    // All text decodes successfully, then 65 fails closed before JSON parsing.
    let result = tx.context_session_turns(&[(session, second)]).await.unwrap();
    assert!(result[0].is_none());
    db::query("UPDATE oc_session_turn_evidence SET evidence=? WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=? AND ordinal=64")
        .bind(vec![0xff_u8])
        .bind(tx.scope.tenant_id)
        .bind(tx.scope.workspace_id)
        .bind(session)
        .bind(second)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    // String decoding of the entire 65 window precedes its size rejection.
    assert!(matches!(
        tx.context_session_turns(&[(session, second)]).await,
        Err(StorageError::Backend(_))
    ));
    tx.rollback().await.unwrap();
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn empty_context_turn_batch_does_not_require_writer() {
    let (_dir, store, admin) = database().await;
    let key = store.issue_key(admin.scope, "reader").await.unwrap();
    let reader = store.authenticate(&key.token).await.unwrap();
    let mut tx = store.begin(reader).await.unwrap();
    assert!(tx.context_session_turns(&[]).await.unwrap().is_empty());
    assert!(matches!(
        tx.context_session_turns(&[(Uuid::new_v4(), Uuid::new_v4())]).await,
        Err(StorageError::Forbidden)
    ));
    tx.rollback().await.unwrap();
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn hidden_turns_do_not_prepare_final_body_or_timestamp_columns() {
    let dir = tempfile::tempdir().unwrap();
    let pool = Pool::open(dir.path().join("missing-final-columns.db")).await.unwrap();
    for sql in [
        "CREATE TABLE oc_session_turns(tenant_id BLOB,workspace_id BLOB,session_id BLOB,turn_id BLOB,source_event_id BLOB)",
        "CREATE TABLE oc_events(tenant_id BLOB,workspace_id BLOB,id BLOB,state TEXT,kind TEXT)",
        "CREATE TABLE oc_session_turn_evidence(tenant_id BLOB,workspace_id BLOB,session_id BLOB,turn_id BLOB,ordinal INTEGER,evidence TEXT)",
    ] {
        db::query(sql).execute(&pool).await.unwrap();
    }
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    let session = Uuid::new_v4();
    let turn = Uuid::new_v4();
    let source = Uuid::new_v4();
    db::query("INSERT INTO oc_session_turns VALUES(?,?,?,?,?)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(session)
        .bind(turn)
        .bind(source)
        .execute(&pool)
        .await
        .unwrap();
    db::query("INSERT INTO oc_events VALUES(?,?,?,'retracted','session_turn')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(source)
        .execute(&pool)
        .await
        .unwrap();
    let mut tx = TursoTx {
        tx: pool.begin().await.unwrap(),
        scope,
        principal_id: Uuid::nil(),
        required: Vec::new(),
        command: None,
    };
    let mut batch = tx.context_turn_metadata(&[(session, turn)]).await.unwrap();
    assert!(tx.context_turn_from_metadata(&mut batch, 0).await.unwrap().is_none());
    db::query("UPDATE oc_events SET state='active'").execute(&mut *tx.tx).await.unwrap();
    for ordinal in 0..65_i64 {
        db::query("INSERT INTO oc_session_turn_evidence VALUES(?,?,?,?,?,'bad JSON')")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(session)
            .bind(turn)
            .bind(ordinal)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
    }
    let mut batch = tx.context_turn_metadata(&[(session, turn)]).await.unwrap();
    assert!(tx.context_turn_from_metadata(&mut batch, 0).await.unwrap().is_none());
    tx.tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn body_decode_follows_native_content_then_timestamp_then_json_order() {
    let (_dir, store, auth) = database().await;
    let mut tx = store.begin(auth).await.unwrap();
    let session = Uuid::new_v4();
    let id = Uuid::new_v4();
    let source = turn(&mut tx, session, id, 1, "session_turn").await;
    let file = tx
        .insert_file_inner("session.txt", "text/plain", "hash", 1)
        .await
        .unwrap();
    db::query("UPDATE oc_events SET file_id=? WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(file)
        .bind(tx.scope.tenant_id)
        .bind(tx.scope.workspace_id)
        .bind(source)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_files SET deleted=1 WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(tx.scope.tenant_id)
        .bind(tx.scope.workspace_id)
        .bind(file)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(matches!(tx.event_view_inner(source).await, Err(StorageError::NotFound)));
    // Context's session projection has no public event-file predicate.
    assert!(
        tx.context_session_turns(&[(session, id)]).await.unwrap()[0].is_some()
    );
    body(&mut tx, source, "bad JSON").await;
    db::query("UPDATE oc_session_turns SET created_at='not an integer' WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=?")
        .bind(tx.scope.tenant_id)
        .bind(tx.scope.workspace_id)
        .bind(session)
        .bind(id)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(matches!(
        tx.context_session_turns(&[(session, id)]).await,
        Err(StorageError::Backend(message)) if message.contains("expected integer")
    ));
    db::query("UPDATE oc_events SET content=? WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(vec![0xff_u8])
        .bind(tx.scope.tenant_id)
        .bind(tx.scope.workspace_id)
        .bind(source)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(matches!(
        tx.context_session_turns(&[(session, id)]).await,
        Err(StorageError::Backend(message)) if message.contains("expected text")
    ));
    tx.rollback().await.unwrap();
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn evidence_pages_keep_each_turns_65_row_window_across_batch_boundaries() {
    let (_dir, store, auth) = database().await;
    let mut tx = store.begin(auth).await.unwrap();
    let session = Uuid::new_v4();
    let mut keys = Vec::new();
    for index in 1..=9 {
        let id = Uuid::from_u128(index);
        turn(&mut tx, session, id, 1, "session_turn").await;
        for ordinal in 0..66 {
            evidence(&mut tx, session, id, ordinal, "invalid JSON").await;
        }
        // The 66th raw value must never be String-decoded.
        db::query("UPDATE oc_session_turn_evidence SET evidence=? WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=? AND ordinal=65")
            .bind(vec![0xff_u8])
            .bind(tx.scope.tenant_id)
            .bind(tx.scope.workspace_id)
            .bind(session)
            .bind(id)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        keys.push((session, id));
    }
    let result = tx.context_session_turns(&keys).await.unwrap();
    assert_eq!(result.len(), 9);
    assert!(result.iter().all(Option::is_none));
    tx.rollback().await.unwrap();
    store.shutdown().await.unwrap();
}
