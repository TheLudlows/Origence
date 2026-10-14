//! Native equivalence gates for the extracted, benchmark-only JSONB candidate.
use super::*;

// Original TEXT expressions, with no historical cross-table oracle.
const ORIGINAL: &str = "SELECT json_quote(json_extract(json_extract(payload,?1),'$.id')),upper(replace(json_extract(json_extract(payload,?1),'$.id'),'-','')),json_array_length(json_extract(payload,?2),'$.embedding') FROM oc_jobs WHERE tenant_id=?3 AND workspace_id=?4 AND id=?5";
type Slots = [(usize, usize); 8];

fn candidate() -> Result<Json> {
    let path = std::env::var_os("PUBLICATION_PROBE_JSONB_SQL").or_else(|| std::env::var_os("PUBLICATION_PROBE_SQL"))
        .context("PUBLICATION_PROBE_SQL must name the extracted three-variant evidence")?;
    let bundle: Json = serde_json::from_slice(&std::fs::read(path)?)?;
    let variants = bundle["variants"].as_array().context("missing variants")?;
    ensure!(bundle["schema_version"] == 1 && variants.len() == 3, "expected three-variant evidence");
    let candidate = variants.iter().find(|variant| variant["label"] == "reviewed_candidate")
        .context("missing reviewed_candidate")?;
    ensure!(candidate["mode"] == "eight_slots" && candidate["slots"] == 8, "unexpected candidate flow");
    for key in ["ids_sql", "dimensions_sql"] {
        let statement = sql(candidate, key)?;
        ensure!(statement.contains("AS MATERIALIZED") && statement.contains("jsonb(payload)"), "{key} is not the materialized JSONB candidate");
    }
    ensure!(sql(candidate, "distinct_sql")? == "SELECT count(DISTINCT value) FROM json_each(?)", "unexpected DISTINCT flow");
    Ok(candidate.clone())
}

async fn database() -> Result<(turso::Database, Connection)> {
    let database = turso::Builder::new_local(":memory:").build().await?;
    let conn = database.connect()?;
    conn.execute("CREATE TABLE oc_jobs(tenant_id BLOB,workspace_id BLOB,id BLOB,payload TEXT)", Vec::<Value>::new()).await?;
    conn.execute("BEGIN", Vec::<Value>::new()).await?;
    Ok((database, conn))
}

fn path(index: usize) -> Value {
    Value::Text(format!("$.publication.memories[0].chunks[{index}]"))
}

fn consecutive(start: usize) -> Slots {
    std::array::from_fn(|slot| (start + slot, start + slot))
}

async fn set_payload(conn: &Connection, payload: Option<&str>) -> Result<()> {
    conn.execute("DELETE FROM oc_jobs WHERE tenant_id=? AND workspace_id=? AND id=?", scope_job()).await?;
    let mut params = scope_job();
    params.push(payload.map_or(Value::Null, |value| Value::Text(value.into())));
    conn.execute("INSERT INTO oc_jobs VALUES(?,?,?,?)", params).await?;
    Ok(())
}

async fn original(conn: &Connection, id: usize, dimension: usize) -> Result<Vec<Value>> {
    let mut params = vec![path(id), path(dimension)];
    params.extend(scope_job());
    one(query(conn, ORIGINAL, params, &mut Counter::default()).await?)
}

async fn projected(conn: &Connection, candidate: &Json, slots: &Slots, dimensions: bool) -> Result<Vec<Value>> {
    let mut params = Vec::new();
    for (id, dimension) in slots {
        params.push(path(*id));
        if dimensions { params.push(path(*dimension)); }
    }
    params.extend(scope_job());
    let key = if dimensions { "dimensions_sql" } else { "ids_sql" };
    one(query(conn, sql(candidate, key)?, params, &mut Counter::default()).await?)
}

async fn assert_projections(conn: &Connection, candidate: &Json, slots: &Slots) -> Result<Vec<Vec<Value>>> {
    let ids = projected(conn, candidate, slots, false).await?;
    let dimensions = projected(conn, candidate, slots, true).await?;
    ensure!(ids.len() == 8 && dimensions.len() == 16, "candidate layout changed");
    let mut originals = Vec::new();
    for (slot, (id, dimension)) in slots.iter().enumerate() {
        let before = original(conn, *id, *dimension).await?;
        let after = vec![ids[slot].clone(), dimensions[slot * 2].clone(), dimensions[slot * 2 + 1].clone()];
        assert_eq!(after, before, "slot {slot}, paths {id}/{dimension}");
        originals.push(before);
    }
    Ok(originals)
}

