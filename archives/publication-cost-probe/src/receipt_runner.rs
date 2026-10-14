// Reviewed SDK adaptation of the source-bound receipt_metadata helpers. SQL and
// the typed metadata below are extracted verbatim. The normal-shape metadata
// benchmark adapts DB plumbing, result containers and errors; it is not a full
// copy of authority/state branches. See extractor flow hashes.
#[derive(Debug)]
enum StorageError { Unavailable(String) }
impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self { Self::Unavailable(message) => f.write_str(message) }
    }
}
impl std::error::Error for StorageError {}
type StorageResult<T> = std::result::Result<T, StorageError>;
include!("receipt_types.rs");

fn optional_text(value: &Value) -> Result<Option<&str>> {
    match value { Value::Null => Ok(None), Value::Text(value) => Ok(Some(value)), _ => bail!("expected nullable native text") }
}
fn optional_integer(value: &Value) -> Result<Option<i64>> {
    match value { Value::Null => Ok(None), Value::Integer(value) => Ok(Some(*value)), _ => bail!("expected nullable native integer") }
}
async fn receipt_distinct(conn: &Connection, variant: &Json, metadata: &ReceiptMetadata, counter: &mut Counter) -> Result<i64> {
    if metadata.chunks.is_empty() { return Ok(0); }
    let tokens: Vec<_> = metadata.chunks.iter().map(|chunk| chunk.id_json.as_str()).collect();
    let invalid = one(query(conn, sql(variant, "validate_tokens_sql")?, vec![Value::Text(json!(tokens).to_string())], counter).await?)?;
    if integer(&invalid[0])? != 0 { return Err(invalid_plan().into()); }
    let result = one(query(conn, sql(variant, "distinct_sql")?, vec![Value::Text(metadata.ids_json())], counter).await?)?;
    integer(&result[0])
}
async fn receipt_expected(conn: &Connection, variant: &Json, metadata: &ReceiptMetadata, counter: &mut Counter) -> Result<HashMap<String, HashSet<i64>>> {
    let mut expected: HashMap<String, HashSet<i64>> = HashMap::new();
    if metadata.chunks.is_empty() { return Ok(expected); }
    let rows = query(conn, sql(variant, "dimensions_sql")?, vec![Value::Text(metadata.ids_json())], counter).await?;
    for row in rows {
        let index = usize::try_from(integer(&row[0])?)?;
        let id = optional_text(&row[1])?;
        let chunk = metadata.chunks.get(index).ok_or_else(invalid_plan)?;
        if let (Some(id), Some(dimension)) = (id, chunk.dimension)
            && (1..=4096).contains(&dimension)
        { expected.entry(id.to_owned()).or_default().insert(dimension); }
    }
    Ok(expected)
}
async fn receipt_derive(conn: &Connection, variant: &Json, plan: &str, counter: &mut Counter) -> Result<ReceiptMetadata> {
    let row = one(query(conn, sql(variant, "derive_kind_sql")?, vec![Value::Text(plan.to_owned())], counter).await?)?;
    let kind = optional_text(&row[0])?;
    let count = optional_integer(&row[1])?;
    if !matches!(kind, None | Some("null" | "array")) { return Err(invalid_plan().into()); }
    let mut metadata = ReceiptMetadata::empty();
    if count.unwrap_or(0) > 0 {
        let rows = query(conn, sql(variant, "derive_chunks_sql")?, vec![Value::Text(plan.to_owned())], counter).await?;
        metadata.chunks = rows.iter().map(|row| Ok(ReceiptChunk { id_json: text(&row[0])?.to_owned(), dimension: optional_integer(&row[1])? })).collect::<Result<_>>()?;
    }
    Ok(metadata)
}
struct SaveSample { elapsed_ms: f64, derive_ms: f64, serialized_plan_bytes: usize, counter: Counter }
async fn receipt_save(conn: &Connection, variant: &Json, plan: &Json) -> Result<SaveSample> {
    let start = Instant::now();
    let mut counter = Counter::default();
    let serialized = plan.to_string();
    let serialized_plan_bytes = serialized.len();
    let mut params = vec![Value::Text(serialized.clone())];
    params.extend(scope_job());
    let saved = one(query(conn, sql(variant, "saved_sql")?, params, &mut counter).await?)?;
    let same_generation = integer(&saved[0])? != 0;
    let saved_count = optional_integer(&saved[1])?;
    let saved_metadata = optional_text(&saved[2])?;
    let retained = if same_generation && saved_count.unwrap_or(0) > 0 {
        let count = usize::try_from(saved_count.unwrap_or(0)).map_err(|_| invalid_plan())?;
        Some(ReceiptMetadata::decode(saved_metadata, count)?)
    } else { None };
    let derive_start = Instant::now();
    let metadata = receipt_derive(conn, variant, &serialized, &mut counter).await?;
    let derive_ms = derive_start.elapsed().as_secs_f64() * 1000.0;
    if retained.as_ref().is_some_and(|saved| saved != &metadata) { return Err(invalid_plan().into()); }
    let supplied = plan.get("receipt_metadata");
    if let Some(supplied) = supplied {
        let supplied: ReceiptMetadata = serde_json::from_value(supplied.clone()).map_err(|_| invalid_plan())?;
        if supplied != metadata { return Err(invalid_plan().into()); }
    }
    let metadata_text = if retained.is_some() { saved_metadata.ok_or_else(invalid_plan)?.to_owned() }
        else if let Some(supplied) = supplied { supplied.to_string() }
        else { serde_json::to_string(&metadata).map_err(|_| invalid_plan())? };
    let mut params = vec![Value::Text(serialized), Value::Text(metadata_text), Value::Integer(1)];
    params.extend(scope_job());
    execute(conn, sql(variant, "save_sql")?, params, &mut counter).await?;
    Ok(SaveSample { elapsed_ms: start.elapsed().as_secs_f64() * 1000.0, derive_ms, serialized_plan_bytes, counter })
}

