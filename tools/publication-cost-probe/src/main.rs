//! Isolated native Turso metadata benchmark. Historical SQL is supplied from
//! verified, read-only external checkouts; no product SQL exception is added.
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;
use turso::{Connection, Value};

const COUNTS: [usize; 3] = [50, 100, 200];
const DIMENSION: usize = 4096;

fn uuid(value: u128) -> String {
    let hex = format!("{value:032x}");
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}
fn blob(value: u128) -> Value { Value::Blob(value.to_be_bytes().to_vec()) }
fn scope_job() -> Vec<Value> { vec![blob(1), blob(2), blob(3)] }
fn text(value: &Value) -> Result<&str> {
    match value { Value::Text(value) => Ok(value), _ => bail!("expected native text") }
}
fn integer(value: &Value) -> Result<i64> {
    match value { Value::Integer(value) => Ok(*value), _ => bail!("expected native integer") }
}
fn native_json(value: &Value) -> Json {
    match value {
        Value::Null => Json::Null,
        Value::Integer(value) => json!(value),
        Value::Real(value) => json!(value),
        Value::Text(value) => json!(value),
        Value::Blob(value) => json!({"blob": value}),
    }
}
fn report(value: Json) -> Result<()> {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{value}")?;
    out.flush()?;
    Ok(())
}

#[derive(Default)]
struct Counter { queries: usize, returned_text_bytes: usize }
async fn query(conn: &Connection, sql: &str, params: Vec<Value>, counter: &mut Counter) -> Result<Vec<Vec<Value>>> {
    counter.queries += 1;
    let mut rows = conn.query(sql, params).await?;
    let mut output = Vec::new();
    while let Some(row) = rows.next().await? {
        let mut values = Vec::new();
        for index in 0..row.column_count() {
            let value = row.get_value(index)?;
            if let Value::Text(value) = &value { counter.returned_text_bytes += value.len(); }
            values.push(value);
        }
        output.push(values);
    }
    Ok(output)
}
fn one(mut rows: Vec<Vec<Value>>) -> Result<Vec<Value>> {
    ensure!(rows.len() == 1, "expected one metadata row");
    Ok(rows.remove(0))
}
fn sql<'a>(variant: &'a Json, key: &str) -> Result<&'a str> {
    variant[key].as_str().with_context(|| format!("missing {key}"))
}

#[derive(Debug, PartialEq)]
struct Metadata {
    header: Vec<Json>,
    distinct: i64,
    expected: BTreeMap<String, BTreeSet<i64>>,
}
struct Sample {
    result: Metadata,
    elapsed_ms: f64,
    id_phase_ms: f64,
    dimensions_ms: f64,
    queries: usize,
    returned_text_bytes: usize,
}

