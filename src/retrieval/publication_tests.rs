//! Deterministic races between ranking and the final authority snapshot. Both
//! Search and Context call this exact helper; no production fault hook or native
//! timing assumptions are needed to exercise the publication boundary.
use super::*;
use crate::storage::{
    AuthorizedScope, LedgerEntry, LedgerKey, Lifecycle, RelationalStore, Scope,
    Surface, TursoStore, ledger_idempotency_key, native_turso as db,
};

struct Fixture {
    _dir: tempfile::TempDir,
    store: TursoStore,
    auth: AuthorizedScope,
    ranked: Vec<NativeRankedCandidate>,
}

impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = TursoStore::open(dir.path().join("authority.db")).await.unwrap();
        let scope = Scope { tenant_id: Uuid::new_v4(), workspace_id: Uuid::new_v4() };
        store.create_workspace(scope.tenant_id, scope.workspace_id, "final publication").await.unwrap();
        let key = store.issue_key(scope, "admin").await.unwrap();
        let auth = store.authenticate(&key.token).await.unwrap();
        let mut tx = store.begin(auth.clone()).await.unwrap();
        tx.check_permission(Permission::Write).await.unwrap();
        let mut originals = Vec::new();
        for ordinal in 0..2 {
            let content = format!("Atlas original {ordinal}");
            let asset = tx.insert_knowledge_asset(&content).await.unwrap();
            let root = tx.create_event("knowledge", &content, None).await.unwrap();
            let source = SourceVersion { source_id: root, version: 1 };
            let chunk = Uuid::new_v4();
            tx.insert_version(asset, 1, &content, "fixture", root, None, None).await.unwrap();
            tx.insert_chunk(chunk, asset, 1, 0, &content, &json!({"byte_start":0}), "atlas original").await.unwrap();
            tx.update_asset_version(asset, 1, None).await.unwrap();
            tx.index_ready(chunk, "fixture:2:v1", 2, 1).await.unwrap();
            for surface in [Surface::FullText, Surface::Vector] {
                let key = LedgerKey { scope, source, artifact_id: chunk, surface, generation: 1 };
                tx.register_pending(LedgerEntry {
                    key, artifact_type: "chunk".into(), idempotency_key: ledger_idempotency_key(&key),
                }).await.unwrap();
                tx.confirm_committed(key).await.unwrap();
            }
            originals.push((chunk, source));
        }
        tx.commit().await.unwrap();
        let mut tx = store.begin_read(auth.clone()).await.unwrap();
        tx.check_permission(Permission::Read).await.unwrap();
        let mut ranked = Vec::new();
        for (chunk, source) in originals {
            let mut hit = tx.full_text_hit(chunk, source, 1).await.unwrap().unwrap();
            hit.score = 1.0 / 61.0;
            ranked.push(NativeRankedCandidate {
                hit: hit.clone(),
                proof: NativePublicationProof::FullText { source, generation: 1 },
            });
            ranked.push(NativeRankedCandidate {
                hit,
                proof: NativePublicationProof::Vector {
                    source, generation: 1, profile: "fixture:2:v1".into(), dimension: 2,
                },
            });
        }
        tx.commit().await.unwrap();
        Self { _dir: dir, store, auth, ranked }
    }

    async fn verify(&self) -> Result<()> {
        let mut tx = self.store.begin_read(self.auth.clone()).await?;
        tx.check_permission(Permission::Read).await?;
        verify_native_candidates(&mut tx, &self.ranked).await?;
        tx.commit().await?;
        Ok(())
    }
}

#[tokio::test]
async fn final_native_validation_rejects_late_withdrawal_instead_of_short_top_k() {
    let fixture = Fixture::new().await;
    fixture.verify().await.unwrap();
    // The second chunk can be below the caller's response limit, yet removing
    // its earlier branch vote can change scores of other fused candidates.
    let withdrawn = &fixture.ranked[2];
    let mut tx = fixture.store.begin(fixture.auth.clone()).await.unwrap();
    tx.check_permission(Permission::Delete).await.unwrap();
    tx.retract_event(withdrawn.hit.source_event_id).await.unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(fixture.verify().await, Err(AppError::Unavailable(_))));
    // The first fresh verification after native IO obeys the same fail-closed
    // rule, rather than silently hiding a row which competed in native top-k.
    let mut tx = fixture.store.begin_read(fixture.auth.clone()).await.unwrap();
    assert!(matches!(
        live_native_candidate(&mut tx, withdrawn.hit.chunk_id, None).await,
        Err(AppError::Unavailable(_))
    ));
    tx.rollback().await.unwrap();
    fixture.store.shutdown().await.unwrap();
}

#[tokio::test]
async fn final_native_validation_rechecks_exact_publication_identity_and_readiness() {
    for sql in [
        "UPDATE oc_artifact_ledger SET state='pending' WHERE artifact_id=? AND surface='full_text'",
        "UPDATE oc_artifact_ledger SET generation=2 WHERE artifact_id=? AND surface='full_text'",
        "UPDATE oc_artifact_ledger SET source_id=zeroblob(16) WHERE artifact_id=? AND surface='full_text'",
        "UPDATE oc_artifact_ledger SET version=2 WHERE artifact_id=? AND surface='full_text'",
        "UPDATE oc_artifact_ledger SET state='orphan' WHERE artifact_id=? AND surface='vector'",
        "UPDATE oc_artifact_ledger SET generation=2 WHERE artifact_id=? AND surface='vector'",
        "UPDATE oc_artifact_ledger SET source_id=zeroblob(16) WHERE artifact_id=? AND surface='vector'",
        "UPDATE oc_artifact_ledger SET version=2 WHERE artifact_id=? AND surface='vector'",
        "UPDATE oc_index_entries SET state='removed' WHERE artifact_id=?",
        "UPDATE oc_index_entries SET generation=2 WHERE artifact_id=?",
        "UPDATE oc_index_entries SET model_id='other-profile' WHERE artifact_id=?",
        "UPDATE oc_index_entries SET dimension=3 WHERE artifact_id=?",
        "UPDATE oc_index_entries SET field='other-field' WHERE artifact_id=?",
    ] {
        let fixture = Fixture::new().await;
        fixture.verify().await.unwrap();
        db::query(sql).bind(fixture.ranked[0].hit.chunk_id).execute(fixture.store.pool()).await.unwrap();
        assert!(matches!(fixture.verify().await, Err(AppError::Conflict(_))), "{sql}");
        fixture.store.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn final_native_validation_never_attaches_an_old_score_to_changed_evidence() {
    let fixture = Fixture::new().await;
    fixture.verify().await.unwrap();
    db::query("UPDATE oc_chunks SET content='different evidence' WHERE id=?")
        .bind(fixture.ranked[0].hit.chunk_id).execute(fixture.store.pool()).await.unwrap();
    assert!(matches!(fixture.verify().await, Err(AppError::Conflict(_))));
    fixture.store.shutdown().await.unwrap();
}
