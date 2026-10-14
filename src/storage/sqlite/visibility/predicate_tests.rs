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
    assert_eq!(
        support_members("null", "null").unwrap(),
        vec!["null".to_owned()]
    );
    assert_eq!(
        support_members("array", "[9e999]").unwrap(),
        vec!["9e999".to_owned()]
    );
}

#[tokio::test]
async fn local_support_matches_native_parameter_json_oracle() {
    let dir = tempfile::tempdir().unwrap();
    let pool = Pool::open(dir.path().join("supports.db")).await.unwrap();
    native_turso::query("CREATE TABLE oc_chunks(tenant_id BLOB,workspace_id BLOB,id BLOB,asset_id BLOB,version INTEGER,locator TEXT)")
        .execute(&pool).await.unwrap();
    native_turso::query(
        "CREATE TABLE oc_events(tenant_id BLOB,workspace_id BLOB,id BLOB,state TEXT)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    let active = Uuid::from_u128(0xabcdef1234567890abcdef1234567890);
    let inactive = Uuid::new_v4();
    let foreign = Uuid::new_v4();
    for (workspace, source, state) in [
        (scope.workspace_id, active, "active"),
        (scope.workspace_id, inactive, "retracted"),
        (Uuid::new_v4(), foreign, "active"),
    ] {
        native_turso::query("INSERT INTO oc_events VALUES(?,?,?,?)")
            .bind(scope.tenant_id)
            .bind(workspace)
            .bind(source)
            .bind(state)
            .execute(&pool)
            .await
            .unwrap();
    }
    let semantic =
        |value| json!({"parser":"memory-semantics:v1","support_sources":value}).to_string();
    let mut cases = vec![
        None,
        Some("null".into()),
        Some("{}".into()),
        Some(json!({"parser":null,"support_sources":["missing"]}).to_string()),
        Some(json!({"parser":"plain","support_sources":["missing"]}).to_string()),
        Some(json!({"parser":"memory-semantics:v1"}).to_string()),
    ];
    for value in [
        json!([]),
        json!({}),
        json!(null),
        json!([active]),
        json!([active, active]),
        json!([active.simple().to_string().to_uppercase()]),
        json!([format!("-{}-", active.simple())]),
        json!([inactive]),
        json!([foreign]),
        json!([Uuid::new_v4()]),
        json!(["not-a-uuid"]),
        json!(active),
        json!(17),
        json!(true),
        json!({"member":active}),
        json!([null]),
        json!([17]),
        json!([true]),
        json!([{}]),
        json!([[active]]),
        json!([active, "invalid"]),
    ] {
        cases.push(Some(semantic(value)));
    }
    cases.push(Some(format!(
        r#"{{"parser":"memory-semantics:v1","support_sources":{{"x":"missing","x":"{active}"}}}}"#
    )));
    cases.push(Some(format!(
        "{{parser:'memory-semantics:v1',support_sources:['{active}',],}}"
    )));
    cases.push(Some(
        "{parser:'memory-semantics:v1',support_sources:[Infinity,-Infinity,NaN]}".into(),
    ));
    cases.push(Some(
        r#"{"parser":"memory-semantics:v1","support_sources":[9e999]}"#.into(),
    ));
    let mut expected = HashMap::new();
    let active_hex = active.simple().to_string().to_uppercase();
    for (index, locator) in cases.iter().enumerate() {
        let asset = Uuid::from_u128(index as u128 + 1);
        native_turso::query("INSERT INTO oc_chunks VALUES(?,?,?,?,?,?)")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(Uuid::new_v4())
            .bind(asset)
            .bind(1_i32)
            .bind(locator)
            .execute(&pool)
            .await
            .unwrap();
        // Independent oracle: native JSON directly over a bound locator,
        // with no business-table association or correlated subquery.
        let semantic: bool = native_turso::query_scalar(
            "SELECT json_extract(?,'$.parser') IS 'memory-semantics:v1'",
        )
        .bind(locator)
        .fetch_one(&pool)
        .await
        .unwrap();
        let visible = if semantic {
            let sources: Vec<Option<String>> = native_turso::query_scalar(
                "SELECT upper(replace(value,'-','')) FROM json_each(?,'$.support_sources')",
            )
            .bind(locator)
            .fetch_all(&pool)
            .await
            .unwrap();
            sources
                .iter()
                .all(|source| source.as_ref() == Some(&active_hex))
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
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(Uuid::new_v4())
            .bind(asset)
            .bind(1_i32)
            .bind(malformed)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            native_turso::query_scalar::<bool>(
                "SELECT json_extract(?,'$.parser') IS 'memory-semantics:v1'",
            )
            .bind(malformed)
            .fetch_one(&pool)
            .await
            .is_err()
        );
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
    let pool = Pool::open(dir.path().join("numeric-support.db"))
        .await
        .unwrap();
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
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(vec![0x31_u8])
        .execute(&pool)
        .await
        .unwrap();
    // The old source extraction fails String decoding even though local
    // upper/replace finds the deliberately corrupt active event.
    assert!(
        native_turso::query_scalar::<String>(
            "SELECT DISTINCT value FROM json_each(?,'$.support_sources')",
        )
        .bind(locator)
        .fetch_all(&pool)
        .await
        .is_err()
    );
    native_turso::query("INSERT INTO oc_events VALUES(?,?,?,'active','knowledge')")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(source)
        .execute(&pool)
        .await
        .unwrap();
    native_turso::query("INSERT INTO oc_versions VALUES(?,?,?,?,?)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(asset)
        .bind(1_i32)
        .bind(source)
        .execute(&pool)
        .await
        .unwrap();
    native_turso::query("INSERT INTO oc_chunks VALUES(?,?,?,?,?,?)")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(id)
        .bind(asset)
        .bind(1_i32)
        .bind(locator)
        .execute(&pool)
        .await
        .unwrap();
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
        root: SourceVersion {
            source_id: source,
            version: 1,
        },
        search_terms: "numeric support".into(),
    };
    let mut tx = transaction(pool.begin().await.unwrap(), scope);
    assert!(tx.local_support_visibility(&[(asset, 1)]).await.unwrap()[&(asset, 1)]);
    assert!(matches!(
        tx.semantic_version_visible(asset, 1).await,
        Err(StorageError::Backend(_))
    ));
    assert!(matches!(
        tx.guard_retrieval_chunks(vec![chunk()]).await,
        Err(StorageError::Backend(_))
    ));
    tx.tx.rollback().await.unwrap();
    // Rejection by either earlier guard must not expose the delayed type error.
    native_turso::query("UPDATE oc_events SET state='retracted'")
        .execute(&pool)
        .await
        .unwrap();
    let mut tx = transaction(pool.begin().await.unwrap(), scope);
    assert!(!tx.semantic_version_visible(asset, 1).await.unwrap());
    assert!(
        tx.guard_retrieval_chunks(vec![chunk()])
            .await
            .unwrap()
            .is_empty()
    );
    tx.tx.rollback().await.unwrap();
    native_turso::query("UPDATE oc_events SET state='active',kind='learning'")
        .execute(&pool)
        .await
        .unwrap();
    let mut tx = transaction(pool.begin().await.unwrap(), scope);
    assert!(!tx.semantic_version_visible(asset, 1).await.unwrap());
    assert!(
        tx.guard_retrieval_chunks(vec![chunk()])
            .await
            .unwrap()
            .is_empty()
    );
    tx.tx.rollback().await.unwrap();
    // Containers are native text values: their later failure is UUID parsing,
    // not String decoding. A non-text member anywhere is decoded before UUIDs.
    let rows = native_turso::query("SELECT value AS raw_source FROM json_each(?)")
        .bind(r#"[{},[]]"#)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(matches!(
        strict_support_sources(&rows),
        Err(StorageError::NotFound)
    ));
    let rows = native_turso::query("SELECT value AS raw_source FROM json_each(?)")
        .bind(r#"["not-a-uuid",17]"#)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(matches!(
        strict_support_sources(&rows),
        Err(StorageError::Backend(_))
    ));
    let rows = native_turso::query("SELECT value AS raw_source FROM json_each(?)")
        .bind(r#"[17,"not-a-uuid"]"#)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(matches!(strict_support_sources(&rows), Err(StorageError::Backend(_))));
    native_turso::query("UPDATE oc_events SET state='active',kind='knowledge'")
        .execute(&pool).await.unwrap();
    let invalid_uuid = format!("-{}-", source.simple());
    for values in [
        json!([invalid_uuid, 31]),
        json!([31, invalid_uuid]),
        json!([invalid_uuid]),
    ] {
        native_turso::query("UPDATE oc_chunks SET locator=?")
            .bind(json!({"parser":"memory-semantics:v1","support_sources":values}).to_string())
            .execute(&pool).await.unwrap();
        let mut tx = transaction(pool.begin().await.unwrap(), scope);
        assert!(tx.local_support_visibility(&[(asset, 1)]).await.unwrap()[&(asset, 1)]);
        let result = tx.semantic_version_visible(asset, 1).await;
        if values.as_array().unwrap().len() == 1 {
            assert!(matches!(result, Err(StorageError::NotFound)));
        } else {
            assert!(matches!(result, Err(StorageError::Backend(_))));
        }
        tx.tx.rollback().await.unwrap();
    }
    // This is bound JSON, not a SQL literal. Native Turso rejects the bare
    // integer lexeme before it can become a real value; preserve that oracle.
    let large_integer_locator =
        r#"{"parser":"memory-semantics:v1","support_sources":[10000000000000000000000000000000]}"#;
    let error = native_turso::query("SELECT value FROM json_each(?,'$.support_sources')")
        .bind(large_integer_locator)
        .fetch_all(&pool)
        .await
        .unwrap_err();
    assert!(matches!(&error, native_turso::Error::Native(_)));
    assert!(error.to_string().contains("number too large"));
    native_turso::query("UPDATE oc_chunks SET locator=?")
        .bind(large_integer_locator)
        .execute(&pool)
        .await
        .unwrap();
    let mut tx = transaction(pool.begin().await.unwrap(), scope);
    assert!(matches!(
        tx.local_support_visibility(&[(asset, 1)]).await,
        Err(StorageError::Backend(_))
    ));
    // A native expression error invalidates this lease; settle it before
    // continuing the independent real-valued oracle.
    tx.tx.rollback().await.unwrap();
    // Exercise a real lexeme separately. A valid all-decimal UUID collision
    // is checked when its native text has 32 digits; do not assume formatting.
    let large_locator = r#"{"parser":"memory-semantics:v1","support_sources":[1e31]}"#;
    let large = native_turso::query(
        "SELECT value AS raw_source,upper(replace(value,'-','')) AS source FROM json_each(?,'$.support_sources')",
    ).bind(large_locator).fetch_one(&pool).await.unwrap();
    assert!(matches!(
        strict_support_sources(std::slice::from_ref(&large)),
        Err(StorageError::Backend(_))
    ));
    let normalized: String = large.try_get("source").unwrap();
    if normalized.len() == 32 && normalized.bytes().all(|byte| byte.is_ascii_digit()) {
        let numeric_source = Uuid::parse_str(&normalized).unwrap();
        native_turso::query("INSERT INTO oc_events VALUES(?,?,?,'active','knowledge')")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(numeric_source)
            .execute(&pool)
            .await
            .unwrap();
        native_turso::query("UPDATE oc_events SET state='active',kind='knowledge'")
            .execute(&pool)
            .await
            .unwrap();
        native_turso::query("UPDATE oc_chunks SET locator=?")
            .bind(large_locator)
            .execute(&pool)
            .await
            .unwrap();
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

#[tokio::test]
async fn exact_semantic_support_preserves_history_empty_sets_and_batched_sources() {
    let dir = tempfile::tempdir().unwrap();
    let pool = Pool::open(dir.path().join("exact-support.db")).await.unwrap();
    for sql in [
        "CREATE TABLE oc_chunks(tenant_id BLOB,workspace_id BLOB,id BLOB,asset_id BLOB,version INTEGER,locator TEXT)",
        "CREATE TABLE oc_events(tenant_id BLOB,workspace_id BLOB,id BLOB,state TEXT,kind TEXT)",
        "CREATE TABLE oc_versions(tenant_id BLOB,workspace_id BLOB,asset_id BLOB,version INTEGER,source_event_id BLOB)",
    ] {
        native_turso::query(sql).execute(&pool).await.unwrap();
    }
    let scope = Scope { tenant_id: Uuid::new_v4(), workspace_id: Uuid::new_v4() };
    let asset = Uuid::new_v4();
    let sources: Vec<_> = (1..=130).map(Uuid::from_u128).collect();
    for source in &sources {
        native_turso::query("INSERT INTO oc_events VALUES(?,?,?,'active','knowledge')")
            .bind(scope.tenant_id).bind(scope.workspace_id).bind(*source)
            .execute(&pool).await.unwrap();
    }
    let mut repeated = sources.clone();
    repeated.push(sources[0]);
    let mut locators = vec![
        json!({"parser":"memory-semantics:v1","support_sources":repeated}).to_string(),
        json!({"parser":"memory-semantics:v1"}).to_string(),
        json!({"parser":"memory-semantics:v1","support_sources":[]}).to_string(),
        json!({"parser":"memory-semantics:v1","support_sources":{}}).to_string(),
        json!({"parser":"memory-semantics:v1","support_sources":sources[0]}).to_string(),
        format!("{{parser:'memory-semantics:v1',support_sources:['{}',],}}", sources[0]),
        format!(r#"{{"parser":"memory-semantics:v1","support_sources":{{"x":"{}","x":"{}"}}}}"#, sources[0], sources[1]),
    ];
    // Malformed unrelated history must not be evaluated by an exact read.
    locators.push("{".into());
    for (index, locator) in locators.iter().enumerate() {
        native_turso::query("INSERT INTO oc_chunks VALUES(?,?,?,?,?,?)")
            .bind(scope.tenant_id).bind(scope.workspace_id).bind(Uuid::new_v4())
            .bind(asset).bind(index as i32 + 1).bind(locator)
            .execute(&pool).await.unwrap();
    }
    let mut tx = transaction(pool.begin().await.unwrap(), scope);
    for version in 1..=7 {
        assert!(tx.semantic_version_visible(asset, version).await.unwrap());
    }
    assert!(tx.semantic_version_visible(asset, 99).await.unwrap());
    assert!(tx.semantic_version_visible(Uuid::new_v4(), 1).await.unwrap());
    native_turso::query("UPDATE oc_events SET state='retracted' WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(sources[129])
        .execute(&mut *tx.tx).await.unwrap();
    assert!(!tx.semantic_version_visible(asset, 1).await.unwrap());
    assert!(tx.semantic_version_visible(asset, 2).await.unwrap());
    native_turso::query("UPDATE oc_events SET kind=? WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(vec![0xff_u8]).bind(scope.tenant_id).bind(scope.workspace_id).bind(sources[0])
        .execute(&mut *tx.tx).await.unwrap();
    // Local activity succeeds; exact source visibility still strictly decodes
    // the native event kind instead of treating a malformed kind as ordinary.
    assert!(tx.local_support_visibility(&[(asset, 5)]).await.unwrap()[&(asset, 5)]);
    assert!(matches!(tx.semantic_version_visible(asset, 5).await, Err(StorageError::Backend(_))));
    tx.tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}
