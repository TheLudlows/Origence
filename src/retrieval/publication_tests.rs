//! Deterministic races between ranking and the final authority snapshot. Both
//! Search and Context call this exact helper; no production fault hook or native
//! timing assumptions are needed to exercise the publication boundary.
use super::*;
use crate::storage::{
    AuthorizedScope, LedgerEntry, LedgerKey, Lifecycle, RelationalStore, Scope, Surface,
    TursoStore, VectorStore, ledger_idempotency_key, native_turso as db,
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
        let store = TursoStore::open(dir.path().join("authority.db"))
            .await
            .unwrap();
        let scope = Scope {
            tenant_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
        };
        store
            .create_workspace(scope.tenant_id, scope.workspace_id, "final publication")
            .await
            .unwrap();
        let key = store.issue_key(scope, "admin").await.unwrap();
        let auth = store.authenticate(&key.token).await.unwrap();
        let mut tx = store.begin(auth.clone()).await.unwrap();
        tx.check_permission(Permission::Write).await.unwrap();
        let mut originals = Vec::new();
        for ordinal in 0..2 {
            let content = format!("Atlas original {ordinal}");
            let asset = tx.insert_knowledge_asset(&content).await.unwrap();
            let root = tx.create_event("knowledge", &content, None).await.unwrap();
            let source = SourceVersion {
                source_id: root,
                version: 1,
            };
            let chunk = Uuid::new_v4();
            tx.insert_version(asset, 1, &content, "fixture", root, None, None)
                .await
                .unwrap();
            tx.insert_chunk(
                chunk,
                asset,
                1,
                0,
                &content,
                &json!({"byte_start":0}),
                "atlas original",
            )
            .await
            .unwrap();
            tx.update_asset_version(asset, 1, None).await.unwrap();
            tx.index_ready(chunk, "fixture:2:v1", 2, 1).await.unwrap();
            for surface in [Surface::FullText, Surface::Vector] {
                let key = LedgerKey {
                    scope,
                    source,
                    artifact_id: chunk,
                    surface,
                    generation: 1,
                };
                tx.register_pending(LedgerEntry {
                    key,
                    artifact_type: "chunk".into(),
                    idempotency_key: ledger_idempotency_key(&key),
                })
                .await
                .unwrap();
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
                proof: NativePublicationProof::FullText {
                    source,
                    generation: 1,
                },
            });
            ranked.push(NativeRankedCandidate {
                hit,
                proof: NativePublicationProof::Vector {
                    source,
                    generation: 1,
                    profile: "fixture:2:v1".into(),
                    dimension: 2,
                },
            });
        }
        tx.commit().await.unwrap();
        Self {
            _dir: dir,
            store,
            auth,
            ranked,
        }
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
    tx.retract_event(withdrawn.hit.source_event_id)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(
        fixture.verify().await,
        Err(AppError::Unavailable(_))
    ));
    // The first fresh verification after native IO obeys the same fail-closed
    // rule, rather than silently hiding a row which competed in native top-k.
    let mut tx = fixture
        .store
        .begin_read(fixture.auth.clone())
        .await
        .unwrap();
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
        db::query(sql)
            .bind(fixture.ranked[0].hit.chunk_id)
            .execute(fixture.store.pool())
            .await
            .unwrap();
        assert!(
            matches!(fixture.verify().await, Err(AppError::Conflict(_))),
            "{sql}"
        );
        fixture.store.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn final_native_validation_never_attaches_an_old_score_to_changed_evidence() {
    let fixture = Fixture::new().await;
    fixture.verify().await.unwrap();
    db::query("UPDATE oc_chunks SET content='different evidence' WHERE id=?")
        .bind(fixture.ranked[0].hit.chunk_id)
        .execute(fixture.store.pool())
        .await
        .unwrap();
    assert!(matches!(fixture.verify().await, Err(AppError::Conflict(_))));
    fixture.store.shutdown().await.unwrap();
}

