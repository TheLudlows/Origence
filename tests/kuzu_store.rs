//! Kuzu graph adapter tests (M4): idempotent MERGE upsert, scoped traversal,
//! source-tagged delete, and delete-by-id primitives against a real Kuzu db.
#![cfg(feature = "local-graph")]

use opencontext::graph::{entity_id, relation_id};
use opencontext::storage::kuzu::KuzuStore;
use opencontext::storage::{GraphStore, Lifecycle, Scope, SourceVersion};
use opencontext::types::{Entity, Relation};
use uuid::Uuid;

fn make_scope() -> Scope {
    Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    }
}

fn src() -> SourceVersion {
    SourceVersion {
        source_id: Uuid::new_v4(),
        version: 1,
    }
}

fn entity(name: &str) -> Entity {
    Entity {
        name: name.into(),
        entity_type: "Person".into(),
        description: String::new(),
    }
}

#[tokio::test]
async fn upsert_is_idempotent_and_traverse_is_scoped() {
    let dir = tempfile::tempdir().unwrap();
    let store = KuzuStore::open(dir.path()).await.unwrap();
    let scope = make_scope();
    let src = src();

    let alice = entity("Alice");
    let bob = entity("Bob");
    store
        .upsert_entities(scope, src, vec![alice.clone(), bob.clone()])
        .await
        .unwrap();
    // Idempotent replay must not create a duplicate node.
    store
        .upsert_entities(scope, src, vec![alice.clone(), bob.clone()])
        .await
        .unwrap();

    let rel = Relation {
        source: "Alice".into(),
        predicate: "knows".into(),
        target: "Bob".into(),
    };
    store
        .upsert_relations(scope, src, vec![rel.clone()])
        .await
        .unwrap();
    store
        .upsert_relations(scope, src, vec![rel.clone()])
        .await
        .unwrap();

    let ids = store.list_ids(scope).await.unwrap();
    assert_eq!(ids["entities"].as_array().unwrap().len(), 2);
    assert_eq!(ids["relations"].as_array().unwrap().len(), 1);
    assert_eq!(
        ids["relations"][0].as_str().unwrap(),
        relation_id(entity_id("Alice"), "knows", entity_id("Bob")).to_string()
    );

    let reached = store.traverse(scope, entity_id("Alice"), 1).await.unwrap();
    assert_eq!(reached, vec![entity_id("Bob")]);

    // A different workspace has no nodes and no reachable ids.
    let other = make_scope();
    let empty = store.list_ids(other).await.unwrap();
    assert_eq!(empty["entities"].as_array().unwrap().len(), 0);
    assert!(store.traverse(other, entity_id("Alice"), 1).await.unwrap().is_empty());
}

#[tokio::test]
async fn delete_by_id_removes_shared_node_only_when_told() {
    let dir = tempfile::tempdir().unwrap();
    let store = KuzuStore::open(dir.path()).await.unwrap();
    let scope = make_scope();
    let alice = entity("Alice");

    // Two sources extract the same entity name -> the same deterministic id,
    // so MERGE keeps a single node (the shared-source case).
    store
        .upsert_entities(scope, src(), vec![alice.clone()])
        .await
        .unwrap();
    store
        .upsert_entities(scope, src(), vec![alice.clone()])
        .await
        .unwrap();
    assert_eq!(
        store.list_ids(scope).await.unwrap()["entities"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // delete_by_id is the primitive graph-core G4 uses after the owner diff;
    // it removes the node only when the last owner is gone.
    store
        .delete_objects(scope, vec![entity_id("Alice")], vec![])
        .await
        .unwrap();
    assert_eq!(
        store.list_ids(scope).await.unwrap()["entities"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn source_tagged_delete_removes_last_writer() {
    let dir = tempfile::tempdir().unwrap();
    let store = KuzuStore::open(dir.path()).await.unwrap();
    let scope = make_scope();
    let src_a = src();
    store
        .upsert_entities(scope, src_a, vec![entity("Alice")])
        .await
        .unwrap();
    store.delete_source(scope, src_a).await.unwrap();
    assert_eq!(
        store.list_ids(scope).await.unwrap()["entities"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn lifecycle_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let store = KuzuStore::open(dir.path()).await.unwrap();
    store.initialize().await.unwrap();
    store.check().await.unwrap();
    store.shutdown().await.unwrap();
}
