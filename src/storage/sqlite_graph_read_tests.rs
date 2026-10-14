//! Real native graph association regressions. Expected graph results and a
//! single-table text-order oracle replace any previous multi-relation SQL.
use super::*;
use crate::storage::native_turso as db;

fn scope() -> Scope {
    Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    }
}

fn source() -> SourceVersion {
    SourceVersion {
        source_id: Uuid::new_v4(),
        version: 1,
    }
}

fn entity(name: &str) -> Entity {
    Entity {
        name: name.into(),
        entity_type: "person".into(),
        description: String::new(),
    }
}

fn relation(head: &str, predicate: &str, tail: &str) -> Relation {
    Relation {
        source: head.into(),
        predicate: predicate.into(),
        target: tail.into(),
    }
}

fn key(value: &str) -> GraphKey {
    GraphKey(turso::Value::Text(value.to_owned()))
}

async fn fixture() -> (tempfile::TempDir, TursoGraphStore) {
    let dir = tempfile::tempdir().unwrap();
    let pool = Pool::open(dir.path().join("graph-reads.db")).await.unwrap();
    // Isolated diagnostic schema permits missing endpoints and raw storage
    // types. Production graph DDL/admission/FKs are not changed by this fixture.
    for sql in [
        "CREATE TABLE oc_graph_entities(uid TEXT PRIMARY KEY,tenant_id TEXT,workspace_id TEXT,id TEXT,name TEXT,entity_type TEXT,description TEXT,source_id TEXT,version INTEGER)",
        "CREATE TABLE oc_graph_relations(id TEXT,tenant_id TEXT,workspace_id TEXT,head_uid TEXT,tail_uid TEXT,predicate TEXT,source_id TEXT,version INTEGER,PRIMARY KEY(head_uid,tail_uid,id))",
    ] {
        db::query(sql).execute(&pool).await.unwrap();
    }
    (dir, TursoGraphStore { pool, max_hops: 3 })
}

async fn put_entity(
    conn: &mut Connection,
    scope: Scope,
    uid: GraphKey,
    id: impl db::Encode,
    name: &str,
) {
    db::query("INSERT INTO oc_graph_entities VALUES(?,?,?,?,?,'person','','source',1)")
        .bind(uid)
        .bind(scope.tenant_id.to_string())
        .bind(scope.workspace_id.to_string())
        .bind(id)
        .bind(name)
        .execute(conn)
        .await
        .unwrap();
}

async fn put_relation(
    conn: &mut Connection,
    scope: Scope,
    id: impl db::Encode,
    head: GraphKey,
    tail: GraphKey,
    predicate: impl db::Encode,
) {
    db::query("INSERT INTO oc_graph_relations VALUES(?,?,?,?,?,?,'source',1)")
        .bind(id)
        .bind(scope.tenant_id.to_string())
        .bind(scope.workspace_id.to_string())
        .bind(head)
        .bind(tail)
        .bind(predicate)
        .execute(conn)
        .await
        .unwrap();
}