#[tokio::test]
async fn public_retrieval_future_sizes_have_bounded_stack_footprints() {
    // The closure is never called: measure its return type without allocating,
    // polling or even constructing a potentially large native future.
    fn future_size<F: std::future::Future>(_: impl FnOnce() -> F) -> usize {
        std::mem::size_of::<F>()
    }
    fn consuming_future_size<A, F: std::future::Future>(_: impl FnOnce(A) -> F) -> usize {
        std::mem::size_of::<F>()
    }
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(dir.path(), crate::models::Models::disabled())
        .await
        .unwrap();
    let auth = AuthContext {
        id: Uuid::new_v4(),
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
        role: "reader".into(),
    };
    let search_input =
        || serde_json::from_value(json!({"query":"fixture","mode":"hybrid"})).unwrap();
    let resolve_input =
        || serde_json::from_value(json!({"query":"fixture","mode":"hybrid"})).unwrap();
    let sizes = [
        (
            "search",
            future_size(|| service.search(&auth, search_input())),
        ),
        (
            "resolve",
            future_size(|| service.resolve(&auth, resolve_input())),
        ),
        (
            "evidence_bundle",
            future_size(|| {
                service.evidence_bundle(
                    &auth,
                    serde_json::from_value(json!({"query":"fixture","mode":"hybrid"})).unwrap(),
                )
            }),
        ),
        (
            "semantic_capture",
            future_size(|| {
                service.semantic_capture(
                    &auth,
                    "size-only",
                    crate::semantic_capture::SemanticCaptureInput {
                        content: "fixture".into(),
                        targets: Vec::new(),
                    },
                )
            }),
        ),
        (
            "process_next",
            future_size(|| crate::worker::process_next(&service)),
        ),
    ];
    eprintln!(
        "unboxed search implementation bytes={}",
        future_size(|| service.search_inner(&auth, search_input()))
    );
    eprintln!(
        "unboxed resolve implementation bytes={}",
        future_size(|| service.resolve_inner(&auth, resolve_input()))
    );
    eprintln!(
        "unboxed semantic_capture implementation bytes={}",
        service.unboxed_semantic_capture_future_size(&auth)
    );
    eprintln!(
        "unboxed process_next implementation bytes={}",
        crate::worker::unboxed_process_next_future_size(&service)
    );
    let pool = service.engine.relational().pool();
    eprintln!(
        "Turso acquire/begin/begin_with future bytes={}/{}/{}",
        future_size(|| pool.acquire()),
        future_size(|| pool.begin()),
        future_size(|| pool.begin_with("BEGIN DEFERRED"))
    );
    eprintln!(
        "Turso commit/rollback future bytes={}/{}",
        consuming_future_size(db::Transaction::commit),
        consuming_future_size(db::Transaction::rollback)
    );
    let scope = Scope {
        tenant_id: auth.tenant_id,
        workspace_id: auth.workspace_id,
    };
    let native = service.engine.vector();
    eprintln!(
        "Lance vector search future bytes={}",
        future_size(|| native.search(VectorQuery {
            scope,
            artifact_ids: None,
            profile: "fixture".into(),
            dimension: 2,
            generation: 1,
            embedding: vec![1.0, 0.0],
            limit: 100,
        }))
    );
    eprintln!(
        "Lance full-text search future bytes={}",
        future_size(|| native.search_full_text(FullTextQuery {
            scope,
            artifact_ids: None,
            generation: 1,
            search_terms: "fixture".into(),
            limit: 100,
        }))
    );
    eprintln!(
        "Lance vector/full-text upsert future bytes={}/{}",
        future_size(|| native.upsert(scope, Vec::new())),
        future_size(|| native.upsert_full_text(scope, Vec::new()))
    );
    for (name, bytes) in sizes {
        eprintln!("public {name} future bytes={bytes}");
        assert!(
            bytes <= 1024,
            "{name} public future embeds {bytes} bytes of stack state"
        );
    }
    service.engine.shutdown().await.unwrap();
}

impl Fixture {
    async fn verify_response_hits(
        &self,
        hits: &[SearchHit],
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        let mut tx = self.store.begin_read(self.auth.clone()).await?;
        tx.check_permission(Permission::Read).await?;
        let result = verify_search_hits(&mut tx, hits, limit, None).await;
        tx.rollback().await?;
        result
    }
}

