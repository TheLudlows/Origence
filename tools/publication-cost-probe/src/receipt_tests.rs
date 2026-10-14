//! Focused native gates for the reviewed summary runner. Historical JSONB
//! correctness tests remain in jsonb_tests.rs, with their own frozen input.
use super::*;

fn candidate() -> Result<Json> {
    let path = std::env::var_os("PUBLICATION_PROBE_SQL").context("PUBLICATION_PROBE_SQL required")?;
    let bundle: Json = serde_json::from_slice(&std::fs::read(path)?)?;
    let candidate = bundle["variants"].as_array().context("variants")?.iter()
        .find(|value| value["mode"] == "reviewed_receipt_metadata_v1").context("reviewed summary variant required")?;
    Ok(candidate.clone())
}
async fn database() -> Result<(turso::Database, Connection)> {
    let database = turso::Builder::new_local(":memory:").build().await?;
    let conn = database.connect()?;
    conn.execute("CREATE TABLE oc_jobs(tenant_id BLOB,workspace_id BLOB,id BLOB,operation TEXT,payload TEXT,updated_at INTEGER)", Vec::<Value>::new()).await?;
    let mut params = scope_job(); params.push(Value::Text("{}".into()));
    conn.execute("INSERT INTO oc_jobs VALUES(?,?,?,'message_batch_ingest',?,1)", params).await?;
    Ok((database, conn))
}
async fn saved(conn: &Connection) -> Result<String> {
    let row = one(query(conn, "SELECT json_quote(json_extract(payload,'$.publication.receipt_metadata')) FROM oc_jobs", Vec::new(), &mut Counter::default()).await?)?;
    Ok(text(&row[0])?.to_owned())
}
#[test]
fn receipt_metadata_typed_contract_requires_dimension_and_format() -> Result<()> {
    let valid = r#"{"format":"message-receipt:v1","chunks":[{"id_json":"null","dimension":null}]}"#;
    assert!(ReceiptMetadata::decode(Some(valid), 1).is_ok());
    for invalid in [r#"{"format":"message-receipt:v1","chunks":[{"id_json":"null"}]}"#,
        r#"{"format":"wrong","chunks":[{"id_json":"null","dimension":null}]}"#,
        r#"{"format":"message-receipt:v1","chunks":[{"id_json":"null","dimension":null,"extra":1}]}"#] {
        assert!(ReceiptMetadata::decode(Some(invalid), 1).is_err());
    }
    assert!(ReceiptMetadata::decode(None, 1).is_err());
    assert_eq!(ReceiptMetadata::decode(None, 0)?, ReceiptMetadata::empty());
    Ok(())
}
#[tokio::test(flavor = "current_thread")]
async fn receipt_metadata_native_tokens_and_dimensions_preserved() -> Result<()> {
    let candidate = candidate()?;
    let (_database, conn) = database().await?;
    let plan = json!({"generation":1,"memories":[{"chunks":[
        {"id":null,"embedding":null},{"id":1,"embedding":[0]},
        {"id":1.0,"embedding":[]},{"id":true,"embedding":[0,1]},
        {"id":{"x":1},"embedding":[0]},{"id":[1,2],"embedding":[0]},
        {"id":"a-b","embedding":[0]},{"id":"a-b","embedding":[0,1]}]}]});
    let metadata = receipt_derive(&conn, &candidate, &plan.to_string(), &mut Counter::default()).await?;
    let mut tokens = Vec::new();
    for index in 0..metadata.chunks.len() {
        let row = one(query(&conn, "SELECT json_quote(json_extract(json_extract(?1,?2),'$.id')),json_array_length(json_extract(?1,?2),'$.embedding')", vec![Value::Text(plan.to_string()), Value::Text(format!("$.memories[0].chunks[{index}]"))], &mut Counter::default()).await?)?;
        assert_eq!(metadata.chunks[index].id_json, text(&row[0])?);
        assert_eq!(metadata.chunks[index].dimension, optional_integer(&row[1])?);
        tokens.push(text(&row[0])?.to_owned());
    }
    let tokens_json = format!("[{}]", tokens.join(","));
    let native = one(query(&conn, "SELECT count(DISTINCT value) FROM json_each(?)", vec![Value::Text(tokens_json)], &mut Counter::default()).await?)?;
    assert_eq!(receipt_distinct(&conn, &candidate, &metadata, &mut Counter::default()).await?, integer(&native[0])?);
    let expected = receipt_expected(&conn, &candidate, &metadata, &mut Counter::default()).await?;
    assert_eq!(expected.get("AB"), Some(&HashSet::from([1,2])));
    Ok(())
}
#[tokio::test(flavor = "current_thread")]
async fn receipt_metadata_native_rejects_empty_and_multiple_tokens() -> Result<()> {
    let candidate = candidate()?;
    let (_database, conn) = database().await?;
    for id_json in ["", "1,2", "{} []", "invalid"] {
        let metadata = ReceiptMetadata { format: RECEIPT_METADATA_FORMAT.into(), chunks: vec![ReceiptChunk { id_json: id_json.into(), dimension: Some(1) }] };
        assert!(receipt_distinct(&conn, &candidate, &metadata, &mut Counter::default()).await.is_err());
    }
    Ok(())
}
#[tokio::test(flavor = "current_thread")]
async fn receipt_metadata_save_replay_and_generation_contract() -> Result<()> {
    let candidate = candidate()?;
    let (_database, conn) = database().await?;
    let plan = json!({"generation":1,"memories":[{"chunks":[{"id":"a-b","embedding":[0,1]}]}]});
    receipt_save(&conn, &candidate, &plan).await?;
    let original = saved(&conn).await?;
    receipt_save(&conn, &candidate, &plan).await?;
    assert_eq!(saved(&conn).await?, original);
    let mut changed = plan.clone(); changed["memories"][0]["chunks"][0]["id"] = json!("different");
    assert!(receipt_save(&conn, &candidate, &changed).await.is_err());
    assert_eq!(saved(&conn).await?, original);
    conn.execute("UPDATE oc_jobs SET payload=json_remove(payload,'$.publication.receipt_metadata')", Vec::<Value>::new()).await?;
    assert!(receipt_save(&conn, &candidate, &plan).await.is_err());
    changed["generation"] = json!(2);
    receipt_save(&conn, &candidate, &changed).await?;
    assert_ne!(saved(&conn).await?, original);
    Ok(())
}
