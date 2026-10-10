//! SQLite graph adapter tests (M4): idempotent upsert, scoped traversal,
//! source-tagged delete, and delete-by-id primitives against a real SQLite db.

use origence::graph::{entity_id, relation_id};
use origence::storage::sqlite_graph::SqliteGraphStore;
use origence::storage::{GraphStore, Lifecycle, Scope, SourceVersion};
use origence::types::{Entity, Relation};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
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
    let store = SqliteGraphStore::open(dir.path().join("graph.db"))
        .await
        .unwrap();
    let scope = make_scope();
    let src = src();

    let alice = entity("Alice");
    let bob = entity("Bob");
    store
        .upsert_entities(scope, src, vec![alice.clone(), bob.clone()])
        .await
        .unwrap();
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

    let other = make_scope();
    let empty = store.list_ids(other).await.unwrap();
    assert_eq!(empty["entities"].as_array().unwrap().len(), 0);
    assert!(
        store
            .traverse(other, entity_id("Alice"), 1)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn delete_by_id_removes_shared_node_only_when_told() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteGraphStore::open(dir.path().join("graph.db"))
        .await
        .unwrap();
    let scope = make_scope();
    let alice = entity("Alice");

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
    let store = SqliteGraphStore::open(dir.path().join("graph.db"))
        .await
        .unwrap();
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
    let store = SqliteGraphStore::open(dir.path().join("graph.db"))
        .await
        .unwrap();
    store.initialize().await.unwrap();
    store.check().await.unwrap();
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn partial_graph_schema_is_rejected_without_repair() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.db");
    {
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE oc_graph_entities (uid TEXT PRIMARY KEY)")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }
    // open() runs initialize (no repair) then check (must fail on the partial
    // column projection).
    assert!(SqliteGraphStore::open(&path).await.is_err());
    let options = SqliteConnectOptions::new().filename(&path);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    let tables: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name LIKE 'oc_graph_%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tables, 1); // the partial entities table was not repaired or extended
}

fn relation(head: &str, predicate: &str, tail: &str) -> Relation {
    Relation {
        source: head.into(),
        predicate: predicate.into(),
        target: tail.into(),
    }
}

#[tokio::test]
async fn missing_endpoints_do_not_create_relations() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteGraphStore::open(dir.path().join("graph.db"))
        .await
        .unwrap();
    let scope = make_scope();
    let other = Scope {
        workspace_id: Uuid::new_v4(),
        ..scope
    };
    let source = src();
    store
        .upsert_entities(scope, source, vec![entity("Alice")])
        .await
        .unwrap();
    store
        .upsert_entities(other, source, vec![entity("Bob")])
        .await
        .unwrap();
    let r = relation("Alice", "knows", "Bob");
    store
        .upsert_relations(scope, source, vec![r.clone()])
        .await
        .unwrap();
    assert_eq!(
        store.list_ids(scope).await.unwrap()["relations"],
        serde_json::json!([])
    );
    store
        .upsert_entities(scope, source, vec![entity("Bob")])
        .await
        .unwrap();
    store
        .upsert_relations(scope, source, vec![r])
        .await
        .unwrap();
    assert_eq!(
        store.list_ids(scope).await.unwrap()["relations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn traversal_is_directed_bounded_distinct_and_excludes_start() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteGraphStore::open(dir.path().join("graph.db"))
        .await
        .unwrap();
    let scope = make_scope();
    let source = src();
    store
        .upsert_entities(
            scope,
            source,
            ["A", "B", "C", "D", "E"].map(entity).to_vec(),
        )
        .await
        .unwrap();
    store
        .upsert_relations(
            scope,
            source,
            vec![
                relation("A", "edge", "A"),
                relation("A", "edge", "B"),
                relation("A", "parallel", "B"),
                relation("B", "edge", "A"),
                relation("B", "edge", "C"),
                relation("C", "edge", "D"),
                relation("D", "edge", "E"),
            ],
        )
        .await
        .unwrap();
    let a = entity_id("A");
    assert!(store.traverse(scope, a, 0).await.unwrap().is_empty());
    assert_eq!(
        store.traverse(scope, a, 1).await.unwrap(),
        vec![entity_id("B")]
    );
    let mut expected = vec![entity_id("B"), entity_id("C"), entity_id("D")];
    expected.sort_by_key(Uuid::to_string);
    assert_eq!(
        store.traverse(scope, a, usize::MAX).await.unwrap(),
        expected
    );
    assert!(
        store
            .traverse(scope, entity_id("E"), 3)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .traverse(scope, Uuid::new_v4(), 3)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn source_tags_preserve_entity_last_writer_and_relation_first_writer() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteGraphStore::open(dir.path().join("graph.db"))
        .await
        .unwrap();
    let scope = make_scope();
    let first = src();
    let last = SourceVersion {
        version: 2,
        ..first
    };
    let es = vec![entity("Alice"), entity("Bob")];
    let rs = vec![relation("Alice", "KNOWS", "Bob")];
    store
        .upsert_entities(scope, first, es.clone())
        .await
        .unwrap();
    store
        .upsert_relations(scope, first, rs.clone())
        .await
        .unwrap();
    store.upsert_entities(scope, last, es).await.unwrap();
    store.upsert_relations(scope, last, rs).await.unwrap();
    store.delete_source(scope, first).await.unwrap();
    let ids = store.list_ids(scope).await.unwrap();
    assert_eq!(ids["entities"].as_array().unwrap().len(), 2);
    assert_eq!(ids["relations"], serde_json::json!([]));
    store.delete_source(scope, last).await.unwrap();
    assert_eq!(
        store.list_ids(scope).await.unwrap(),
        serde_json::json!({"entities": [], "relations": []})
    );
}

#[tokio::test]
async fn detach_delete_cascades_incoming_outgoing_and_self_edges_only_in_scope() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteGraphStore::open(dir.path().join("graph.db"))
        .await
        .unwrap();
    let scope = make_scope();
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
    let source = src();
    for s in scopes {
        store
            .upsert_entities(s, source, vec![entity("A"), entity("B")])
            .await
            .unwrap();
        store
            .upsert_relations(
                s,
                source,
                vec![
                    relation("A", "edge", "B"),
                    relation("B", "edge", "A"),
                    relation("A", "edge", "A"),
                ],
            )
            .await
            .unwrap();
    }
    store
        .delete_objects(scope, vec![entity_id("A")], vec![])
        .await
        .unwrap();
    let ids = store.list_ids(scope).await.unwrap();
    assert_eq!(
        ids["entities"],
        serde_json::json!([entity_id("B").to_string()])
    );
    assert_eq!(ids["relations"], serde_json::json!([]));
    for s in &scopes[1..] {
        assert_eq!(
            store.list_ids(*s).await.unwrap()["relations"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }
    // Source deletion must cascade even relations tagged with another source.
    let foreign_source = src();
    store
        .upsert_entities(scope, foreign_source, vec![entity("A")])
        .await
        .unwrap();
    store
        .upsert_relations(scope, source, vec![relation("A", "edge", "B")])
        .await
        .unwrap();
    store.delete_source(scope, foreign_source).await.unwrap();
    assert_eq!(
        store.list_ids(scope).await.unwrap()["relations"],
        serde_json::json!([])
    );
    let rel_id = relation_id(entity_id("A"), "edge", entity_id("B"));
    store
        .delete_objects(scopes[1], vec![], vec![rel_id])
        .await
        .unwrap();
    assert_eq!(
        store.list_ids(scopes[1]).await.unwrap()["relations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        store.list_ids(scopes[2]).await.unwrap()["relations"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[tokio::test]
async fn concurrent_relation_replay_remains_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let store = std::sync::Arc::new(
        SqliteGraphStore::open(dir.path().join("graph.db"))
            .await
            .unwrap(),
    );
    let scope = make_scope();
    let source = src();
    store
        .upsert_entities(scope, source, vec![entity("A"), entity("B")])
        .await
        .unwrap();
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..16 {
        let store = store.clone();
        tasks.spawn(async move {
            store
                .upsert_relations(scope, source, vec![relation("A", "edge", "B")])
                .await
        });
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap().unwrap();
    }
    assert_eq!(
        store.list_ids(scope).await.unwrap()["relations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn snapshot_is_normalized_sorted_scoped_and_persistent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.db");
    let store = SqliteGraphStore::open(&path).await.unwrap();
    let scope = make_scope();
    let source = src();
    store
        .upsert_entities(scope, source, vec![entity(" ALICE "), entity("Bob")])
        .await
        .unwrap();
    store
        .upsert_relations(scope, source, vec![relation("alice", " KNOWS ", "BOB")])
        .await
        .unwrap();
    store
        .upsert_entities(make_scope(), source, vec![entity("hidden")])
        .await
        .unwrap();
    let a = entity_id("alice");
    let b = entity_id("bob");
    let mut entities = vec![
        serde_json::json!({"id": a, "name": "alice"}),
        serde_json::json!({"id": b, "name": "bob"}),
    ];
    entities.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    let expected = serde_json::json!({"entities": entities, "relations": [{
        "id": relation_id(a, "knows", b), "source_id": a,
        "target_id": b, "fact_text": "alice knows bob",
    }]});
    assert_eq!(store.snapshot(scope).await.unwrap(), expected);
    store.shutdown().await.unwrap();
    let reopened = SqliteGraphStore::open(&path).await.unwrap();
    assert_eq!(reopened.snapshot(scope).await.unwrap(), expected);
    assert_eq!(reopened.traverse(scope, a, 1).await.unwrap(), vec![b]);
}

#[tokio::test]
async fn schema_without_keys_or_cascades_is_rejected_without_repair() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.db");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    let weakened = include_str!("../src/storage/sqlite-graph-schema.sql")
        .replace(
            ",\n    FOREIGN KEY (head_uid) REFERENCES oc_graph_entities(uid) ON DELETE CASCADE",
            "",
        )
        .replace(
            ",\n    FOREIGN KEY (tail_uid) REFERENCES oc_graph_entities(uid) ON DELETE CASCADE",
            "",
        );
    sqlx::raw_sql(&weakened).execute(&pool).await.unwrap();
    pool.close().await;
    assert!(SqliteGraphStore::open(&path).await.is_err());
    let pool = SqlitePoolOptions::new()
        .connect_with(SqliteConnectOptions::new().filename(&path))
        .await
        .unwrap();
    let sql: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name='oc_graph_relations'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!sql.contains("FOREIGN KEY"));
    pool.close().await;
}

#[tokio::test]
async fn occupied_schema_view_is_rejected_without_creating_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.db");
    let pool = SqlitePoolOptions::new()
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    sqlx::query("CREATE VIEW oc_graph_entities AS SELECT 'x' AS uid")
        .execute(&pool)
        .await
        .unwrap();
    assert!(SqliteGraphStore::open(&path).await.is_err());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE type='table'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    pool.close().await;
}