#[tokio::test]
async fn final_response_validation_stops_at_limit_and_continues_past_hidden_hits() {
    let fixture = Fixture::new().await;
    // These rank inputs exercise final Summary/Graph selection. Native branch
    // participants additionally pass the all-candidate publication verifier.
    let mut first = fixture.ranked[0].hit.clone();
    let mut second = fixture.ranked[2].hit.clone();
    first.score = 0.5;
    second.score = 0.25;
    let hits = vec![first.clone(), second.clone()];
    db::query("UPDATE oc_chunks SET locator='{' WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(fixture.auth.scope.tenant_id)
        .bind(fixture.auth.scope.workspace_id)
        .bind(second.chunk_id)
        .execute(fixture.store.pool())
        .await
        .unwrap();
    let selected = fixture.verify_response_hits(&hits, 1).await.unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].chunk_id, first.chunk_id);
    assert_eq!(selected[0].score, first.score);
    assert_eq!(
        fixture
            .verify_response_hits(&hits, 2)
            .await
            .unwrap_err()
            .code(),
        "INTERNAL_ERROR"
    );
    // The limit never exempts a native participant from fresh validation.
    assert_eq!(fixture.verify().await.unwrap_err().code(), "INTERNAL_ERROR");
    db::query("UPDATE oc_assets SET deleted=1 WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(fixture.auth.scope.tenant_id)
        .bind(fixture.auth.scope.workspace_id)
        .bind(first.asset_id)
        .execute(fixture.store.pool())
        .await
        .unwrap();
    // A hidden first candidate must continue to the next rank, not silently
    // return an empty short page or hide the lower candidate's storage error.
    assert_eq!(
        fixture
            .verify_response_hits(&hits, 1)
            .await
            .unwrap_err()
            .code(),
        "INTERNAL_ERROR"
    );
    fixture.store.shutdown().await.unwrap();
}

#[tokio::test]
async fn final_response_validation_preserves_first_semantic_status_type_errors() {
    let fixture = Fixture::new().await;
    let hit = fixture.ranked[0].hit.clone();
    let hits = vec![hit.clone()];
    for status in [
        None,
        Some(Value::Null),
        Some(json!(7)),
        Some(json!("asserted")),
    ] {
        let mut locator = json!({
            "parser":"memory-semantics:v1",
            "support_sources":[hit.source_event_id],
        });
        if let Some(status) = &status {
            locator["semantic_status"] = status.clone();
        }
        db::query("UPDATE oc_chunks SET locator=? WHERE tenant_id=? AND workspace_id=? AND id=?")
            .bind(locator.to_string())
            .bind(fixture.auth.scope.tenant_id)
            .bind(fixture.auth.scope.workspace_id)
            .bind(hit.chunk_id)
            .execute(fixture.store.pool())
            .await
            .unwrap();
        let result = fixture.verify_response_hits(&hits, 1).await;
        if status.as_ref().is_some_and(Value::is_string) {
            assert_eq!(result.unwrap()[0].chunk_id, hit.chunk_id);
        } else {
            assert_eq!(result.unwrap_err().code(), "INTERNAL_ERROR", "{status:?}");
        }
    }
    // Only the first semantic row's status was decoded by the old contract.
    // A later non-string status must not poison a valid first-row status.
    db::query("INSERT INTO oc_chunks(tenant_id,workspace_id,id,asset_id,version,ordinal,content,locator,search_terms) VALUES(?,?,?,?,1,1,'later',?,'later')")
        .bind(fixture.auth.scope.tenant_id)
        .bind(fixture.auth.scope.workspace_id)
        .bind(Uuid::new_v4())
        .bind(hit.asset_id)
        .bind(json!({"parser":"memory-semantics:v1","support_sources":[hit.source_event_id],"semantic_status":7}).to_string())
        .execute(fixture.store.pool())
        .await
        .unwrap();
    assert_eq!(
        fixture.verify_response_hits(&hits, 1).await.unwrap()[0].chunk_id,
        hit.chunk_id
    );
    fixture.store.shutdown().await.unwrap();
}
