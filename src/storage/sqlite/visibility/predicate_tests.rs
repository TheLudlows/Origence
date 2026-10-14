use super::{CHUNK_SUPPORT_ACTIVE, CURRENT_CHUNK_ROWS};
use crate::storage::native_turso as db;
use serde_json::json;
use uuid::Uuid;

// Keep the previous SQL as an independent native-engine oracle. In particular,
// do not replace json_each or UUID string normalization with Rust parsing.
const PREVIOUS_CHUNK_SUPPORT_ACTIVE: &str = "(json_extract(c.locator,'$.parser') IS NOT 'memory-semantics:v1' OR NOT EXISTS(SELECT 1 FROM json_each(c.locator,'$.support_sources') ss WHERE NOT EXISTS(SELECT 1 FROM oc_events se WHERE se.tenant_id=c.tenant_id AND se.workspace_id=c.workspace_id AND hex(se.id)=upper(replace(ss.value,'-','')) AND se.state='active')))";

type Scope = (Uuid, Uuid);

async fn database() -> (tempfile::TempDir, db::Pool) {
    let dir = tempfile::tempdir().unwrap();
    let pool = db::Pool::open(dir.path().join("predicate.db")).await.unwrap();
    for sql in [
        "CREATE TABLE oc_events(tenant_id BLOB NOT NULL,workspace_id BLOB NOT NULL,id BLOB NOT NULL,state TEXT NOT NULL,PRIMARY KEY(tenant_id,workspace_id,id))",
        "CREATE TABLE oc_assets(tenant_id BLOB NOT NULL,workspace_id BLOB NOT NULL,id BLOB NOT NULL,current_version INTEGER NOT NULL,deleted INTEGER NOT NULL,PRIMARY KEY(tenant_id,workspace_id,id))",
        "CREATE TABLE oc_versions(tenant_id BLOB NOT NULL,workspace_id BLOB NOT NULL,asset_id BLOB NOT NULL,version INTEGER NOT NULL,source_event_id BLOB NOT NULL,PRIMARY KEY(tenant_id,workspace_id,asset_id,version))",
        // Nullable only to exercise SQL NULL input to the predicate, in addition
        // to the JSON null values permitted by the production locator column.
        "CREATE TABLE oc_chunks(tenant_id BLOB NOT NULL,workspace_id BLOB NOT NULL,id BLOB NOT NULL,asset_id BLOB NOT NULL,version INTEGER NOT NULL,locator TEXT,PRIMARY KEY(tenant_id,workspace_id,id))",
    ] {
        db::query(sql).execute(&pool).await.unwrap();
    }
    (dir, pool)
}

async fn event(pool: &db::Pool, scope: Scope, id: Uuid, state: &str) {
    db::query("INSERT INTO oc_events VALUES(?,?,?,?)")
        .bind(scope.0)
        .bind(scope.1)
        .bind(id)
        .bind(state)
        .execute(pool)
        .await
        .unwrap();
}

async fn chunk(
    pool: &db::Pool,
    scope: Scope,
    id: Uuid,
    asset: Uuid,
    version: i32,
    locator: Option<&str>,
) {
    db::query("INSERT INTO oc_chunks VALUES(?,?,?,?,?,?)")
        .bind(scope.0)
        .bind(scope.1)
        .bind(id)
        .bind(asset)
        .bind(version)
        .bind(locator)
        .execute(pool)
        .await
        .unwrap();
}

async fn support(pool: &db::Pool, predicate: &str, scope: Scope, id: Uuid) -> db::Result<bool> {
    db::query_scalar(&format!(
        "SELECT {predicate} FROM oc_chunks c WHERE c.tenant_id=? AND c.workspace_id=? AND c.id=?"
    ))
    .bind(scope.0)
    .bind(scope.1)
    .bind(id)
    .fetch_one(pool)
    .await
}

