use super::*;

fn transaction(tx: Transaction, scope: Scope) -> TursoTx {
    TursoTx {
        tx,
        scope,
        principal_id: Uuid::nil(),
        required: Vec::new(),
        command: None,
    }
}

#[test]
fn normalized_object_members_preserve_every_duplicate_key() {
    assert_eq!(
        support_members("object", r#"{"x":"missing","x":"active"}"#).unwrap(),
        vec![r#""missing""#.to_owned(), r#""active""#.to_owned()]
    );
    assert!(support_members("object", "{}").unwrap().is_empty());
    assert!(support_members("array", "[]").unwrap().is_empty());
    assert_eq!(support_members("null", "null").unwrap(), vec!["null".to_owned()]);
    assert_eq!(support_members("array", "[9e999]").unwrap(), vec!["9e999".to_owned()]);
}

#[tokio::test]
async fn local_support_matches_native_parameter_json_oracle() {
    let dir = tempfile::tempdir().unwrap();
    let pool = Pool::open(dir.path().join("supports.db")).await.unwrap();
    native_turso::query("CREATE TABLE oc_chunks(tenant_id BLOB,workspace_id BLOB,id BLOB,asset_id BLOB,version INTEGER,locator TEXT)")
        .execute(&pool).await.unwrap();
    native_turso::query("CREATE TABLE oc_events(tenant_id BLOB,workspace_id BLOB,id BLOB,state TEXT)")
        .execute(&pool).await.unwrap();
    let scope = Scope { tenant_id: Uuid::new_v4(), workspace_id: Uuid::new_v4() };
    let active = Uuid::from_u128(0xabcdef1234567890abcdef1234567890);
    let inactive = Uuid::new_v4();
    let foreign = Uuid::new_v4();
    for (workspace, source, state) in [
        (scope.workspace_id, active, "active"),
        (scope.workspace_id, inactive, "retracted"),
        (Uuid::new_v4(), foreign, "active"),
    ] {
        native_turso::query("INSERT INTO oc_events VALUES(?,?,?,?)")
            .bind(scope.tenant_id).bind(workspace).bind(source).bind(state)
            .execute(&pool).await.unwrap();
    }
    let semantic = |value| json!({"parser":"memory-semantics:v1","support_sources":value}).to_string();
    let mut cases = vec![
        None,
        Some("null".into()),
        Some("{}".into()),
        Some(json!({"parser":null,"support_sources":["missing"]}).to_string()),
        Some(json!({"parser":"plain","support_sources":["missing"]}).to_string()),
        Some(json!({"parser":"memory-semantics:v1"}).to_string()),
    ];
    for value in [
        json!([]), json!({}), json!(null), json!([active]), json!([active, active]),
        json!([active.simple().to_string().to_uppercase()]),
        json!([format!("-{}-", active.simple())]), json!([inactive]), json!([foreign]),
        json!([Uuid::new_v4()]), json!(["not-a-uuid"]), json!(active), json!(17),
        json!(true), json!({"member":active}), json!([null]), json!([17]),
        json!([true]), json!([{}]), json!([[active]]), json!([active, "invalid"]),
    ] {
        cases.push(Some(semantic(value)));
    }
    cases.push(Some(format!(
        r#"{{"parser":"memory-semantics:v1","support_sources":{{"x":"missing","x":"{active}"}}}}"#
    )));
    cases.push(Some(format!(
        "{{parser:'memory-semantics:v1',support_sources:['{active}',],}}"
    )));
    cases.push(Some("{parser:'memory-semantics:v1',support_sources:[Infinity,-Infinity,NaN]}".into()));
    cases.push(Some(r#"{"parser":"memory-semantics:v1","support_sources":[9e999]}"#.into()));
    let mut expected = HashMap::new();
    let active_hex = active.simple().to_string().to_uppercase();
    for (index, locator) in cases.iter().enumerate() {
        let asset = Uuid::from_u128(index as u128 + 1);
        native_turso::query("INSERT INTO oc_chunks VALUES(?,?,?,?,?,?)")
            .bind(scope.tenant_id).bind(scope.workspace_id).bind(Uuid::new_v4())
            .bind(asset).bind(1_i32).bind(locator)
            .execute(&pool).await.unwrap();
        // Independent oracle: native JSON directly over a bound locator,
        // with no business-table association or correlated subquery.
        let semantic: bool = native_turso::query_scalar(
            "SELECT json_extract(?,'$.parser') IS 'memory-semantics:v1'",
        ).bind(locator).fetch_one(&pool).await.unwrap();
        let visible = if semantic {
            let sources: Vec<Option<String>> = native_turso::query_scalar(
                "SELECT upper(replace(value,'-','')) FROM json_each(?,'$.support_sources')",
            ).bind(locator).fetch_all(&pool).await.unwrap();
            sources.iter().all(|source| source.as_ref() == Some(&active_hex))
        } else {
            true
        };
        expected.insert((asset, 1), visible);
    }
    let absent = (Uuid::new_v4(), 42);
    expected.insert(absent, true);
    let keys: Vec<_> = expected.keys().copied().collect();
    let mut tx = transaction(pool.begin().await.unwrap(), scope);
    assert_eq!(tx.local_support_visibility(&keys).await.unwrap(), expected);
    tx.tx.rollback().await.unwrap();
    // Native extraction rejects malformed originals before Rust decoding.
    for malformed in ["{", "[", "not JSON"] {
        let asset = Uuid::new_v4();
        native_turso::query("INSERT INTO oc_chunks VALUES(?,?,?,?,?,?)")
            .bind(scope.tenant_id).bind(scope.workspace_id).bind(Uuid::new_v4())
            .bind(asset).bind(1_i32).bind(malformed)
            .execute(&pool).await.unwrap();
        assert!(native_turso::query_scalar::<bool>(
            "SELECT json_extract(?,'$.parser') IS 'memory-semantics:v1'",
        ).bind(malformed).fetch_one(&pool).await.is_err());
        let mut tx = transaction(pool.begin().await.unwrap(), scope);
        assert!(matches!(
            tx.local_support_visibility(&[(asset, 1)]).await,
            Err(StorageError::Backend(_))
        ));
        drop(tx);
    }
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_non_text_support_keeps_delayed_string_decode_error() {
    let dir = tempfile::tempdir().unwrap();
    let pool = Pool::open(dir.path().join("numeric-support.db")).await.unwrap();
    for sql in [
        "CREATE TABLE oc_chunks(tenant_id BLOB,workspace_id BLOB,id BLOB,asset_id BLOB,version INTEGER,locator TEXT)",
        "CREATE TABLE oc_events(tenant_id BLOB,workspace_id BLOB,id BLOB,state TEXT,kind TEXT)",
        "CREATE TABLE oc_versions(tenant_id BLOB,workspace_id BLOB,asset_id BLOB,version INTEGER,source_event_id BLOB)",
    ] {
        native_turso::query(sql).execute(&pool).await.unwrap();
    }
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    let asset = Uuid::new_v4();
    let id = Uuid::new_v4();
    let source = Uuid::new_v4();
    let locator = r#"{"parser":"memory-semantics:v1","support_sources":[31]}"#;
    let oracle = native_turso::query(
        "SELECT value AS raw_source,typeof(value) AS raw_type,upper(replace(value,'-','')) AS source FROM json_each(?,'$.support_sources')",
    )
    .bind(locator)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(oracle.try_get::<String>("raw_type").unwrap(), "integer");
    assert_eq!(oracle.try_get::<String>("source").unwrap(), "31");
    assert!(oracle.try_get::<String>("raw_source").is_err());
    // Deliberately corrupt short-blob event identity in this unconstrained
    // fixture: hex(x'31') matches numeric 31. This proves the guard cannot drop
    // non-text values; it does not claim x'31' is a valid application UUID.
    native_turso::query("INSERT INTO oc_events VALUES(?,?,?,'active','knowledge')")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(vec![0x31_u8])
        .execute(&pool).await.unwrap();
    // The old source extraction fails String decoding even though local
    // upper/replace finds the deliberately corrupt active event.
    assert!(native_turso::query_scalar::<String>(
        "SELECT DISTINCT value FROM json_each(?,'$.support_sources')",
    )
    .bind(locator)
    .fetch_all(&pool)
    .await
    .is_err());
    native_turso::query("INSERT INTO oc_events VALUES(?,?,?,'active','knowledge')")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(source)
        .execute(&pool).await.unwrap();
    native_turso::query("INSERT INTO oc_versions VALUES(?,?,?,?,?)")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(asset).bind(1_i32).bind(source)
        .execute(&pool).await.unwrap();
    native_turso::query("INSERT INTO oc_chunks VALUES(?,?,?,?,?,?)")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(id).bind(asset).bind(1_i32).bind(locator)
        .execute(&pool).await.unwrap();
    let chunk = || CurrentChunk {
        hit: SearchHit {
            identity: None,
            chunk_id: id,
            asset_id: asset,
            version: 1,
            kind: "knowledge".into(),
            title: "numeric support".into(),
            content: "numeric support".into(),
            locator: Value::Null,
            source_event_id: source,
            score: 0.0,
        },
        root: SourceVersion { source_id: source, version: 1 },
        search_terms: "numeric support".into(),
    };
    let mut tx = transaction(pool.begin().await.unwrap(), scope);
    assert!(tx.local_support_visibility(&[(asset, 1)]).await.unwrap()[&(asset, 1)]);
    assert!(matches!(
        tx.guard_retrieval_chunks(vec![chunk()]).await,
        Err(StorageError::Backend(_))
    ));
    tx.tx.rollback().await.unwrap();
    // Rejection by either earlier guard must not expose the delayed type error.
    native_turso::query("UPDATE oc_events SET state='retracted'")
        .execute(&pool).await.unwrap();
    let mut tx = transaction(pool.begin().await.unwrap(), scope);
    assert!(tx.guard_retrieval_chunks(vec![chunk()]).await.unwrap().is_empty());
    tx.tx.rollback().await.unwrap();
    native_turso::query("UPDATE oc_events SET state='active',kind='learning'")
        .execute(&pool).await.unwrap();
    let mut tx = transaction(pool.begin().await.unwrap(), scope);
    assert!(tx.guard_retrieval_chunks(vec![chunk()]).await.unwrap().is_empty());
    tx.tx.rollback().await.unwrap();
    // Containers are native text values: their later failure is UUID parsing,
    // not String decoding. A non-text member anywhere is decoded before UUIDs.
    let rows = native_turso::query("SELECT value AS raw_source FROM json_each(?)")
        .bind(r#"[{},[]]"#).fetch_all(&pool).await.unwrap();
    assert!(matches!(strict_support_sources(&rows), Err(StorageError::NotFound)));
    let rows = native_turso::query("SELECT value AS raw_source FROM json_each(?)")
        .bind(r#"["not-a-uuid",17]"#).fetch_all(&pool).await.unwrap();
    assert!(matches!(strict_support_sources(&rows), Err(StorageError::Backend(_))));
    // Also exercise a valid all-decimal UUID collision whenever the native
    // real-to-text representation has 32 digits. Do not assume f64 formatting.
    let large_locator = r#"{"parser":"memory-semantics:v1","support_sources":[10000000000000000000000000000000]}"#;
    let large = native_turso::query(
        "SELECT value AS raw_source,upper(replace(value,'-','')) AS source FROM json_each(?,'$.support_sources')",
    ).bind(large_locator).fetch_one(&pool).await.unwrap();
    assert!(matches!(strict_support_sources(std::slice::from_ref(&large)), Err(StorageError::Backend(_))));
    let normalized: String = large.try_get("source").unwrap();
    if normalized.len() == 32 && normalized.bytes().all(|byte| byte.is_ascii_digit()) {
        let numeric_source = Uuid::parse_str(&normalized).unwrap();
        native_turso::query("INSERT INTO oc_events VALUES(?,?,?,'active','knowledge')")
            .bind(scope.tenant_id).bind(scope.workspace_id).bind(numeric_source)
            .execute(&pool).await.unwrap();
        native_turso::query("UPDATE oc_events SET state='active',kind='knowledge'")
            .execute(&pool).await.unwrap();
        native_turso::query("UPDATE oc_chunks SET locator=?")
            .bind(large_locator).execute(&pool).await.unwrap();
        let mut tx = transaction(pool.begin().await.unwrap(), scope);
        assert!(tx.local_support_visibility(&[(asset, 1)]).await.unwrap()[&(asset, 1)]);
        assert!(matches!(
            tx.guard_retrieval_chunks(vec![chunk()]).await,
            Err(StorageError::Backend(_))
        ));
        tx.tx.rollback().await.unwrap();
    }
    pool.shutdown().await.unwrap();
}
