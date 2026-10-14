//! Native-engine regression fixtures for the batched authority read model.
//! These call internal reads directly; public authentication is covered by the
//! existing domain tests. No previous multi-table SQL is used as an oracle.
#![cfg(feature = "local-storage")]

use super::read_model::{NativeHitKey, current_chunks, rows_by_ids};
use super::*;
use crate::storage::native_turso as db;

// Test-only composition of the production read primitives. Session archives
// now use their batched guard, exercised directly in session_read_tests.
async fn active_supported_version(
    tx: &mut TursoTx,
    asset: Uuid,
    version: i32,
) -> StorageResult<Uuid> {
    let root = tx
        .active_version_roots(&[(asset, version)])
        .await?
        .get(&(asset, version))
        .copied()
        .ok_or(StorageError::NotFound)?;
    if !tx.semantic_version_visible_local(asset, version).await? {
        return Err(StorageError::NotFound);
    }
    Ok(root)
}

#[derive(Clone, Copy)]
struct Record {
    asset: Uuid,
    chunk: Uuid,
    source: Uuid,
    version: i32,
}

impl Record {
    fn new(index: u128) -> Self {
        Self {
            asset: Uuid::from_u128(10_000 + index),
            chunk: Uuid::from_u128(20_000 + index),
            source: Uuid::from_u128(30_000 + index),
            version: 1,
        }
    }

    fn key(self, generation: i64) -> NativeHitKey {
        (
            self.chunk,
            SourceVersion {
                source_id: self.source,
                version: self.version,
            },
            generation,
        )
    }
}

