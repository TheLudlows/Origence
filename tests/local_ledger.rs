//! Cross-store ledger round-trip (M4): a pending write is registered on the
//! SQLite ledger, applied idempotently to an AnySeek protocol peer with attached graph, confirmed, and a
//! reconcile pass leaves it committed without duplicating (A2.6).
#![cfg(feature = "local-storage")]

use origence::storage::anyseek::{AnySeekFullTextStore, operation_epoch};
use origence::storage::ledger::{LedgerEntry, LedgerKey, Surface};
use origence::storage::sqlite::SqliteStore;
use origence::storage::{
    DomainTx, Permission, RelationalStore, Scope, SourceVersion, VectorEntry, VectorQuery,
};
use origence::types::Entity;
use uuid::Uuid;

#[tokio::test]
async fn pending_write_confirms_and_reconcile_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let sqlite = SqliteStore::open(dir.path().join("oc.db")).await.unwrap();
    let remote = crate::support::AnySeekFixture::start(Some(2));
    remote.bind_profile(Some("p:v1".into()));
    let vector =
        AnySeekFullTextStore::new(remote.config().with_embedding("p:v1".into(), 2).unwrap())
            .unwrap();

    let tenant = Uuid::new_v4();
    let workspace = Uuid::new_v4();
    sqlite
        .create_workspace(tenant, workspace, "acme")
        .await
        .unwrap();
    let scope = Scope {
        tenant_id: tenant,
        workspace_id: workspace,
    };
    let key = sqlite.issue_key(scope, "admin").await.unwrap();
    let auth = sqlite.authenticate(&key.token).await.unwrap();

    let source = SourceVersion {
        source_id: Uuid::new_v4(),
        version: 1,
    };
    let chunk_id = Uuid::new_v4();
    let entity = Entity {
        name: "Alice".into(),
        entity_type: "Person".into(),
        description: String::new(),
    };
    let generation = 1i64;

    let vector_key = LedgerKey {
        scope,
        source,
        artifact_id: chunk_id,
        surface: Surface::Vector,
        generation,
    };
    let graph_key = LedgerKey {
        scope,
        source,
        artifact_id: origence::graph::entity_id("Alice"),
        surface: Surface::Graph,
        generation,
    };

    // Step 1: register both pending writes in one relational transaction.
    {
        let mut tx = sqlite.begin(auth.clone()).await.unwrap();
        tx.check_permission(Permission::Write).await.unwrap();
        tx.register_pending(LedgerEntry {
            key: vector_key,
            artifact_type: "chunk".into(),
            idempotency_key: origence::storage::ledger::ledger_idempotency_key(&vector_key),
        })
        .await
        .unwrap();
        tx.register_pending(LedgerEntry {
            key: graph_key,
            artifact_type: "entity".into(),
            idempotency_key: origence::storage::ledger::ledger_idempotency_key(&graph_key),
        })
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }

    // Step 2: apply the external writes idempotently (replayed twice).
    let entry = VectorEntry {
        id: chunk_id,
        embedding: vec![1.0, 0.0],
        profile: "p:v1".into(),
        dimension: 2,
        generation,
        source,
    };
    for _ in 0..2 {
        vector
            .upsert_with_graph(
                scope,
                vec![origence::storage::FullTextEntry {
                    id: chunk_id,
                    source,
                    generation,
                    search_terms: "Alice".into(),
                }],
                vec![entry.clone()],
                Some(origence::storage::anyseek::GraphPublication {
                    source,
                    generation,
                    graph: origence::types::GraphExtraction {
                        entities: vec![entity.clone()],
                        relations: vec![],
                    },
                }),
                operation_epoch().unwrap(),
            )
            .await
            .unwrap();
    }

    // Step 3: confirm both committed in a relational transaction.
    {
        let mut tx = sqlite.begin(auth.clone()).await.unwrap();
        tx.check_permission(Permission::Write).await.unwrap();
        tx.confirm_committed(vector_key).await.unwrap();
        tx.confirm_committed(graph_key).await.unwrap();
        tx.commit().await.unwrap();
    }

    // Step 4: reconcile leaves committed entries untouched.
    let counts = sqlite.reconcile_ledger().await.unwrap();
    assert_eq!(counts["requeued"].as_i64().unwrap(), 0);
    assert_eq!(counts["orphaned"].as_i64().unwrap(), 0);

    // Step 5: no duplication from the replayed writes.
    let hits = vector
        .search_vector(
            VectorQuery {
                artifact_ids: Some(vec![chunk_id]),
                scope,
                profile: "p:v1".into(),
                dimension: 2,
                generation,
                embedding: vec![1.0, 0.0],
                limit: 10,
            },
            &[(chunk_id, source)].into(),
        )
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    let entities = vector.graph_seed_candidates(scope, "alice").await.unwrap();
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].source, source);
    assert!(!dir.path().join("graph.db").exists());

    // Step 6: re-confirm is a no-op (state-guard) and reconcile stays clean.
    {
        let mut tx = sqlite.begin(auth.clone()).await.unwrap();
        tx.check_permission(Permission::Write).await.unwrap();
        tx.confirm_committed(vector_key).await.unwrap();
        tx.commit().await.unwrap();
    }
    let counts = sqlite.reconcile_ledger().await.unwrap();
    assert_eq!(counts["requeued"].as_i64().unwrap(), 0);
    assert_eq!(counts["orphaned"].as_i64().unwrap(), 0);
}