async fn metadata(conn: &Connection, variant: &Json, count: usize) -> Result<Sample> {
    let mut counter = Counter::default();
    let start = Instant::now();
    let mut header = one(query(conn, sql(variant, "header_sql")?, scope_job(), &mut counter).await?)?;
    ensure!(header.len() == 14, "header layout changed: {} columns", header.len());
    let last = header.pop().context("missing final header column")?;
    let mode = sql(variant, "mode")?;
    let distinct = match mode {
        "historical_metadata" => integer(&last)?,
        "eight_slots" => {
            ensure!(text(&last)? == "array", "normal fixture must be an array");
            ensure!(variant["slots"] == 8, "unreviewed slot count");
            let mut tokens = String::from("[");
            for offset in (0..count).step_by(8) {
                let mut params: Vec<_> = (0..8).map(|slot| Value::Text(format!("$.publication.memories[0].chunks[{}]", offset + slot))).collect();
                params.extend(scope_job());
                let row = one(query(conn, sql(variant, "ids_sql")?, params, &mut counter).await?)?;
                ensure!(row.len() == 8, "ID projection layout changed");
                for (slot, value) in row.iter().take(8.min(count - offset)).enumerate() {
                    if offset + slot != 0 { tokens.push(','); }
                    tokens.push_str(text(value)?);
                }
            }
            tokens.push(']');
            let result = one(query(conn, sql(variant, "distinct_sql")?, vec![Value::Text(tokens)], &mut counter).await?)?;
            integer(&result[0])?
        }
        _ => bail!("unreviewed benchmark algorithm {mode}"),
    };
    let id_phase_ms = start.elapsed().as_secs_f64() * 1000.0;
    let dimension_start = Instant::now();
    let mut expected = BTreeMap::<String, BTreeSet<i64>>::new();
    if mode == "historical_metadata" {
        for row in query(conn, sql(variant, "dimensions_sql")?, scope_job(), &mut counter).await? {
            ensure!(row.len() == 2, "historical projection layout changed");
            expected.entry(text(&row[0])?.to_owned()).or_default().insert(integer(&row[1])?);
        }
    } else {
        for offset in (0..count).step_by(8) {
            let mut params = Vec::new();
            for slot in 0..8 {
                let path = format!("$.publication.memories[0].chunks[{}]", offset + slot);
                params.push(Value::Text(path.clone()));
                params.push(Value::Text(path));
            }
            params.extend(scope_job());
            let row = one(query(conn, sql(variant, "dimensions_sql")?, params, &mut counter).await?)?;
            ensure!(row.len() == 16, "dimension projection layout changed");
            for slot in 0..8.min(count - offset) {
                let id = text(&row[slot * 2])?;
                let dimension = integer(&row[slot * 2 + 1])?;
                if (1..=4096).contains(&dimension) {
                    expected.entry(id.to_owned()).or_default().insert(dimension);
                }
            }
        }
    }
    let dimensions_ms = dimension_start.elapsed().as_secs_f64() * 1000.0;
    let result = Metadata { header: header.iter().map(native_json).collect(), distinct, expected };
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    ensure!(distinct == count as i64 && result.expected.len() == count, "metadata cardinality differs");
    ensure!(result.expected.values().all(|set| set == &BTreeSet::from([DIMENSION as i64])), "wrong fixture dimensions");
    ensure!(counter.queries == if mode == "historical_metadata" { 2 } else { 2 + 2 * count.div_ceil(8) }, "query count differs");
    Ok(Sample { result, elapsed_ms, id_phase_ms, dimensions_ms, queries: counter.queries, returned_text_bytes: counter.returned_text_bytes })
}