async fn database() -> (tempfile::TempDir, Pool) {
    let dir = tempfile::tempdir().unwrap();
    let pool = Pool::open(dir.path().join("read-model.db")).await.unwrap();
    // Preserve scoped keys and relevant columns. Unrelated authentication and
    // publication tables are omitted; missing-root fixtures intentionally omit
    // foreign keys so the fail-closed read path can be exercised directly.
    for sql in [
        "CREATE TABLE oc_assets(tenant_id BLOB NOT NULL,workspace_id BLOB NOT NULL,id BLOB NOT NULL,kind TEXT NOT NULL,title TEXT NOT NULL,current_version INTEGER,deleted INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(tenant_id,workspace_id,id))",
        "CREATE TABLE oc_versions(tenant_id BLOB NOT NULL,workspace_id BLOB NOT NULL,asset_id BLOB NOT NULL,version INTEGER NOT NULL,title TEXT NOT NULL,content TEXT NOT NULL,content_hash TEXT NOT NULL,source_event_id BLOB NOT NULL,restored_from INTEGER,PRIMARY KEY(tenant_id,workspace_id,asset_id,version))",
        "CREATE TABLE oc_events(tenant_id BLOB NOT NULL,workspace_id BLOB NOT NULL,id BLOB NOT NULL,state TEXT NOT NULL,kind TEXT NOT NULL,PRIMARY KEY(tenant_id,workspace_id,id))",
        "CREATE TABLE oc_chunks(tenant_id BLOB NOT NULL,workspace_id BLOB NOT NULL,id BLOB NOT NULL,asset_id BLOB NOT NULL,version INTEGER NOT NULL,ordinal INTEGER NOT NULL,content TEXT NOT NULL,locator TEXT NOT NULL,search_terms TEXT NOT NULL,PRIMARY KEY(tenant_id,workspace_id,id))",
        "CREATE TABLE oc_artifact_ledger(tenant_id BLOB NOT NULL,workspace_id BLOB NOT NULL,artifact_id BLOB NOT NULL,artifact_type TEXT NOT NULL,source_id BLOB NOT NULL,version INTEGER NOT NULL,generation INTEGER NOT NULL,surface TEXT NOT NULL,state TEXT NOT NULL,PRIMARY KEY(tenant_id,workspace_id,source_id,version,artifact_id,surface,generation))",
        "CREATE TABLE oc_index_entries(tenant_id BLOB NOT NULL,workspace_id BLOB NOT NULL,artifact_id BLOB NOT NULL,field TEXT NOT NULL,model_id TEXT NOT NULL,dimension INTEGER NOT NULL,generation INTEGER NOT NULL,state TEXT NOT NULL,PRIMARY KEY(tenant_id,workspace_id,artifact_id,field,model_id,generation))",
    ] {
        db::query(sql).execute(&pool).await.unwrap();
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

async fn seed(
    tx: &mut TursoTx,
    scope: Scope,
    row: Record,
    current: Option<i32>,
    root_state: Option<&str>,
    content: &str,
) {
    db::query("INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,current_version) VALUES(?,?,?,'knowledge','asset title',?) ON CONFLICT(tenant_id,workspace_id,id) DO NOTHING")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(row.asset)
        .bind(current)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    if let Some(state) = root_state {
        db::query("INSERT INTO oc_events VALUES(?,?,?,?,'knowledge')")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(row.source)
            .bind(state)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
    }
    db::query("INSERT INTO oc_versions VALUES(?,?,?,?,'version title',?,'hash',?,NULL)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(row.asset)
        .bind(row.version)
        .bind(content)
        .bind(row.source)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("INSERT INTO oc_chunks VALUES(?,?,?,?,?,0,?,'{}','needle')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(row.chunk)
        .bind(row.asset)
        .bind(row.version)
        .bind(content)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
}

async fn publication(
    tx: &mut TursoTx,
    scope: Scope,
    key: NativeHitKey,
    surface: &str,
    state: &str,
    artifact_type: &str,
) {
    db::query("INSERT INTO oc_artifact_ledger VALUES(?,?,?,?,?,?,?,?,?)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(key.0)
        .bind(artifact_type)
        .bind(key.1.source_id)
        .bind(key.1.version)
        .bind(key.2)
        .bind(surface)
        .bind(state)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
}

async fn ready_index(tx: &mut TursoTx, row: Record, field: &str, generation: i64) {
    db::query("INSERT INTO oc_index_entries VALUES(?,?,?,?,'profile',3,?,'ready')")
        .bind(tx.scope.tenant_id)
        .bind(tx.scope.workspace_id)
        .bind(row.chunk)
        .bind(field)
        .bind(generation)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
}

async fn locator(tx: &mut TursoTx, chunk: Uuid, value: &str) {
    db::query("UPDATE oc_chunks SET locator=? WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(value)
        .bind(tx.scope.tenant_id)
        .bind(tx.scope.workspace_id)
        .bind(chunk)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
}

#[tokio::test]
async fn batched_reads_cross_id_and_version_boundaries_without_truncation() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let records: Vec<_> = (0..130).map(Record::new).collect();
    for row in &records {
        seed(&mut tx, scope, *row, Some(1), Some("active"), "needle").await;
        locator(
            &mut tx,
            row.chunk,
            &json!({
                "parser": "memory-semantics:v1",
                "support_sources": [row.source],
            })
            .to_string(),
        )
        .await;
        for surface in ["full_text", "vector"] {
            publication(&mut tx, scope, row.key(11), surface, "committed", "chunk").await;
        }
        ready_index(&mut tx, *row, "content", 11).await;
    }
    // Unrequested historical JSON must not be evaluated by an exact-version
    // batch, even when the asset also has an eligible current chunk.
    db::query("INSERT INTO oc_chunks VALUES(?,?,?,?,999,0,'old','{','old')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(Uuid::new_v4())
        .bind(records[0].asset)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    let mut ids: Vec<_> = records.iter().map(|row| row.chunk).collect();
    ids.extend([records[0].chunk, records[128].chunk, records[0].chunk]);
    let mut versions: Vec<_> = records.iter().map(|row| (row.asset, row.version)).collect();
    versions.extend([
        (records[0].asset, 1),
        (records[32].asset, 1),
        (records[128].asset, 1),
    ]);
    let mut keys: Vec<_> = records.iter().map(|row| row.key(11)).collect();
    keys.push(records[128].key(11));
    let hits = tx.visible_hits(&ids).await.unwrap();
    let roots = tx.active_version_roots(&versions).await.unwrap();
    let support = tx.local_support_visibility(&versions).await.unwrap();
    let full_text = tx.full_text_hits(&keys).await.unwrap();
    let vector = tx.vector_hits_exact(&keys, "profile", 3).await.unwrap();
    assert_eq!(hits.len(), 130);
    assert_eq!(roots.len(), 130);
    assert_eq!(support.len(), 130);
    assert_eq!(full_text.len(), 130);
    assert_eq!(vector.len(), 130);
    for row in records {
        assert_eq!(hits[&row.chunk].asset_id, row.asset);
        assert_eq!(hits[&row.chunk].source_event_id, row.source);
        assert_eq!(roots[&(row.asset, row.version)], row.source);
        assert!(support[&(row.asset, row.version)]);
        assert_eq!(full_text[&row.key(11)].chunk_id, row.chunk);
        assert_eq!(vector[&row.key(11)].chunk_id, row.chunk);
    }
    assert!(tx.visible_hits(&[]).await.unwrap().is_empty());
    assert!(tx.active_version_roots(&[]).await.unwrap().is_empty());
    assert!(tx.local_support_visibility(&[]).await.unwrap().is_empty());
    assert!(tx.full_text_hits(&[]).await.unwrap().is_empty());
    assert!(
        tx.vector_hits_exact(&[], "profile", 3)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(tx.native_candidate_sources(&[]).await.unwrap().is_empty());
    assert!(
        rows_by_ids(&mut tx.tx, scope, "oc_chunks", "id", "id", &[])
            .await
            .unwrap()
            .is_empty()
    );
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn same_ids_in_other_scopes_cannot_supply_missing_or_inactive_roots() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let scopes = [
        scope,
        Scope {
            workspace_id: Uuid::new_v4(),
            ..scope
        },
        Scope {
            tenant_id: Uuid::new_v4(),
            ..scope
        },
    ];
    let missing = Record::new(1);
    let inactive = Record::new(2);
    let mut tx = transaction(&pool, scope).await;
    for (index, other) in scopes.into_iter().enumerate() {
        seed(
            &mut tx,
            other,
            missing,
            Some(1),
            (index != 0).then_some("active"),
            "missing root",
        )
        .await;
        seed(
            &mut tx,
            other,
            inactive,
            Some(1),
            Some(if index == 0 { "retracted" } else { "active" }),
            "inactive root",
        )
        .await;
        for row in [missing, inactive] {
            publication(
                &mut tx,
                other,
                row.key(11),
                "full_text",
                "committed",
                "chunk",
            )
            .await;
        }
    }
    let ids = [missing.chunk, inactive.chunk];
    assert!(tx.visible_hits(&ids).await.unwrap().is_empty());
    assert!(
        tx.active_version_roots(&[(missing.asset, 1), (inactive.asset, 1)])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        tx.full_text_hits(&[missing.key(11), inactive.key(11)])
            .await
            .unwrap()
            .is_empty()
    );
    for other in [scopes[1], scopes[2]] {
        let chunks = current_chunks(&mut tx.tx, other, Some(&ids), None)
            .await
            .unwrap();
        assert_eq!(chunks.len(), 2);
    }
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn historical_roots_and_views_do_not_acquire_a_current_version_guard() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let first = Record::new(1);
    let second = Record {
        asset: first.asset,
        version: 2,
        ..Record::new(2)
    };
    seed(&mut tx, scope, first, Some(2), Some("active"), "historical").await;
    seed(&mut tx, scope, second, Some(2), Some("active"), "current").await;
    let roots = tx
        .active_version_roots(&[(first.asset, 1), (first.asset, 2)])
        .await
        .unwrap();
    assert_eq!(roots.len(), 2);
    assert!(
        tx.active_version_roots(&[(first.asset, 99)])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        tx.semantic_version_visible_local(first.asset, 99)
            .await
            .unwrap()
    );
    assert!(tx.semantic_version_visible(first.asset, 1).await.unwrap());
    assert!(tx.semantic_version_visible(first.asset, 99).await.unwrap());
    assert!(matches!(
        active_supported_version(&mut tx, first.asset, 99).await,
        Err(StorageError::NotFound)
    ));
    assert_eq!(
        active_supported_version(&mut tx, first.asset, 1)
            .await
            .unwrap(),
        first.source
    );
    let current = tx.visible_hits(&[first.chunk, second.chunk]).await.unwrap();
    assert_eq!(current.len(), 1);
    assert!(current.contains_key(&second.chunk));
    assert_eq!(
        tx.asset_view_inner(first.asset, None).await.unwrap()["content"],
        "current"
    );
    assert_eq!(
        tx.asset_view_inner(first.asset, Some(1)).await.unwrap()["content"],
        "historical"
    );
    let restore = tx.restore_source_inner(first.asset, 1).await.unwrap();
    assert_eq!(restore["current_version"], 2);
    assert_eq!(restore["source_event_id"], first.source.to_string());
    db::query(
        "UPDATE oc_assets SET current_version=NULL WHERE tenant_id=? AND workspace_id=? AND id=?",
    )
    .bind(scope.tenant_id)
    .bind(scope.workspace_id)
    .bind(first.asset)
    .execute(&mut *tx.tx)
    .await
    .unwrap();
    assert!(matches!(
        tx.asset_view_inner(first.asset, None).await,
        Err(StorageError::NotFound)
    ));
    assert_eq!(
        tx.asset_view_inner(first.asset, Some(1)).await.unwrap()["content"],
        "historical"
    );
    assert!(tx.restore_source_inner(first.asset, 1).await.unwrap()["current_version"].is_null());
    db::query(
        "UPDATE oc_events SET state='retracted' WHERE tenant_id=? AND workspace_id=? AND id=?",
    )
    .bind(scope.tenant_id)
    .bind(scope.workspace_id)
    .bind(first.source)
    .execute(&mut *tx.tx)
    .await
    .unwrap();
    assert!(matches!(
        active_supported_version(&mut tx, first.asset, 1).await,
        Err(StorageError::NotFound)
    ));
    assert!(matches!(
        tx.restore_source_inner(first.asset, 1).await,
        Err(StorageError::NotFound)
    ));
    db::query("UPDATE oc_assets SET deleted=1 WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(first.asset)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(
        tx.active_version_roots(&[(first.asset, 2)])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        tx.asset_view_inner(first.asset, Some(2)).await,
        Err(StorageError::NotFound)
    ));
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn repeated_reads_observe_writes_in_the_same_transaction() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let row = Record::new(1);
    seed(&mut tx, scope, row, Some(1), Some("active"), "before").await;
    assert_eq!(
        tx.visible_hits(&[row.chunk]).await.unwrap()[&row.chunk].content,
        "before"
    );
    db::query(
        "UPDATE oc_events SET state='retracted' WHERE tenant_id=? AND workspace_id=? AND id=?",
    )
    .bind(scope.tenant_id)
    .bind(scope.workspace_id)
    .bind(row.source)
    .execute(&mut *tx.tx)
    .await
    .unwrap();
    assert!(tx.visible_hits(&[row.chunk]).await.unwrap().is_empty());
    db::query("UPDATE oc_events SET state='active' WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(row.source)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_chunks SET content='after' WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(row.chunk)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert_eq!(
        tx.visible_hits(&[row.chunk]).await.unwrap()[&row.chunk].content,
        "after"
    );
    db::query(
        "UPDATE oc_assets SET current_version=2 WHERE tenant_id=? AND workspace_id=? AND id=?",
    )
    .bind(scope.tenant_id)
    .bind(scope.workspace_id)
    .bind(row.asset)
    .execute(&mut *tx.tx)
    .await
    .unwrap();
    assert!(tx.visible_hits(&[row.chunk]).await.unwrap().is_empty());
    assert_eq!(
        tx.active_version_roots(&[(row.asset, 1)]).await.unwrap()[&(row.asset, 1)],
        row.source
    );
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_hit_batches_require_exact_committed_publication_keys() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let records: Vec<_> = (0..9).map(Record::new).collect();
    let mut keys = Vec::new();
    for (index, row) in records.iter().copied().enumerate() {
        seed(&mut tx, scope, row, Some(1), Some("active"), "needle").await;
        ready_index(&mut tx, row, "content", 11).await;
        keys.push(row.key(11));
        if index == 1 {
            continue; // No ledger entry.
        }
        let mut key = row.key(11);
        let state = match index {
            2 => {
                key.1.source_id = Uuid::new_v4();
                "committed"
            }
            3 => {
                key.1.version = 2;
                "committed"
            }
            4 => {
                key.2 = 12;
                "committed"
            }
            5 => "pending",
            6 => "retry_wait",
            7 => "orphan",
            _ => "committed",
        };
        if index == 2 || index == 3 {
            // Even a key matching the ledger cannot replace the chunk's root.
            keys.push(key);
        }
        if index == 8 {
            publication(&mut tx, scope, key, "graph", state, "summary").await;
        } else {
            for surface in ["full_text", "vector"] {
                // The original native hit/candidate checks did not restrict type.
                publication(&mut tx, scope, key, surface, state, "summary").await;
            }
        }
    }
    let good = records[0];
    keys.push(good.key(11));
    let mut wrong_source = good.key(11);
    wrong_source.1.source_id = Uuid::new_v4();
    let mut wrong_version = good.key(11);
    wrong_version.1.version = 2;
    keys.extend([wrong_source, wrong_version, good.key(12)]);
    for hits in [
        tx.full_text_hits(&keys).await.unwrap(),
        tx.vector_hits_exact(&keys, "profile", 3).await.unwrap(),
    ] {
        assert_eq!(hits.len(), 1);
        assert!(hits.contains_key(&good.key(11)));
    }
    assert_eq!(
        tx.full_text_candidates(11, None).await.unwrap(),
        vec![good.chunk]
    );
    assert_eq!(
        tx.vector_candidates("profile", 3, 11, None).await.unwrap(),
        vec![good.chunk]
    );
    assert!(
        tx.vector_hits_exact(&[good.key(11)], "wrong-profile", 3)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        tx.vector_hits_exact(&[good.key(11)], "profile", 4)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        tx.full_text_hits(&[records[4].key(12)])
            .await
            .unwrap()
            .contains_key(&records[4].key(12))
    );
    assert!(
        tx.vector_hits_exact(&[records[4].key(12)], "profile", 3)
            .await
            .unwrap()
            .is_empty()
    );
    db::query("UPDATE oc_artifact_ledger SET state='pending' WHERE tenant_id=? AND workspace_id=? AND artifact_id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(good.chunk)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(tx.full_text_hits(&[good.key(11)]).await.unwrap().is_empty());
    assert!(
        tx.vector_hits_exact(&[good.key(11)], "profile", 3)
            .await
            .unwrap()
            .is_empty()
    );
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn legacy_vector_ready_fields_are_distinct_from_exact_content_checks() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let row = Record::new(1);
    seed(&mut tx, scope, row, Some(1), Some("active"), "needle").await;
    publication(
        &mut tx,
        scope,
        row.key(11),
        "vector",
        "committed",
        "summary",
    )
    .await;
    ready_index(&mut tx, row, "summary", 11).await;
    assert_eq!(
        tx.vector_candidates("profile", 3, 11, None).await.unwrap(),
        vec![row.chunk]
    );
    assert!(
        tx.vector_hit(row.chunk, row.key(11).1, "profile", 11)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        tx.vector_hits_exact(&[row.key(11)], "profile", 3)
            .await
            .unwrap()
            .is_empty()
    );
    ready_index(&mut tx, row, "content", 11).await;
    assert_eq!(
        tx.vector_hits_exact(&[row.key(11)], "profile", 3)
            .await
            .unwrap()
            .len(),
        1
    );
    db::query("UPDATE oc_index_entries SET state='removed' WHERE tenant_id=? AND workspace_id=? AND artifact_id=? AND field='content'")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(row.chunk)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(
        tx.vector_hits_exact(&[row.key(11)], "profile", 3)
            .await
            .unwrap()
            .is_empty()
    );
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_sources_check_only_requested_chunks_without_publication_guards() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let row = Record::new(1);
    seed(&mut tx, scope, row, Some(1), Some("active"), "needle").await;
    let sibling = Uuid::new_v4();
    db::query("INSERT INTO oc_chunks VALUES(?,?,?,?,1,1,'sibling',?,'needle')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(sibling)
        .bind(row.asset)
        .bind(
            json!({
                "parser": "memory-semantics:v1",
                "support_sources": [Uuid::new_v4()],
            })
            .to_string(),
        )
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    // No ledger exists, and another chunk makes the whole version invisible.
    // This narrower helper must still return the requested chunk's active root.
    let expected = HashMap::from([(row.chunk, row.key(11).1)]);
    assert_eq!(
        tx.native_candidate_sources(&[row.chunk, row.chunk])
            .await
            .unwrap(),
        expected
    );
    assert!(
        !tx.semantic_version_visible_local(row.asset, 1)
            .await
            .unwrap()
    );
    assert!(matches!(
        tx.native_candidate_sources(&[row.chunk, sibling]).await,
        Err(StorageError::Unavailable(_))
    ));
    locator(&mut tx, sibling, "{").await;
    assert_eq!(
        tx.native_candidate_sources(&[row.chunk]).await.unwrap(),
        expected
    );
    assert!(matches!(
        tx.semantic_version_visible_local(row.asset, 1).await,
        Err(StorageError::Backend(_))
    ));
    // A native statement error leaves this lease interrupted until rollback.
    // Isolate the next malformed-query case instead of writing on that lease.
    tx.rollback().await.unwrap();
    let mut tx = transaction(&pool, scope).await;
    seed(&mut tx, scope, row, Some(1), Some("active"), "needle").await;
    locator(&mut tx, row.chunk, "{").await;
    assert!(matches!(
        tx.native_candidate_sources(&[row.chunk]).await,
        Err(StorageError::Backend(_))
    ));
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn event_projection_preserves_missing_deleted_and_foreign_file_semantics() {
    let (_dir, pool) = database().await;
    for sql in [
        "ALTER TABLE oc_events ADD COLUMN content TEXT NOT NULL DEFAULT 'source'",
        "ALTER TABLE oc_events ADD COLUMN file_id BLOB",
        "ALTER TABLE oc_events ADD COLUMN created_at INTEGER NOT NULL DEFAULT 7",
        "CREATE TABLE oc_files(tenant_id BLOB,workspace_id BLOB,id BLOB,hash TEXT,media_type TEXT,deleted INTEGER)",
    ] {
        db::query(sql).execute(&pool).await.unwrap();
    }
    let scope = scope();
    let file = Uuid::new_v4();
    let source = Uuid::new_v4();
    let mut tx = transaction(&pool, scope).await;
    db::query("INSERT INTO oc_events(tenant_id,workspace_id,id,state,kind,file_id) VALUES(?,?,?,'active','knowledge',?)")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(source).bind(file)
        .execute(&mut *tx.tx).await.unwrap();
    for foreign in [
        Scope {
            workspace_id: Uuid::new_v4(),
            ..scope
        },
        Scope {
            tenant_id: Uuid::new_v4(),
            ..scope
        },
    ] {
        db::query("INSERT INTO oc_files VALUES(?,?,?,'foreign','text/plain',0)")
            .bind(foreign.tenant_id)
            .bind(foreign.workspace_id)
            .bind(file)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
    }
    assert!(matches!(
        tx.event_view_inner(source).await,
        Err(StorageError::NotFound)
    ));
    db::query("UPDATE oc_events SET content=? WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(vec![0xff_u8])
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(source)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(matches!(
        tx.event_view_inner(source).await,
        Err(StorageError::NotFound)
    ));
    db::query(
        "UPDATE oc_events SET content='source' WHERE tenant_id=? AND workspace_id=? AND id=?",
    )
    .bind(scope.tenant_id)
    .bind(scope.workspace_id)
    .bind(source)
    .execute(&mut *tx.tx)
    .await
    .unwrap();
    let missing = tx.event_file_inner(source).await.unwrap();
    assert_eq!(missing["content"], "source");
    assert_eq!(missing["file_id"], file.to_string());
    assert!(missing["hash"].is_null());
    assert!(missing["media_type"].is_null());
    db::query("INSERT INTO oc_files VALUES(?,?,?,'local','text/plain',0)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(file)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert_eq!(
        tx.event_view_inner(source).await.unwrap()["content"],
        "source"
    );
    assert_eq!(tx.event_file_inner(source).await.unwrap()["hash"], "local");
    // Change only the file flag: event activity cannot mask the file predicate.
    db::query("UPDATE oc_files SET deleted=1 WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(file)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(matches!(
        tx.event_view_inner(source).await,
        Err(StorageError::NotFound)
    ));
    assert_eq!(tx.event_file_inner(source).await.unwrap(), missing);
    db::query("UPDATE oc_events SET content=? WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(vec![0xff_u8])
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(source)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(matches!(
        tx.event_view_inner(source).await,
        Err(StorageError::NotFound)
    ));
    for malformed in [vec![0x31_u8], Vec::new()] {
        db::query("UPDATE oc_events SET file_id=? WHERE tenant_id=? AND workspace_id=? AND id=?")
            .bind(malformed)
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(source)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        assert!(matches!(
            tx.event_view_inner(source).await,
            Err(StorageError::NotFound)
        ));
    }
    db::query(
        "UPDATE oc_events SET content='source' WHERE tenant_id=? AND workspace_id=? AND id=?",
    )
    .bind(scope.tenant_id)
    .bind(scope.workspace_id)
    .bind(source)
    .execute(&mut *tx.tx)
    .await
    .unwrap();
    db::query("UPDATE oc_events SET file_id=NULL WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(source)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(tx.event_view_inner(source).await.unwrap()["file_id"].is_null());
    assert!(tx.event_file_inner(source).await.unwrap()["file_id"].is_null());
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn identity_kind_filter_precedes_native_key_decoding() {
    let (_dir, pool) = database().await;
    for sql in [
        "ALTER TABLE oc_assets ADD COLUMN fact_key TEXT",
        "CREATE TABLE oc_memory_identities(tenant_id BLOB,workspace_id BLOB,identity_key TEXT,asset_id BLOB,identity_json TEXT)",
    ] {
        db::query(sql).execute(&pool).await.unwrap();
    }
    let scope = scope();
    let identity: crate::memory_identity::MemoryIdentity = serde_json::from_value(json!({
        "subject":{"kind":"service","stable_id":"billing"},
        "predicate":"release.approval","context":{}
    }))
    .unwrap();
    let key = identity.key(scope).unwrap();
    let mut tx = transaction(&pool, scope).await;
    // Non-STRICT BLOB columns can hold a short key. A non-memory asset with
    // that key is filtered before the original SELECT decodes any UUID.
    db::query("INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title) VALUES(?,?,?,'knowledge','not memory')")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(vec![0x31_u8])
        .execute(&mut *tx.tx).await.unwrap();
    db::query("INSERT INTO oc_memory_identities VALUES(?,?,?,?,?)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(&key)
        .bind(vec![0x31_u8])
        .bind(serde_json::to_string(&identity).unwrap())
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,fact_key) VALUES(?,?,?,'memory','legacy',?)")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(Uuid::new_v4()).bind(&key)
        .execute(&mut *tx.tx).await.unwrap();
    assert!(matches!(tx.identity_slot_inner(&identity).await,
        Err(StorageError::Conflict(message)) if message == "memory identity conflicts with a legacy fact slot"));
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn root_only_reads_do_not_decode_render_fields_or_historical_current() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let row = Record::new(50_000);
    seed(&mut tx, scope, row, Some(1), Some("active"), "body").await;
    db::query("UPDATE oc_assets SET kind=x'01'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_versions SET title=x'02',content=x'03'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    let roots = super::read_model::current_chunk_roots(&mut tx.tx, scope, &[row.chunk])
        .await
        .unwrap();
    assert_eq!(roots[&row.chunk].source_id, row.source);
    db::query("UPDATE oc_assets SET current_version=x'04'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert_eq!(
        tx.active_version_roots(&[(row.asset, 1)]).await.unwrap()[&(row.asset, 1)],
        row.source
    );
    assert!(
        super::read_model::current_chunk_roots(&mut tx.tx, scope, &[row.chunk])
            .await
            .unwrap()
            .is_empty()
    );
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn raw_root_predicates_hide_bad_keys_until_the_projection_is_consumed() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let row = Record::new(50_001);
    seed(&mut tx, scope, row, Some(1), Some("active"), "body").await;
    db::query("UPDATE oc_versions SET source_event_id=x'31',title=x'02'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(
        tx.active_version_roots(&[(row.asset, 1)])
            .await
            .unwrap()
            .is_empty()
    );
    db::query("UPDATE oc_events SET id=x'31',state=x'02'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(
        tx.active_version_roots(&[(row.asset, 1)])
            .await
            .unwrap()
            .is_empty()
    );
    db::query("UPDATE oc_events SET state='active'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(matches!(
        tx.active_version_roots(&[(row.asset, 1)]).await,
        Err(StorageError::Backend(_))
    ));
    // A noncurrent row must be hidden before that malformed root UUID decodes.
    db::query("UPDATE oc_assets SET current_version=2")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(
        super::read_model::current_chunk_roots(&mut tx.tx, scope, &[row.chunk])
            .await
            .unwrap()
            .is_empty()
    );
    db::query("UPDATE oc_assets SET deleted=1")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(
        tx.active_version_roots(&[(row.asset, 1)])
            .await
            .unwrap()
            .is_empty()
    );
    db::query("UPDATE oc_chunks SET asset_id=x'ff',version=x'fe'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(
        super::read_model::current_chunk_roots(&mut tx.tx, scope, &[row.chunk])
            .await
            .unwrap()
            .is_empty()
    );
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn explicit_history_ignores_invalid_current_and_hidden_kind() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let row = Record::new(50_002);
    seed(&mut tx, scope, row, Some(1), Some("active"), "body").await;
    db::query("UPDATE oc_assets SET current_version=x'01'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert_eq!(
        tx.asset_view_inner(row.asset, Some(1)).await.unwrap()["content"],
        "body"
    );
    assert!(matches!(
        tx.asset_view_inner(row.asset, None).await,
        Err(StorageError::NotFound)
    ));
    db::query("UPDATE oc_assets SET kind=x'02'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_events SET state='retracted'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(matches!(
        tx.asset_view_inner(row.asset, Some(1)).await,
        Err(StorageError::NotFound)
    ));
    db::query("UPDATE oc_events SET state='active'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(matches!(
        tx.asset_view_inner(row.asset, Some(1)).await,
        Err(StorageError::Backend(_))
    ));
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn empty_history_still_prepares_the_original_projection() {
    for restore in [false, true] {
        let (_dir, pool) = database().await;
        db::query("DROP TABLE oc_versions")
            .execute(&pool)
            .await
            .unwrap();
        db::query("CREATE TABLE oc_versions(tenant_id BLOB,workspace_id BLOB,asset_id BLOB,version INTEGER,source_event_id BLOB)")
            .execute(&pool)
            .await
            .unwrap();
        let mut tx = transaction(&pool, scope()).await;
        let absent = Uuid::new_v4();
        let result = if restore {
            tx.restore_source_inner(absent, 1).await
        } else {
            tx.asset_view_inner(absent, Some(1)).await
        };
        assert!(matches!(result, Err(StorageError::Backend(_))));
        tx.rollback().await.unwrap();
        pool.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn current_chunks_native_filters_precede_typed_payloads() {
    for (hidden, sql) in [
        ("deleted", "UPDATE oc_assets SET deleted=1"),
        ("inactive", "UPDATE oc_events SET state='retracted'"),
        ("missing", "DELETE FROM oc_events"),
        ("superseded", "UPDATE oc_assets SET current_version=2"),
        ("state_blob", "UPDATE oc_events SET state=x'03'"),
    ] {
        let (_dir, pool) = database().await;
        let scope = scope();
        let mut tx = transaction(&pool, scope).await;
        let row = Record::new(60_000);
        seed(&mut tx, scope, row, Some(1), Some("active"), "body").await;
        db::query("UPDATE oc_assets SET kind=x'01'")
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        db::query("UPDATE oc_versions SET title=x'02'")
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        db::query("UPDATE oc_chunks SET content=x'04',locator=x'05'")
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        db::query(sql).execute(&mut *tx.tx).await.unwrap();
        assert!(
            current_chunks(&mut tx.tx, scope, Some(&[row.chunk]), None)
                .await
                .unwrap()
                .is_empty(),
            "{hidden}"
        );
        tx.rollback().await.unwrap();
        pool.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn current_chunks_keep_selected_errors_and_hide_unmatched_raw_keys() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let row = Record::new(60_001);
    seed(&mut tx, scope, row, Some(1), Some("active"), "body").await;
    db::query("UPDATE oc_versions SET title=x'02'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(matches!(
        current_chunks(&mut tx.tx, scope, Some(&[row.chunk]), None).await,
        Err(StorageError::Backend(_))
    ));
    assert!(
        current_chunks(&mut tx.tx, scope, Some(&[row.chunk]), Some(Uuid::new_v4()))
            .await
            .unwrap()
            .is_empty()
    );
    db::query("UPDATE oc_chunks SET asset_id=x'ff',version=x'fe'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(
        current_chunks(&mut tx.tx, scope, Some(&[row.chunk]), None)
            .await
            .unwrap()
            .is_empty()
    );
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn current_chunk_metadata_is_read_once_per_native_version_request() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let mut ids = Vec::new();
    for index in 0..9 {
        let row = Record::new(61_000 + index);
        seed(&mut tx, scope, row, Some(1), Some("active"), "body").await;
        ids.push(row.chunk);
        for ordinal in 1..17 {
            let id = Uuid::new_v4();
            db::query("INSERT INTO oc_chunks VALUES(?,?,?,?,1,?,'body','{}','needle')")
                .bind(scope.tenant_id)
                .bind(scope.workspace_id)
                .bind(id)
                .bind(row.asset)
                .bind(ordinal)
                .execute(&mut *tx.tx)
                .await
                .unwrap();
            ids.push(id);
        }
    }
    let (chunks, counts) =
        super::read_model::current_chunks_measured(&mut tx.tx, scope, Some(&ids), None)
            .await
            .unwrap();
    assert_eq!(chunks.len(), 153);
    assert_eq!(counts, [153, 9, 9, 9]);
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn raw_request_compression_keeps_distinct_storage_types() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    db::query("DROP TABLE oc_chunks")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    // No affinity on version: preserve integer/real/text/blob as distinct
    // storage values even where native equality may later match some of them.
    db::query("CREATE TABLE oc_chunks(tenant_id BLOB,workspace_id BLOB,id BLOB,asset_id BLOB,version,content TEXT,locator TEXT,search_terms TEXT)")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    let asset = Uuid::new_v4();
    for sql in [
        "INSERT INTO oc_chunks VALUES(?,?,?,?,1,'body','{}','needle')",
        "INSERT INTO oc_chunks VALUES(?,?,?,?,1.0,'body','{}','needle')",
        "INSERT INTO oc_chunks VALUES(?,?,?,?,'1','body','{}','needle')",
        "INSERT INTO oc_chunks VALUES(?,?,?,?,x'31','body','{}','needle')",
    ] {
        db::query(sql)
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(Uuid::new_v4())
            .bind(asset)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
    }
    let (chunks, counts) =
        super::read_model::current_chunks_measured(&mut tx.tx, scope, None, None)
            .await
            .unwrap();
    assert!(chunks.is_empty());
    assert_eq!(counts, [4, 4, 0, 0]);
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn visible_and_keyword_defer_payload_until_support_and_tokens_pass() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let row = Record::new(70_000);
    seed(&mut tx, scope, row, Some(1), Some("active"), "body").await;
    db::query("UPDATE oc_chunks SET locator='{\"parser\":\"plain\",\"support_sources\":[9e999]}'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(tx.visible_hit(row.chunk).await.unwrap().is_some());
    db::query("UPDATE oc_versions SET title=x'01'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_chunks SET content=x'02',locator=?")
        .bind(
            json!({"parser":"memory-semantics:v1","support_sources":[Uuid::new_v4()]}).to_string(),
        )
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(tx.visible_hit(row.chunk).await.unwrap().is_none());
    assert!(
        tx.keyword_hits("needle", false, None)
            .await
            .unwrap()
            .is_empty()
    );
    db::query("UPDATE oc_chunks SET locator='{}'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(tx.keyword_hits("", false, None).await.unwrap().is_empty());
    assert!(
        tx.keyword_hits("absent", false, None)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        tx.keyword_hits("needle", false, None).await,
        Err(StorageError::Backend(_))
    ));
    assert!(matches!(
        tx.visible_hit(row.chunk).await,
        Err(StorageError::Backend(_))
    ));
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn visible_hit_uses_semantic_source_override_before_root_uuid_fallback() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let row = Record::new(70_001);
    seed(&mut tx, scope, row, Some(1), Some("active"), "body").await;
    db::query("UPDATE oc_events SET id=x'31'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_versions SET source_event_id=x'31'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    let override_source = Uuid::new_v4();
    db::query("UPDATE oc_chunks SET locator=?")
        .bind(json!({"parser":"memory-semantics:v1","source_event_id":override_source,"support_sources":[]}).to_string())
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert_eq!(
        tx.visible_hit(row.chunk)
            .await
            .unwrap()
            .unwrap()
            .source_event_id,
        override_source
    );
    db::query("UPDATE oc_chunks SET locator='{}'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(matches!(
        tx.visible_hit(row.chunk).await,
        Err(StorageError::Backend(_))
    ));
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn keyword_empty_query_keeps_native_json_and_summary_prepare_errors() {
    for summaries in [false, true] {
        let (_dir, pool) = database().await;
        let scope = scope();
        let mut tx = transaction(&pool, scope).await;
        if !summaries {
            let row = Record::new(70_002);
            seed(&mut tx, scope, row, Some(1), Some("active"), "body").await;
            db::query("UPDATE oc_chunks SET locator='{'")
                .execute(&mut *tx.tx)
                .await
                .unwrap();
        }
        assert!(matches!(
            tx.keyword_hits("", summaries, None).await,
            Err(StorageError::Backend(_))
        ));
        tx.rollback().await.unwrap();
        pool.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn keyword_summary_keeps_native_aggregation_and_null_terms() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let row = Record::new(70_003);
    seed(&mut tx, scope, row, Some(1), Some("active"), "body").await;
    db::query(
        "CREATE TABLE oc_summaries(tenant_id BLOB,workspace_id BLOB,chunk_id BLOB,search_terms)",
    )
    .execute(&mut *tx.tx)
    .await
    .unwrap();
    assert!(
        tx.keyword_hits("needle", true, None)
            .await
            .unwrap()
            .is_empty()
    );
    for sql in [
        "INSERT INTO oc_summaries VALUES(?,?,?,NULL)",
        "INSERT INTO oc_summaries VALUES(?,?,?,'needle')",
        "INSERT INTO oc_summaries VALUES(?,?,?,12)",
    ] {
        db::query(sql)
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(row.chunk)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
    }
    let hits = tx.keyword_hits("needle 12", true, None).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].score, 1.0);
    assert!(
        tx.keyword_hits("needle absent", true, None)
            .await
            .unwrap()
            .is_empty()
    );
    // Hit rendering never consumes terms; the keyword route does.
    db::query("UPDATE oc_chunks SET search_terms=x'01'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(tx.visible_hit(row.chunk).await.unwrap().is_some());
    assert!(matches!(
        tx.keyword_hits("needle", false, None).await,
        Err(StorageError::Backend(_))
    ));
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn selected_payload_waits_for_other_chunk_version_support() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let row = Record::new(70_004);
    seed(&mut tx, scope, row, Some(1), Some("active"), "body").await;
    db::query("UPDATE oc_versions SET title=x'01'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("INSERT INTO oc_chunks VALUES(?,?,?,?,1,1,'other',?,'other')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(Uuid::new_v4())
        .bind(row.asset)
        .bind(
            json!({"parser":"memory-semantics:v1","support_sources":[Uuid::new_v4()]}).to_string(),
        )
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(tx.visible_hit(row.chunk).await.unwrap().is_none());
    assert!(
        tx.keyword_hits("needle", false, None)
            .await
            .unwrap()
            .is_empty()
    );
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn orphan_learning_hides_invalid_render_payload() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let row = Record::new(70_005);
    seed(&mut tx, scope, row, Some(1), Some("active"), "body").await;
    db::query("UPDATE oc_events SET kind='learning'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_versions SET title=x'01'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_chunks SET content=x'02',search_terms=x'03'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert!(tx.visible_hit(row.chunk).await.unwrap().is_none());
    assert!(
        tx.keyword_hits("needle", false, None)
            .await
            .unwrap()
            .is_empty()
    );
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn index_candidates_do_not_consume_hit_payload() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let row = Record::new(80_000);
    seed(&mut tx, scope, row, Some(1), Some("active"), "body").await;
    ready_index(&mut tx, row, "content", 11).await;
    for surface in ["vector", "full_text"] {
        publication(&mut tx, scope, row.key(11), surface, "committed", "chunk").await;
    }
    db::query("UPDATE oc_assets SET kind=x'04'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_versions SET title=x'01'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_chunks SET content=x'02',search_terms=x'03'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert_eq!(
        tx.vector_candidates("profile", 3, 11, None).await.unwrap(),
        vec![row.chunk]
    );
    assert_eq!(
        tx.full_text_candidates(11, None).await.unwrap(),
        vec![row.chunk]
    );
    assert!(matches!(
        tx.full_text_hit(row.chunk, row.key(11).1, 11).await,
        Err(StorageError::Backend(_))
    ));
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn hidden_index_metadata_and_payload_do_not_poison_hits() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let good = Record::new(80_001);
    let hidden = Record::new(80_002);
    for row in [good, hidden] {
        seed(&mut tx, scope, row, Some(1), Some("active"), "body").await;
        ready_index(&mut tx, row, "content", 11).await;
        for surface in ["vector", "full_text"] {
            publication(&mut tx, scope, row.key(11), surface, "committed", "chunk").await;
        }
    }
    db::query("UPDATE oc_versions SET title=x'01' WHERE asset_id=?")
        .bind(hidden.asset)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_artifact_ledger SET state='pending' WHERE artifact_id=?")
        .bind(hidden.chunk)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("INSERT INTO oc_artifact_ledger VALUES(?,?,?,'chunk',?,1,12,'vector',x'01')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(good.chunk)
        .bind(good.source)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("INSERT INTO oc_index_entries VALUES(?,?,?,'summary',x'01',3,12,'ready')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(good.chunk)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    let keys = [good.key(11), hidden.key(11)];
    assert_eq!(tx.full_text_hits(&keys).await.unwrap().len(), 1);
    assert_eq!(
        tx.vector_hits_exact(&keys, "profile", 3)
            .await
            .unwrap()
            .len(),
        1
    );
    db::query("UPDATE oc_artifact_ledger SET state='committed' WHERE artifact_id=?")
        .bind(hidden.chunk)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    locator(
        &mut tx,
        hidden.chunk,
        &json!({"parser":"memory-semantics:v1","support_sources":[Uuid::new_v4()]}).to_string(),
    )
    .await;
    assert_eq!(tx.full_text_hits(&keys).await.unwrap().len(), 1);
    assert_eq!(
        tx.vector_hits_exact(&keys, "profile", 3)
            .await
            .unwrap()
            .len(),
        1
    );
    // The malformed candidate ID is not needed by the version guard. Another
    // chunk hides the whole version before that output ID is decoded.
    db::query("INSERT INTO oc_chunks VALUES(?,?,?,?,1,1,'support',?,'support')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(Uuid::new_v4())
        .bind(hidden.asset)
        .bind(
            json!({"parser":"memory-semantics:v1","support_sources":[Uuid::new_v4()]}).to_string(),
        )
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    locator(&mut tx, hidden.chunk, "{}").await;
    db::query("UPDATE oc_chunks SET id=x'31' WHERE id=?")
        .bind(hidden.chunk)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_artifact_ledger SET artifact_id=x'31' WHERE artifact_id=?")
        .bind(hidden.chunk)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_index_entries SET artifact_id=x'31' WHERE artifact_id=?")
        .bind(hidden.chunk)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    assert_eq!(
        tx.full_text_candidates(11, None).await.unwrap(),
        vec![good.chunk]
    );
    assert_eq!(
        tx.vector_candidates("profile", 3, 11, None).await.unwrap(),
        vec![good.chunk]
    );
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_source_roots_apply_local_support_before_uuid_decode() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let row = Record::new(80_003);
    seed(&mut tx, scope, row, Some(1), Some("active"), "body").await;
    db::query("UPDATE oc_events SET id=x'31'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_versions SET source_event_id=x'31'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    locator(
        &mut tx,
        row.chunk,
        &json!({"parser":"memory-semantics:v1","support_sources":[Uuid::new_v4()]}).to_string(),
    )
    .await;
    assert!(matches!(
        tx.native_candidate_sources(&[row.chunk]).await,
        Err(StorageError::Unavailable(_))
    ));
    assert!(tx.native_candidate_sources(&[]).await.unwrap().is_empty());
    locator(&mut tx, row.chunk, "{}").await;
    assert!(matches!(
        tx.native_candidate_sources(&[row.chunk]).await,
        Err(StorageError::Backend(_))
    ));
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn context_index_candidates_ignore_unconsumed_title_and_content() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let row = Record::new(80_004);
    seed(&mut tx, scope, row, Some(1), Some("active"), "body").await;
    ready_index(&mut tx, row, "content", 11).await;
    for surface in ["vector", "full_text"] {
        publication(&mut tx, scope, row.key(11), surface, "committed", "chunk").await;
    }
    db::query("UPDATE oc_versions SET title=x'01'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_chunks SET content=x'02'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    let route = crate::context_retrieval::ContextRoute::Knowledge;
    assert_eq!(
        tx.context_vector_candidates("profile", 3, 11, route, None)
            .await
            .unwrap(),
        vec![row.chunk]
    );
    assert_eq!(
        tx.context_full_text_candidates(11, route, None)
            .await
            .unwrap(),
        vec![row.chunk]
    );
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn shallow_vector_readiness_batches_one_hundred_requests() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let records: Vec<_> = (0..100).map(|index| Record::new(100_000 + index)).collect();
    for row in &records {
        ready_index(&mut tx, *row, "content", 11).await;
    }
    for index in [0, 99] {
        db::query("UPDATE oc_index_entries SET generation=12 WHERE artifact_id=?")
            .bind(records[index].chunk)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
    }
    db::query("UPDATE oc_index_entries SET model_id='other' WHERE artifact_id=?")
        .bind(records[3].chunk)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_index_entries SET dimension=2 WHERE artifact_id=?")
        .bind(records[4].chunk)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_index_entries SET state='pending' WHERE artifact_id=?")
        .bind(records[7].chunk)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_index_entries SET field='summary' WHERE artifact_id=?")
        .bind(records[8].chunk)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    let mut keys: Vec<_> = records.iter().map(|row| row.key(11)).collect();
    keys[52] = keys[51]; // Duplicate straddles two four-request batches.
    let expected: Vec<_> = keys
        .iter()
        .enumerate()
        .filter_map(|(index, key)| (![0, 3, 4, 7, 8, 99].contains(&index)).then_some(*key))
        .collect();
    let (ready, queries, rows) = tx
        .ready_vector_keys_with_testmetrics(&keys, "profile", Some(3))
        .await
        .unwrap();
    assert_eq!(ready, expected);
    assert_eq!(queries, 25);
    assert_eq!(rows, 94);
    // Same artifacts can retain many historical ready generations. None is
    // requested by these batches, so they must not increase returned rows.
    for generation in 20..28 {
        ready_index(&mut tx, records[51], "content", generation).await;
    }
    let (after_history, history_queries, history_rows) = tx
        .ready_vector_keys_with_testmetrics(&keys, "profile", Some(3))
        .await
        .unwrap();
    assert_eq!(after_history, ready);
    assert_eq!(history_queries, 25);
    assert_eq!(history_rows, rows);
    let expected: Vec<_> = keys
        .iter()
        .enumerate()
        .filter_map(|(index, key)| (![0, 3, 7, 99].contains(&index)).then_some(*key))
        .collect();
    let (legacy, queries, legacy_rows) = tx
        .ready_vector_keys_with_testmetrics(&keys, "profile", None)
        .await
        .unwrap();
    assert_eq!(legacy, expected);
    assert_eq!(queries, 25);
    assert_eq!(legacy_rows, 96);
    let (padded, queries, _) = tx
        .ready_vector_keys_with_testmetrics(&keys[..5], "profile", Some(3))
        .await
        .unwrap();
    assert_eq!(queries, 2);
    assert_eq!(padded, vec![keys[1], keys[2]]);
    let generations = [records[0].key(11), records[0].key(12)];
    let (ready, queries, rows) = tx
        .ready_vector_keys_with_testmetrics(&generations, "profile", Some(3))
        .await
        .unwrap();
    assert_eq!(ready, vec![generations[1]]);
    assert_eq!(queries, 1);
    assert_eq!(rows, 1);
    // Both IN lists allow these two cross-paired rows, but neither exact
    // (artifact, generation) projection matches the corresponding request.
    let crossed = [records[0].key(11), records[1].key(12)];
    let (ready, queries, rows) = tx
        .ready_vector_keys_with_testmetrics(&crossed, "profile", Some(3))
        .await
        .unwrap();
    assert!(ready.is_empty());
    assert_eq!(queries, 1);
    assert_eq!(rows, 2);
    let (empty, queries, empty_rows) = tx
        .ready_vector_keys_with_testmetrics(&[], "profile", Some(3))
        .await
        .unwrap();
    assert!(empty.is_empty());
    assert_eq!(empty_rows, 0);
    assert_eq!(queries, 1); // Preserve the original empty-input prepare contract.
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn graph_evidence_ignores_unconsumed_payload_and_stops_at_twenty() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    db::query("CREATE TABLE oc_artifact_owners(tenant_id BLOB,workspace_id BLOB,artifact_id BLOB,source_id BLOB,version INTEGER)")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    let artifact = Uuid::new_v4();
    let mut expected = Vec::new();
    for index in 0..22 {
        let row = Record::new(90_000 + index);
        seed(&mut tx, scope, row, Some(1), Some("active"), "body").await;
        db::query("INSERT INTO oc_artifact_owners VALUES(?,?,?,?,1)")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(artifact)
            .bind(row.source)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        publication(
            &mut tx,
            scope,
            (artifact, row.key(11).1, 11),
            "graph",
            "committed",
            "entity",
        )
        .await;
        if index == 0 {
            locator(
                &mut tx,
                row.chunk,
                &json!({"parser":"memory-semantics:v1","support_sources":[Uuid::new_v4()]})
                    .to_string(),
            )
            .await;
        } else if index == 21 {
            // A local native extraction failure beyond the twenty visible
            // evidence budget must not be evaluated by this read.
            locator(
                &mut tx,
                row.chunk,
                r#"{"parser":"memory-semantics:v1","support_sources":[10000000000000000000000000000000]}"#,
            )
            .await;
        } else if expected.len() < 20 {
            expected.push(row.chunk);
        }
    }
    db::query("UPDATE oc_versions SET title=x'01'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("UPDATE oc_chunks SET content=x'02'")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    let evidence = tx.graph_evidence(artifact).await.unwrap();
    let actual: Vec<_> = evidence
        .iter()
        .map(|value| Uuid::parse_str(value["chunk_id"].as_str().unwrap()).unwrap())
        .collect();
    assert_eq!(actual, expected);
    assert!(matches!(
        tx.semantic_version_visible(Record::new(90_021).asset, 1)
            .await,
        Err(StorageError::Backend(_))
    ));
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn graph_evidence_skips_invalid_historical_keys_before_decode() {
    let (_dir, pool) = database().await;
    let scope = scope();
    let mut tx = transaction(&pool, scope).await;
    let row = Record::new(91_000);
    seed(&mut tx, scope, row, Some(1), Some("active"), "body").await;
    let artifact = Uuid::new_v4();
    db::query("CREATE TABLE oc_artifact_owners(tenant_id BLOB,workspace_id BLOB,artifact_id BLOB,source_id BLOB,version INTEGER)")
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("INSERT INTO oc_artifact_owners VALUES(?,?,?,?,1)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(artifact)
        .bind(row.source)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    publication(
        &mut tx,
        scope,
        (artifact, row.key(11).1, 11),
        "graph",
        "committed",
        "entity",
    )
    .await;
    db::query("INSERT INTO oc_assets VALUES(?,?,x'31','knowledge','old',2,0)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("INSERT INTO oc_versions VALUES(?,?,x'31',1,'old','old','hash',?,NULL)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(row.source)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    db::query("INSERT INTO oc_chunks VALUES(?,?,x'32',x'31',1,0,x'01','{}','old')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
    let evidence = tx.graph_evidence(artifact).await.unwrap();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0]["chunk_id"], json!(row.chunk));
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}