#[tokio::test]
async fn support_predicate_preserves_native_json_values_and_errors() {
    let (_dir, pool) = database().await;
    let scope = (Uuid::new_v4(), Uuid::new_v4());
    let id = Uuid::new_v4();
    let active = Uuid::from_u128(0xabcdef1234567890abcdef1234567890);
    let inactive = Uuid::new_v4();
    event(&pool, scope, active, "active").await;
    event(&pool, scope, inactive, "withdrawn").await;
    chunk(&pool, scope, id, Uuid::new_v4(), 1, None).await;
    let semantic = |sources| {
        json!({"parser":"memory-semantics:v1","support_sources":sources}).to_string()
    };
    let cases = vec![
        ("SQL null", None, Some(true)),
        ("JSON null", Some("null".into()), Some(true)),
        ("missing parser", Some("{}".into()), Some(true)),
        (
            "null parser",
            Some(json!({"parser":null,"support_sources":["missing"]}).to_string()),
            Some(true),
        ),
        (
            "other parser",
            Some(json!({"parser":"plain","support_sources":["missing"]}).to_string()),
            Some(true),
        ),
        (
            "missing support",
            Some(json!({"parser":"memory-semantics:v1"}).to_string()),
            Some(true),
        ),
        ("empty support", Some(semantic(json!([]))), Some(true)),
        ("active member", Some(semantic(json!([active]))), Some(true)),
        (
            "duplicate active member",
            Some(semantic(json!([active, active]))),
            Some(true),
        ),
        (
            "uppercase unhyphenated member",
            Some(semantic(json!([active.simple().to_string().to_uppercase()]))),
            Some(true),
        ),
        (
            "extra hyphens retain SQL normalization",
            Some(semantic(json!([format!("-{}-", active.simple())]))),
            Some(true),
        ),
        (
            "inactive member",
            Some(semantic(json!([inactive]))),
            Some(false),
        ),
        (
            "missing member",
            Some(semantic(json!([Uuid::new_v4()]))),
            Some(false),
        ),
        (
            "invalid member",
            Some(semantic(json!(["not-a-uuid"]))),
            Some(false),
        ),
        // Non-array and heterogeneous JSON behavior is defined by native Turso;
        // compare it to the old SQL rather than inventing stricter input rules.
        ("null support", Some(semantic(json!(null))), None),
        ("string support", Some(semantic(json!(active))), None),
        ("number support", Some(semantic(json!(17))), None),
        ("boolean support", Some(semantic(json!(true))), None),
        ("object support", Some(semantic(json!({"member":active}))), None),
        ("empty object support", Some(semantic(json!({}))), None),
        ("null member", Some(semantic(json!([null]))), None),
        ("number member", Some(semantic(json!([17]))), None),
        ("boolean member", Some(semantic(json!([true]))), None),
        ("object member", Some(semantic(json!([{}]))), None),
        ("nested array member", Some(semantic(json!([[active]]))), None),
        (
            "matching then invalid member",
            Some(semantic(json!([active, "invalid"]))),
            Some(false),
        ),
    ];
    for (label, locator, expected) in cases {
        db::query("UPDATE oc_chunks SET locator=? WHERE tenant_id=? AND workspace_id=? AND id=?")
            .bind(locator)
            .bind(scope.0)
            .bind(scope.1)
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        let previous = support(&pool, PREVIOUS_CHUNK_SUPPORT_ACTIVE, scope, id)
            .await
            .unwrap_or_else(|error| panic!("old predicate: {label}: {error}"));
        let current = support(&pool, CHUNK_SUPPORT_ACTIVE, scope, id)
            .await
            .unwrap_or_else(|error| panic!("new predicate: {label}: {error}"));
        assert_eq!(current, previous, "{label}");
        if let Some(expected) = expected {
            assert_eq!(current, expected, "{label}");
        }
    }
    for malformed in ["{", "[", "not JSON"] {
        db::query("UPDATE oc_chunks SET locator=?")
            .bind(malformed)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            support(&pool, PREVIOUS_CHUNK_SUPPORT_ACTIVE, scope, id)
                .await
                .is_err(),
            "old predicate accepted malformed JSON: {malformed}"
        );
        assert!(
            support(&pool, CHUNK_SUPPORT_ACTIVE, scope, id)
                .await
                .is_err(),
            "new predicate accepted malformed JSON: {malformed}"
        );
    }
    pool.shutdown().await.unwrap();
}