fn normal_payload(count: usize) -> String {
    // Match the production Value boundary: embedding components originate as
    // f32 and json!(Publication) converts them to serde_json::Value before save.
    let chunks: Vec<_> = (0..count).map(|index| json!({
        "id": uuid(1000 + index as u128),
        "chunk": {"content": "a".repeat(2400), "locator": {"parser":"message-batch-ranges-v1","source_event_id":uuid(5),"message_index":index,"session_id":"fixture"}},
        "embedding": vec![0.012345679_f32; DIMENSION], "summary":"", "search_terms":"fixture"
    })).collect();
    json!({"schema_version":1,"expected_version":null,
        "accepted_spec":{"parser":"message-batch-ranges-v1","parser_version":1,"chunking_version":1,"raw_only":true,"embedding_profile":"fixture"},
        "publication":{"generation":1,"profile":"fixture","summary_model":null,"graph":null,
            "memories":[{"asset":uuid(4),"fact_key":null,"expected_version":null,"version":1,"chunks":chunks}]}
    }).to_string()
}
fn median(values: &[f64]) -> f64 {
    let mut ordered = values.to_vec();
    ordered.sort_by(f64::total_cmp);
    let middle = ordered.len() / 2;
    if ordered.len().is_multiple_of(2) { (ordered[middle - 1] + ordered[middle]) / 2.0 } else { ordered[middle] }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    ensure!(args.len() == 3 || args.len() == 5, "usage: benchmark SQL-EVIDENCE.json NEW-WORK-DIR [WARMUPS REPEATS]");
    let warmups: usize = args.get(3).map_or(Ok(2), |v| v.parse())?;
    let repeats: usize = args.get(4).map_or(Ok(5), |v| v.parse())?;
    ensure!((1..=10).contains(&warmups) && (3..=21).contains(&repeats), "invalid repetition bounds");
    let bundle: Json = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    ensure!(bundle["schema_version"] == 1 && bundle["counts"] == json!(COUNTS) && bundle["dimension"] == DIMENSION, "unreviewed matrix");
    let variants = bundle["variants"].as_array().context("missing variants")?;
    ensure!((2..=3).contains(&variants.len()), "expected baseline/current and at most one reviewed candidate");
    let work = PathBuf::from(&args[2]);
    std::fs::create_dir(&work).context("work directory must be new")?;
    let path = work.join("metadata.db");
    let database = turso::Builder::new_local(path.to_str().context("non-UTF8 fixture path")?).build().await?;
    let conn = database.connect()?;
    conn.execute("CREATE TABLE oc_jobs(tenant_id BLOB NOT NULL,workspace_id BLOB NOT NULL,id BLOB NOT NULL,operation TEXT,state TEXT,generation INTEGER,asset_id BLOB,source_event_id BLOB,payload TEXT,PRIMARY KEY(tenant_id,workspace_id,id))", Vec::<Value>::new()).await?;
    report(json!({"event":"configuration","engine":"native turso 0.8.2; verify resolved lock separately","os":std::env::consts::OS,"arch":std::env::consts::ARCH,"warmups":warmups,"repeats":repeats,"counts":COUNTS,"dimension":DIMENSION,"scope":"metadata only; excludes permissions, source/index/ledger checks, native adapter wrapping and transaction begin/commit","sql_evidence":bundle}))?;
    for count in COUNTS {
        let payload = normal_payload(count);
        let payload_bytes = payload.len();
        conn.execute("DELETE FROM oc_jobs", Vec::<Value>::new()).await?;
        let mut params = scope_job();
        params.extend([blob(4), blob(5), Value::Text(payload)]);
        conn.execute("INSERT INTO oc_jobs VALUES(?,?,?,'message_batch_ingest','completed',1,?,?,?)", params).await?;
        let mut timings: Vec<Vec<f64>> = vec![Vec::new(); variants.len()];
        for round in 0..warmups + repeats {
            let mut results = Vec::new();
            // Reverse order every round to expose order/cache effects.
            let mut order: Vec<_> = (0..variants.len()).collect();
            order.rotate_left(round % variants.len());
            for index in order {
                conn.execute("BEGIN", Vec::<Value>::new()).await?;
                let sample = metadata(&conn, &variants[index], count).await;
                let sample = match sample {
                    Ok(sample) => { conn.execute("COMMIT", Vec::<Value>::new()).await?; sample }
                    Err(error) => {
                        let _ = conn.execute("ROLLBACK", Vec::<Value>::new()).await;
                        report(json!({"event":"error","variant":variants[index]["label"],"chunks":count,"round":round,"error":error.to_string(),"remaining":"NOT_RUN"}))?;
                        return Err(error);
                    }
                };
                let warmup = round < warmups;
                if !warmup { timings[index].push(sample.elapsed_ms); }
                report(json!({"event":"sample","variant":variants[index]["label"],"chunks":count,"payload_bytes":payload_bytes,"round":round,"warmup":warmup,"elapsed_ms":sample.elapsed_ms,"id_phase_ms":sample.id_phase_ms,"dimensions_ms":sample.dimensions_ms,"sql_queries":sample.queries,"returned_text_bytes":sample.returned_text_bytes}))?;
                results.push(sample.result);
            }
            ensure!(results.iter().all(|result| result == &results[0]), "variant metadata differ at count {count}, round {round}");
        }
        report(json!({"event":"summary","chunks":count,"dimension":DIMENSION,"payload_bytes":payload_bytes,"equal_results":true,"warmups":warmups,"repeats":repeats,"variants": variants.iter().zip(&timings).map(|(variant, samples)| json!({"label":variant["label"],"samples_ms":samples,"median_ms":median(samples)})).collect::<Vec<_>>()}))?;
    }
    Ok(())
}

#[cfg(test)]
mod jsonb_tests;