async fn distinct(conn: &Connection, candidate: &Json, ids: &[Value]) -> Result<i64> {
    let tokens: Vec<_> = ids.iter().map(text).collect::<Result<_>>()?;
    let params = vec![Value::Text(format!("[{}]", tokens.join(",")))];
    let row = one(query(conn, sql(candidate, "distinct_sql")?, params, &mut Counter::default()).await?)?;
    integer(&row[0])
}

async fn rollback_and_check_empty(conn: &Connection) -> Result<()> {
    // Native JSON errors may already abort the transaction. Match the actual
    // engine state; never swallow an unrelated rollback failure.
    if !conn.is_autocommit()? {
        conn.execute("ROLLBACK", Vec::<Value>::new()).await?;
    }
    ensure!(conn.is_autocommit()?, "rollback did not restore autocommit");
    let row = one(query(conn, "SELECT count(*) FROM oc_jobs", Vec::new(), &mut Counter::default()).await?)?;
    assert_eq!(row, vec![Value::Integer(0)]);
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn jsonb_preserves_nulls_mixed_elements_and_second_parse() -> Result<()> {
    let candidate = candidate()?;
    let (_database, conn) = database().await?;
    for payload in [
        None,
        Some("{}"),
        Some(r#"{"publication":{"memories":[{}]}}"#),
        Some(r#"{"publication":{"memories":[{"chunks":null}]}}"#),
        Some(r#"{"publication":{"memories":[{"chunks":[]}]}}"#),
    ] {
        set_payload(&conn, payload).await?;
        for row in assert_projections(&conn, &candidate, &consecutive(0)).await? {
            assert_eq!(row, vec![Value::Text("null".into()), Value::Null, Value::Null]);
        }
    }
    let payload = r#"{"publication":{"memories":[{"chunks":[null,true,17,{},[],{"id":"a-b","embedding":[null,false,{},[]]},"{\"id\":\"encoded-id\",\"embedding\":[0,0]}","null"]}]}}"#;
    set_payload(&conn, Some(payload)).await?;
    let rows = assert_projections(&conn, &candidate, &consecutive(0)).await?;
    assert_eq!(rows[5], vec![Value::Text(r#""a-b""#.into()), Value::Text("AB".into()), Value::Integer(4)]);
    assert_eq!(rows[6], vec![Value::Text(r#""encoded-id""#.into()), Value::Text("ENCODEDID".into()), Value::Integer(2)]);
    rollback_and_check_empty(&conn).await
}

#[tokio::test(flavor = "current_thread")]
async fn jsonb_preserves_duplicate_keys_and_native_distinct_ids() -> Result<()> {
    let candidate = candidate()?;
    let (_database, conn) = database().await?;
    // Raw strings retain duplicate names at every traversed object level.
    for payload in [
        r#"{"publication":{"memories":[{"chunks":[{"id":"first","embedding":[0]}]}]},"publication":{"memories":[{"chunks":[{"id":"second","embedding":[0,0]}]}]}}"#,
        r#"{"publication":{"memories":[{"chunks":[{"id":"first","embedding":[0]}]}],"memories":[{"chunks":[{"id":"second","embedding":[0,0]}]}]}}"#,
        r#"{"publication":{"memories":[{"chunks":[{"id":"first","embedding":[0]}],"chunks":[{"id":"second","embedding":[0,0]}]}]}}"#,
        r#"{"publication":{"memories":[{"chunks":[{"id":"first","id":"second","embedding":[0],"embedding":[0,0]}]}]}}"#,
        r#"{"publication":{"memories":[{"chunks":[{"id":{"k":1,"k":2},"embedding":[0]}]}]}}"#,
    ] {
        set_payload(&conn, Some(payload)).await?;
        assert_projections(&conn, &candidate, &consecutive(0)).await?;
    }
    let payload = r#"{"publication":{"memories":[{"chunks":[{"id":1},{"id":1.0},{"id":"1"},{"id":null},{"id":{"k":1}},{"id":[1]},{"id":true},{"id":1},{"id":{"k":1}}]}]}}"#;
    set_payload(&conn, Some(payload)).await?;
    let mut before: Vec<_> = assert_projections(&conn, &candidate, &consecutive(0)).await?
        .into_iter().map(|row| row[0].clone()).collect();
    before.push(original(&conn, 8, 8).await?[0].clone());
    let mut after = projected(&conn, &candidate, &consecutive(0), false).await?;
    after.push(projected(&conn, &candidate, &consecutive(8), false).await?[0].clone());
    assert_eq!(after, before);
    let original_distinct = distinct(&conn, &candidate, &before).await?;
    assert_eq!(original_distinct, 4);
    assert_eq!(distinct(&conn, &candidate, &after).await?, original_distinct);
    rollback_and_check_empty(&conn).await
}

#[tokio::test(flavor = "current_thread")]
async fn jsonb_binds_each_slot_and_scope_in_the_same_transaction() -> Result<()> {
    let candidate = candidate()?;
    let (_database, conn) = database().await?;
    // An incorrectly scoped materialization would evaluate a bad document.
    for scope in [[99, 2, 3], [1, 99, 3], [1, 2, 99]] {
        let mut params: Vec<_> = scope.into_iter().map(blob).collect();
        params.push(Value::Text("not-json".into()));
        conn.execute("INSERT INTO oc_jobs VALUES(?,?,?,?)", params).await?;
    }
    let chunks: Vec<_> = (0..9).map(|index| format!(
        "{{\"id\":\"slot-{index}\",\"embedding\":[{}]}}", vec!["0"; index + 1].join(",")
    )).collect();
    let payload = format!("{{\"publication\":{{\"memories\":[{{\"chunks\":[{}]}}]}}}}", chunks.join(","));
    set_payload(&conn, Some(&payload)).await?;
    assert_projections(&conn, &candidate, &consecutive(0)).await?;
    let tail = assert_projections(&conn, &candidate, &consecutive(8)).await?;
    assert_eq!(tail[0][1], Value::Text("SLOT8".into()));
    assert_eq!(tail[0][2], Value::Integer(9));
    assert_eq!(tail[1], vec![Value::Text("null".into()), Value::Null, Value::Null]);

    // Every numbered ID/dimension binding has an independent value.
    let slots = [(8, 0), (0, 8), (7, 1), (1, 7), (6, 2), (2, 6), (5, 3), (3, 5)];
    let rows = assert_projections(&conn, &candidate, &slots).await?;
    for (row, (id, dimension)) in rows.iter().zip(slots) {
        assert_eq!(row[1], Value::Text(format!("SLOT{id}")));
        assert_eq!(row[2], Value::Integer(dimension as i64 + 1));
    }
    set_payload(&conn, Some(r#"{"publication":{"memories":[{"chunks":[{"id":"updated","embedding":[0,0]}]}]}}"#)).await?;
    let rows = assert_projections(&conn, &candidate, &consecutive(0)).await?;
    assert_eq!(rows[0], vec![Value::Text(r#""updated""#.into()), Value::Text("UPDATED".into()), Value::Integer(2)]);
    rollback_and_check_empty(&conn).await
}

#[tokio::test(flavor = "current_thread")]
async fn jsonb_keeps_native_errors_and_rolls_back_writes() -> Result<()> {
    let candidate = candidate()?;
    for payload in [
        r#"{"publication":{"memories":[{"chunks":["not-json"]}]}}"#,
        r#"{"publication":{"memories":[{"chunks":["{"]}]}}"#,
        "{",
    ] {
        for projection in 0..3 {
            let (_database, conn) = database().await?;
            set_payload(&conn, Some(payload)).await?;
            let result = match projection {
                0 => original(&conn, 0, 0).await,
                1 => projected(&conn, &candidate, &consecutive(0), false).await,
                _ => projected(&conn, &candidate, &consecutive(0), true).await,
            };
            let error = result.expect_err("malformed JSON must not be accepted");
            ensure!(error.downcast_ref::<turso::Error>().is_some(), "projection {projection} failed outside the native engine: {error}");
            rollback_and_check_empty(&conn).await?;
        }
    }
    Ok(())
}