async fn current_ids(pool: &db::Pool, predicate: &str, scope: Scope) -> Vec<Uuid> {
    db::query_scalar(&format!(
        "SELECT c.id {CURRENT_CHUNK_ROWS} AND {predicate} ORDER BY c.id"
    ))
    .bind(scope.0)
    .bind(scope.1)
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn assert_current_ids(pool: &db::Pool, scope: Scope, expected: &[Uuid]) {
    let previous = current_ids(pool, PREVIOUS_CHUNK_SUPPORT_ACTIVE, scope).await;
    let current = current_ids(pool, CHUNK_SUPPORT_ACTIVE, scope).await;
    assert_eq!(current, previous);
    assert_eq!(current, expected);
}

#[tokio::test]
async fn support_predicate_preserves_scope_and_current_root_guards() {
    let (_dir, pool) = database().await;
    let tenant = Uuid::new_v4();
    let workspace = Uuid::new_v4();
    let scopes = [
        (tenant, workspace),
        (tenant, Uuid::new_v4()),
        (Uuid::new_v4(), workspace),
    ];
    let asset = Uuid::new_v4();
    let root = Uuid::new_v4();
    let member = Uuid::new_v4();
    let first = Uuid::from_u128(1);
    let second = Uuid::from_u128(2);
    let locator = json!({"parser":"memory-semantics:v1","support_sources":[member]}).to_string();
    for (index, scope) in scopes.into_iter().enumerate() {
        event(&pool, scope, root, "active").await;
        if index != 0 {
            event(&pool, scope, member, "active").await;
        }
        db::query("INSERT INTO oc_assets VALUES(?,?,?,1,0)")
            .bind(scope.0)
            .bind(scope.1)
            .bind(asset)
            .execute(&pool)
            .await
            .unwrap();
        for (version, id) in [(1, first), (2, second)] {
            db::query("INSERT INTO oc_versions VALUES(?,?,?,?,?)")
                .bind(scope.0)
                .bind(scope.1)
                .bind(asset)
                .bind(version)
                .bind(root)
                .execute(&pool)
                .await
                .unwrap();
            chunk(&pool, scope, id, asset, version, Some(&locator)).await;
        }
    }
    let scope = scopes[0];
    // The same UUID is active in both neighboring scopes, but absent here.
    assert_current_ids(&pool, scope, &[]).await;
    assert_current_ids(&pool, scopes[1], &[first]).await;
    assert_current_ids(&pool, scopes[2], &[first]).await;
    event(&pool, scope, member, "withdrawn").await;
    assert_current_ids(&pool, scope, &[]).await;
    db::query("UPDATE oc_events SET state='active' WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.0)
        .bind(scope.1)
        .bind(member)
        .execute(&pool)
        .await
        .unwrap();
    assert_current_ids(&pool, scope, &[first]).await;
    db::query("UPDATE oc_assets SET current_version=2 WHERE tenant_id=? AND workspace_id=?")
        .bind(scope.0)
        .bind(scope.1)
        .execute(&pool)
        .await
        .unwrap();
    assert_current_ids(&pool, scope, &[second]).await;
    db::query("UPDATE oc_assets SET deleted=1 WHERE tenant_id=? AND workspace_id=?")
        .bind(scope.0)
        .bind(scope.1)
        .execute(&pool)
        .await
        .unwrap();
    assert_current_ids(&pool, scope, &[]).await;
    db::query("UPDATE oc_assets SET deleted=0 WHERE tenant_id=? AND workspace_id=?")
        .bind(scope.0)
        .bind(scope.1)
        .execute(&pool)
        .await
        .unwrap();
    db::query("UPDATE oc_events SET state='withdrawn' WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.0)
        .bind(scope.1)
        .bind(root)
        .execute(&pool)
        .await
        .unwrap();
    assert_current_ids(&pool, scope, &[]).await;
    assert_current_ids(&pool, scopes[1], &[first]).await;
    assert_current_ids(&pool, scopes[2], &[first]).await;
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn support_predicate_resets_correlated_matches_between_outer_rows() {
    let (_dir, pool) = database().await;
    let scope = (Uuid::new_v4(), Uuid::new_v4());
    let active = Uuid::new_v4();
    let missing = Uuid::new_v4();
    let inactive = Uuid::new_v4();
    event(&pool, scope, active, "active").await;
    event(&pool, scope, inactive, "withdrawn").await;
    event(&pool, (scope.0, Uuid::new_v4()), missing, "active").await;
    event(&pool, (Uuid::new_v4(), scope.1), inactive, "active").await;
    let asset = Uuid::new_v4();
    let mut expected = Vec::new();
    for (index, (members, visible)) in [
        (json!([active]), true),
        (json!([missing]), false),
        (json!([active, active]), true),
        (json!([inactive]), false),
        (json!([]), true),
        (json!([active, missing]), false),
        (json!([missing, active]), false),
        (json!([active]), true),
    ]
    .into_iter()
    .enumerate()
    {
        let id = Uuid::from_u128(index as u128 + 1);
        let locator = json!({"parser":"memory-semantics:v1","support_sources":members}).to_string();
        chunk(&pool, scope, id, asset, 1, Some(&locator)).await;
        expected.push((id, visible));
    }
    for predicate in [PREVIOUS_CHUNK_SUPPORT_ACTIVE, CHUNK_SUPPORT_ACTIVE] {
        let rows: Vec<(Uuid, bool)> = db::query_as(&format!(
            "SELECT c.id,{predicate} FROM oc_chunks c WHERE c.tenant_id=? AND c.workspace_id=? ORDER BY c.id"
        ))
        .bind(scope.0)
        .bind(scope.1)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows, expected);
    }
    pool.shutdown().await.unwrap();
}