async fn measure_saves(conn: &Connection, variant: &Json, payload: &str, count: usize, warmups: usize, repeats: usize) -> Result<()> {
    let mut original: Json = serde_json::from_str(payload)?;
    let plan = original.as_object_mut().context("payload object")?.remove("publication").context("publication")?;
    let without_plan = original.to_string();
    let mut first_samples = Vec::new();
    let mut retry_samples = Vec::new();
    for round in 0..warmups + repeats {
        // Restoring pre-save input is fixture setup; derivation and the complete
        // save helper are timed for both phases, including serialization.
        let mut params = vec![Value::Text(without_plan.clone())]; params.extend(scope_job());
        conn.execute("UPDATE oc_jobs SET payload=? WHERE tenant_id=? AND workspace_id=? AND id=?", params).await?;
        let mut retry_plan = plan.clone();
        for (phase, samples) in [("first_save", &mut first_samples), ("retry_validation", &mut retry_samples)] {
            conn.execute("BEGIN", Vec::<Value>::new()).await?;
            let sample = receipt_save(conn, variant, if phase == "first_save" { &plan } else { &retry_plan }).await?;
            conn.execute("COMMIT", Vec::<Value>::new()).await?;
            if phase == "first_save" {
                // Rebuild the worker's saved-plan input outside the helper timer.
                // Only its small metadata is read back, never the heavy payload.
                let row = one(query(conn, "SELECT json_quote(json_extract(payload,'$.publication.receipt_metadata')) FROM oc_jobs WHERE tenant_id=? AND workspace_id=? AND id=?", scope_job(), &mut Counter::default()).await?)?;
                retry_plan["receipt_metadata"] = serde_json::from_str(text(&row[0])?)?;
            }
            ensure!(sample.counter.queries == 4, "save query count changed");
            let warmup = round < warmups;
            if !warmup { samples.push(sample.elapsed_ms); }
            report(json!({"event":"save_sample","variant":variant["label"],"phase":phase,"chunks":count,"round":round,"warmup":warmup,"elapsed_ms":sample.elapsed_ms,"derive_ms":sample.derive_ms,"serialized_plan_bytes":sample.serialized_plan_bytes,"sql_queries":sample.counter.queries,"bound_text_bytes":sample.counter.bound_text_bytes,"returned_text_bytes":sample.counter.returned_text_bytes,"scope":"SDK save helper adaptation; includes serialization, derivation, equality and atomic update; excludes begin/commit"}))?;
        }
        let verified = metadata(conn, variant, count).await?;
        ensure!(verified.result.distinct == count as i64, "saved metadata invalid");
    }
    report(json!({"event":"save_summary","variant":variant["label"],"chunks":count,"warmups":warmups,"repeats":repeats,"phases":[{"phase":"first_save","samples_ms":first_samples,"median_ms":median(&first_samples)},{"phase":"retry_validation","samples_ms":retry_samples,"median_ms":median(&retry_samples)}]}))?;
    Ok(())
}