#[tokio::test]
async fn graph_frontier_batches_keep_directed_minimum_depth_without_cutoff() {
    let (_dir, store) = fixture().await;
    let scope = scope();
    let from = Uuid::new_v4();
    let from_uid = entity_uid(scope, from);
    let mut tx = store.pool.begin().await.unwrap();
    put_entity(&mut tx, scope, key(&from_uid), from.to_string(), "root").await;
    let mut one = BTreeSet::new();
    let mut two = BTreeSet::new();
    let mut three = BTreeSet::new();
    for index in 0..70 {
        let b = Uuid::from_u128(10_000 + index);
        let c = Uuid::from_u128(20_000 + index);
        let d = Uuid::from_u128(30_000 + index);
        let e = Uuid::from_u128(40_000 + index);
        for id in [b, c, d, e] {
            put_entity(
                &mut tx,
                scope,
                key(&entity_uid(scope, id)),
                id.to_string(),
                "node",
            )
            .await;
        }
        for (head, tail, label) in [
            (from, b, "first"),
            (b, c, "second"),
            (c, d, "third"),
            (d, e, "fourth"),
            (b, from, "back"),
        ] {
            put_relation(
                &mut tx,
                scope,
                format!("{label}-{index}"),
                key(&entity_uid(scope, head)),
                key(&entity_uid(scope, tail)),
                "edge",
            )
            .await;
        }
        // Parallel routes may share a tail; no per-path result duplication.
        put_relation(
            &mut tx,
            scope,
            format!("parallel-{index}"),
            key(&from_uid),
            key(&entity_uid(scope, b)),
            "parallel",
        )
        .await;
        one.insert(b.to_string());
        two.extend([b.to_string(), c.to_string()]);
        three.extend([b.to_string(), c.to_string(), d.to_string()]);
    }
    put_relation(
        &mut tx,
        scope,
        "self",
        key(&from_uid),
        key(&from_uid),
        "edge",
    )
    .await;
    // This shortcut reaches a tail before its longer route, so descendants
    // still receive their full remaining hop budget.
    let shortcut = Uuid::from_u128(20_000);
    put_relation(
        &mut tx,
        scope,
        "shortcut",
        key(&from_uid),
        key(&entity_uid(scope, shortcut)),
        "edge",
    )
    .await;
    one.insert(shortcut.to_string());
    two.insert(Uuid::from_u128(30_000).to_string());
    three.insert(Uuid::from_u128(40_000).to_string());
    tx.commit().await.unwrap();
    for (hops, expected) in [(1, one), (2, two), (usize::MAX, three)] {
        let expected: Vec<_> = expected
            .into_iter()
            .map(|id| Uuid::parse_str(&id).unwrap())
            .collect();
        assert_eq!(store.traverse(scope, from, hops).await.unwrap(), expected);
    }
    assert!(
        store
            .traverse(scope, Uuid::from_u128(40_001), 3)
            .await
            .unwrap()
            .is_empty()
    );
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn graph_intermediate_raw_uids_do_not_acquire_entity_scope_guards() {
    for intermediate_exists in [false, true] {
        let (_dir, store) = fixture().await;
        let scope = scope();
        let foreign = Scope {
            workspace_id: Uuid::new_v4(),
            ..scope
        };
        let from = Uuid::new_v4();
        let target = Uuid::new_v4();
        let from_uid = entity_uid(scope, from);
        let target_uid = entity_uid(scope, target);
        let raw = GraphKey(turso::Value::Blob(vec![0x31]));
        let mut tx = store.pool.begin().await.unwrap();
        // Initial existence is by UID alone, even if stored metadata disagrees.
        put_entity(&mut tx, foreign, key(&from_uid), from.to_string(), "root").await;
        if intermediate_exists {
            put_entity(&mut tx, foreign, raw.clone(), vec![0x32_u8], "foreign").await;
        }
        put_entity(
            &mut tx,
            scope,
            key(&target_uid),
            target.to_string(),
            "target",
        )
        .await;
        put_relation(&mut tx, scope, "first", key(&from_uid), raw.clone(), "edge").await;
        put_relation(&mut tx, scope, "second", raw, key(&target_uid), "edge").await;
        put_relation(
            &mut tx,
            foreign,
            "wrong-scope",
            key(&from_uid),
            key(&target_uid),
            "edge",
        )
        .await;
        tx.commit().await.unwrap();
        assert!(store.traverse(scope, from, 1).await.unwrap().is_empty());
        assert_eq!(store.traverse(scope, from, 2).await.unwrap(), vec![target]);
        store.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn graph_id_merge_matches_native_binary_distinct_before_uuid_parsing() {
    let (_dir, store) = fixture().await;
    let scope = scope();
    let from = Uuid::new_v4();
    let from_uid = entity_uid(scope, from);
    let id = Uuid::from_u128(0xabcdefabcdefabcdefabcdefabcdefab);
    let samples = [
        id.to_string(),
        id.to_string().to_uppercase(),
        id.simple().to_string(),
        from.to_string(),
        "not a UUID".into(),
        "a".into(),
        "A".into(),
        "a ".into(),
        "a\0z".into(),
        "é".into(),
        "中".into(),
    ];
    let mut tx = store.pool.begin().await.unwrap();
    put_entity(&mut tx, scope, key(&from_uid), from.to_string(), "root").await;
    let mut reached = Vec::new();
    // Duplicates deliberately cross every 32-key boundary.
    for index in 0..99 {
        let uid = format!("output-{index}");
        put_entity(
            &mut tx,
            scope,
            key(&uid),
            samples[index % samples.len()].clone(),
            "node",
        )
        .await;
        put_relation(
            &mut tx,
            scope,
            format!("edge-{index}"),
            key(&from_uid),
            key(&uid),
            "edge",
        )
        .await;
        reached.push(key(&uid));
    }
    let native: Vec<String> = db::query_scalar(
        "SELECT DISTINCT id FROM oc_graph_entities WHERE tenant_id=? AND workspace_id=? AND uid<>? ORDER BY id",
    )
    .bind(scope.tenant_id.to_string())
    .bind(scope.workspace_id.to_string())
    .bind(&from_uid)
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    let merged = reached_entity_ids(
        &mut tx,
        &scope.tenant_id.to_string(),
        &scope.workspace_id.to_string(),
        &from_uid,
        &reached,
    )
    .await
    .unwrap();
    assert_eq!(merged.into_iter().collect::<Vec<_>>(), native);
    let expected: Vec<_> = native
        .into_iter()
        .filter_map(|text| Uuid::parse_str(&text).ok())
        .collect();
    assert_eq!(expected.iter().filter(|value| **value == id).count(), 3);
    assert!(
        expected.contains(&from),
        "only the starting UID is excluded"
    );
    tx.commit().await.unwrap();
    assert_eq!(store.traverse(scope, from, 1).await.unwrap(), expected);
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn graph_selected_non_string_id_is_backend_not_uuid_filtering() {
    let (_dir, store) = fixture().await;
    let scope = scope();
    let from = Uuid::new_v4();
    let uid = entity_uid(scope, from);
    let mut tx = store.pool.begin().await.unwrap();
    put_entity(&mut tx, scope, key(&uid), from.to_string(), "root").await;
    for index in 0..40 {
        let target = format!("target-{index}");
        if index == 39 {
            put_entity(&mut tx, scope, key(&target), vec![0x31_u8], "bad output").await;
        } else {
            put_entity(&mut tx, scope, key(&target), "not a UUID", "text output").await;
        }
        put_relation(
            &mut tx,
            scope,
            format!("edge-{index}"),
            key(&uid),
            key(&target),
            "edge",
        )
        .await;
    }
    tx.commit().await.unwrap();
    assert!(matches!(
        store.traverse(scope, from, 1).await,
        Err(StorageError::Backend(_))
    ));
    // The failed traversal transaction is dropped; a new lease remains usable.
    assert!(store.traverse(scope, from, 0).await.unwrap().is_empty());
    let count: i64 = db::query_scalar("SELECT count(*) FROM oc_graph_entities")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(count, 41);
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn graph_snapshot_keeps_endpoint_uid_semantics_and_relation_multiplicity() {
    let (_dir, store) = fixture().await;
    let scope = scope();
    let foreign = Scope {
        tenant_id: Uuid::new_v4(),
        ..scope
    };
    let a = Uuid::new_v4().to_string();
    let b = Uuid::new_v4().to_string();
    let raw_b = GraphKey(turso::Value::Blob(vec![0x31]));
    let mut tx = store.pool.begin().await.unwrap();
    put_entity(&mut tx, scope, key("a"), a.clone(), " ALPHA  One ").await;
    put_entity(&mut tx, foreign, raw_b.clone(), b.clone(), " BETA ").await;
    put_relation(
        &mut tx,
        scope,
        "same-id",
        key("a"),
        raw_b.clone(),
        " KNOWS ",
    )
    .await;
    put_relation(&mut tx, scope, "same-id", raw_b, key("a"), " LIKES ").await;
    put_relation(
        &mut tx,
        scope,
        vec![0x32_u8],
        key("a"),
        key("missing"),
        vec![0x33_u8],
    )
    .await;
    tx.commit().await.unwrap();
    let snapshot = store.snapshot(scope).await.unwrap();
    assert_eq!(
        snapshot["entities"],
        json!([{"id": a, "name": "alpha  one"}])
    );
    let rows = snapshot["relations"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.contains(
        &json!({"id":"same-id","source_id":a,"target_id":b,"fact_text":"alpha  one knows beta"})
    ));
    assert!(rows.contains(
        &json!({"id":"same-id","source_id":b,"target_id":a,"fact_text":"beta likes alpha  one"})
    ));
    let mut tx = store.pool.begin().await.unwrap();
    put_entity(
        &mut tx,
        foreign,
        key("missing"),
        Uuid::new_v4().to_string(),
        "present now",
    )
    .await;
    tx.commit().await.unwrap();
    assert!(matches!(
        store.snapshot(scope).await,
        Err(StorageError::Backend(_))
    ));
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn graph_preparation_errors_keep_zero_hop_and_empty_write_boundaries() {
    for case in 0..5 {
        let dir = tempfile::tempdir().unwrap();
        let pool = Pool::open(dir.path().join("partial.db")).await.unwrap();
        let store = TursoGraphStore { pool, max_hops: 3 };
        let scope = scope();
        assert!(
            store
                .traverse(scope, Uuid::new_v4(), 0)
                .await
                .unwrap()
                .is_empty()
        );
        store
            .upsert_relations(scope, source(), vec![])
            .await
            .unwrap();
        if case != 0 {
            db::query("CREATE TABLE oc_graph_entities(uid TEXT)")
                .execute(&store.pool)
                .await
                .unwrap();
        }
        if case == 2 || case == 3 {
            db::query("CREATE TABLE oc_graph_relations(head_uid TEXT,tail_uid TEXT,tenant_id TEXT,workspace_id TEXT)")
                .execute(&store.pool).await.unwrap();
        } else if case == 4 {
            // All insertion columns exist, but the conflict target does not.
            db::query("CREATE TABLE oc_graph_relations(id TEXT,tenant_id TEXT,workspace_id TEXT,head_uid TEXT,tail_uid TEXT,predicate TEXT,source_id TEXT,version INTEGER)")
                .execute(&store.pool).await.unwrap();
        }
        assert!(matches!(
            store.traverse(scope, Uuid::new_v4(), 1).await,
            Err(StorageError::Backend(_))
        ));
        // Missing endpoints cannot hide an absent target table, missing
        // insertion columns, or an invalid native conflict target.
        if case == 1 || case >= 3 {
            assert!(matches!(
                store
                    .upsert_relations(scope, source(), vec![relation("a", "edge", "b")])
                    .await,
                Err(StorageError::Backend(_))
            ));
        }
        store.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn graph_relation_transactions_keep_successful_prefix_and_first_writer() {
    let dir = tempfile::tempdir().unwrap();
    let store = TursoGraphStore::open(dir.path().join("graph.db"))
        .await
        .unwrap();
    let scope = scope();
    let first = source();
    store
        .upsert_entities(scope, first, ["a", "b", "c"].map(entity).to_vec())
        .await
        .unwrap();
    db::raw_sql("CREATE TRIGGER graph_fixture_failure BEFORE INSERT ON oc_graph_relations WHEN NEW.predicate='fail' BEGIN SELECT RAISE(ABORT,'fixture relation failure'); END;")
        .execute(&store.pool).await.unwrap();
    assert!(matches!(
        store
            .upsert_relations(
                scope,
                first,
                vec![
                    relation("a", "ok", "b"),
                    relation("b", "fail", "c"),
                    relation("c", "late", "a")
                ]
            )
            .await,
        Err(StorageError::Backend(_))
    ));
    let rows: Vec<(String, String, i64)> =
        db::query_as("SELECT predicate,source_id,version FROM oc_graph_relations")
            .fetch_all(&store.pool)
            .await
            .unwrap();
    assert_eq!(rows, [("ok".into(), first.source_id.to_string(), 1)]);
    let later = SourceVersion {
        version: 2,
        ..source()
    };
    store
        .upsert_relations(
            scope,
            later,
            vec![
                relation("a", "ok", "b"),
                relation("a", "self", "a"),
                relation("a", "missing", "absent"),
            ],
        )
        .await
        .unwrap();
    let rows: Vec<(String, String, i64)> = db::query_as(
        "SELECT predicate,source_id,version FROM oc_graph_relations ORDER BY predicate",
    )
    .fetch_all(&store.pool)
    .await
    .unwrap();
    assert_eq!(
        rows,
        [
            ("ok".into(), first.source_id.to_string(), 1),
            ("self".into(), later.source_id.to_string(), 2),
        ]
    );
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn graph_relation_endpoint_delete_race_cannot_leave_dangling_edges() {
    let dir = tempfile::tempdir().unwrap();
    let store = TursoGraphStore::open(dir.path().join("graph.db"))
        .await
        .unwrap();
    let scope = scope();
    let source = source();
    store
        .upsert_entities(scope, source, vec![entity("a"), entity("b")])
        .await
        .unwrap();
    let (inserted, deleted) = tokio::join!(
        store.upsert_relations(scope, source, vec![relation("a", "edge", "b")]),
        store.delete_objects(scope, vec![entity_id("b")], vec![]),
    );
    inserted.unwrap();
    deleted.unwrap();
    let snapshot = store.snapshot(scope).await.unwrap();
    assert_eq!(snapshot["relations"], json!([]));
    let count: i64 = db::query_scalar("SELECT count(*) FROM oc_graph_relations")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn graph_relation_metadata_uses_the_existing_read_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let store = TursoGraphStore::open(dir.path().join("graph.db"))
        .await
        .unwrap();
    let scope = scope();
    let source = source();
    store
        .upsert_entities(scope, source, vec![entity("a"), entity("b")])
        .await
        .unwrap();
    store
        .upsert_relations(scope, source, vec![relation("a", "edge", "b")])
        .await
        .unwrap();
    let mut reader = store.pool.begin().await.unwrap();
    let names: Vec<String> = db::query_scalar("SELECT name FROM oc_graph_entities ORDER BY name")
        .fetch_all(&mut *reader)
        .await
        .unwrap();
    assert_eq!(names, ["a", "b"]);
    db::query("UPDATE oc_graph_entities SET name='changed' WHERE id=?")
        .bind(entity_id("a").to_string())
        .execute(&store.pool)
        .await
        .unwrap();
    let old = snapshot_relations(
        &mut reader,
        &scope.tenant_id.to_string(),
        &scope.workspace_id.to_string(),
    )
    .await
    .unwrap();
    assert_eq!(old[0]["fact_text"], "a edge b");
    reader.commit().await.unwrap();
    assert_eq!(
        store.snapshot(scope).await.unwrap()["relations"][0]["fact_text"],
        "changed edge b"
    );
    store.shutdown().await.unwrap();
}
