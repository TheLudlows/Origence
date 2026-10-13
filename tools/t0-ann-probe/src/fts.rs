//! Bounded raw-chunk FTS capability probe. Full enumeration is an audit oracle,
//! NEVER a selected serving strategy. All authority state is synthetic in memory.
use anyhow::{Context, Result, ensure};
use arrow_array::{
    Array, Float32Array, Int32Array, Int64Array, RecordBatch, RecordBatchIterator,
    RecordBatchReader, StringArray,
};
use arrow_schema::{DataType, Field, Schema};
use futures::TryStreamExt;
use lancedb::{
    Table,
    index::{
        Index,
        scalar::{FtsIndexBuilder, FullTextSearchQuery, MatchQuery, Operator},
    },
    query::{ColumnOrdering, ExecutableQuery, HasQuery, Query, QueryBase},
    table::{CompactionOptions, OptimizeAction, OptimizeOptions},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap, fs::OpenOptions, io::Write, path::Path, sync::Arc, time::Instant,
};

const SCHEMA: &str = "origence-t0-lance-fts-v2";
const INDEX: &str = "t0_chunk_terms";
const CAP: usize = 1000;
const BATCH: usize = 64;
const SCOPE: &str = "tenant = 'tenant-a' AND workspace = 'workspace-a'";

#[derive(Clone, Debug, Serialize)]
struct Row {
    tenant: String,
    workspace: String,
    id: String,
    source: String,
    version: i32,
    generation: i64,
    text: String,
    // Truth labels live outside Lance; they are not production authority.
    eligible: bool,
    state: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
struct Hit {
    tenant: String,
    workspace: String,
    id: String,
    source: String,
    version: i32,
    generation: i64,
    score: f32,
}

fn row(id: &str, text: &str) -> Row {
    Row {
        tenant: "tenant-a".into(),
        workspace: "workspace-a".into(),
        id: id.into(),
        source: format!("source-{id}"),
        version: 2,
        generation: 7,
        text: text.into(),
        eligible: true,
        state: "current".into(),
    }
}

fn fixture() -> Vec<Row> {
    let mut rows = vec![
        row("rank-strong", "quartz quartz quartz"),
        row("rank-weak", &format!("quartz {}", "ballast ".repeat(20))),
        row("and-both", "alpha beta"),
        row("and-alpha", "alpha"),
        row("and-beta", "beta"),
        row("zh-both", "江南 城市"),
        row("zh-first", "江南"),
        row("zh-second", "城市"),
        row("literal", "the Running café alpha-beta AND OR"),
        row("literal-decoy", "run running cafe alpha beta"),
        row("mutable", "beforetoken lifecyclestay"),
        row("deletable", "deleteprobe"),
    ];
    // Reverse physical insertion makes row-ID ties adversarial to artifact-ID order.
    for i in (0..96).rev() {
        rows.push(row(&format!("tie-{i:03}"), "tieanchor parity"));
    }
    for i in 0..12 {
        rows.push(row(
            &format!("authority-ok-{i:02}"),
            "authoritytest ballast ballast",
        ));
    }
    for i in 0..12 {
        let mut r = row(&format!("authority-bad-{i:02}"), "authoritytest");
        r.eligible = false;
        r.state = [
            "fixture_ineligible_version_label",
            "fixture_ineligible_generation_label",
            "fixture_withdrawn_label",
            "fixture_deleted_label",
        ][i % 4]
            .into();
        rows.push(r);
    }
    for i in 0..120 {
        let mut r = row(&format!("foreign-{i:03}"), "quartz alpha beta tieanchor");
        if i % 2 == 0 {
            r.tenant = "tenant-b".into();
        } else {
            r.workspace = "workspace-b".into();
        }
        rows.push(r);
    }
    let mut collision = row("tie-000", "tieanchor parity");
    collision.workspace = "workspace-b".into();
    rows.push(collision);
    rows
}

fn reader(rows: &[Row]) -> Result<Box<dyn RecordBatchReader + Send>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("tenant", DataType::Utf8, false),
        Field::new("workspace", DataType::Utf8, false),
        Field::new("id", DataType::Utf8, false),
        Field::new("source_id", DataType::Utf8, false),
        Field::new("version", DataType::Int32, false),
        Field::new("generation", DataType::Int64, false),
        Field::new("text", DataType::Utf8, false),
    ]));
    let strings =
        |f: fn(&Row) -> &str| Arc::new(StringArray::from(rows.iter().map(f).collect::<Vec<_>>()));
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            strings(|r| &r.tenant),
            strings(|r| &r.workspace),
            strings(|r| &r.id),
            strings(|r| &r.source),
            Arc::new(Int32Array::from(
                rows.iter().map(|r| r.version).collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.generation).collect::<Vec<_>>(),
            )),
            strings(|r| &r.text),
        ],
    )?;
    Ok(Box::new(RecordBatchIterator::new(vec![Ok(batch)], schema)))
}

fn decode(batches: Vec<RecordBatch>) -> Result<Vec<Hit>> {
    let mut hits = Vec::new();
    for batch in batches {
        let string = |name| -> Result<&StringArray> {
            batch
                .column_by_name(name)
                .context("missing FTS string")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("wrong FTS string type")
        };
        let tenant = string("tenant")?;
        let workspace = string("workspace")?;
        let id = string("id")?;
        let source = string("source_id")?;
        let version = batch
            .column_by_name("version")
            .context("version missing")?
            .as_any()
            .downcast_ref::<Int32Array>()
            .context("version type")?;
        let generation = batch
            .column_by_name("generation")
            .context("generation missing")?
            .as_any()
            .downcast_ref::<Int64Array>()
            .context("generation type")?;
        let score = batch
            .column_by_name("_score")
            .context("native _score missing")?
            .as_any()
            .downcast_ref::<Float32Array>()
            .context("native _score type")?;
        for n in 0..batch.num_rows() {
            ensure!(
                !score.is_null(n) && score.value(n).is_finite(),
                "invalid native score"
            );
            hits.push(Hit {
                tenant: tenant.value(n).into(),
                workspace: workspace.value(n).into(),
                id: id.value(n).into(),
                source: source.value(n).into(),
                version: version.value(n),
                generation: generation.value(n),
                score: score.value(n),
            });
        }
    }
    // Deliberately no sorting, filtering, score rewriting or deduplication here.
    Ok(hits)
}

fn query(table: &Table, terms: &[&str], limit: usize, scoped: bool) -> Query {
    query_filter(table, terms, limit, if scoped { Some(SCOPE) } else { None })
}

fn authority_filter(rows: &[Row]) -> String {
    let identities = rows
        .iter()
        .filter(|r| scoped(r) && r.eligible)
        .map(|r| {
            format!(
                "(id = '{}' AND source_id = '{}' AND version = {} AND generation = {})",
                r.id, r.source, r.version, r.generation
            )
        })
        .collect::<Vec<_>>();
    // Fixture identifiers are generated constants containing no quote characters.
    format!("{SCOPE} AND ({})", identities.join(" OR "))
}

fn query_filter(table: &Table, terms: &[&str], limit: usize, filter: Option<&str>) -> Query {
    // Structured Match is literal token input; raw query-string parsing is never used.
    let fts = FullTextSearchQuery::new_query(
        MatchQuery::new(terms.join(" "))
            .with_column(Some("text".into()))
            .with_operator(Operator::And)
            .with_fuzziness(Some(0))
            .into(),
    )
    .wand_factor(Some(1.0));
    let mut q = table.query().full_text_search(fts).limit(limit);
    if let Some(filter) = filter {
        q = q.only_if(filter);
    }
    q.mut_query().prefilter = true;
    q.mut_query().fast_search = false;
    q
}

async fn execute(q: Query) -> Result<(Vec<Hit>, f64)> {
    let start = Instant::now();
    let batches = q.execute().await?.try_collect::<Vec<RecordBatch>>().await?;
    let hits = decode(batches)?;
    Ok((hits, ms(start)))
}

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}
fn key(h: &Hit) -> (&str, &str, &str) {
    (&h.tenant, &h.workspace, &h.id)
}
fn same(r: &Row, h: &Hit) -> bool {
    r.tenant == h.tenant
        && r.workspace == h.workspace
        && r.id == h.id
        && r.source == h.source
        && r.version == h.version
        && r.generation == h.generation
}
fn scoped(r: &Row) -> bool {
    r.tenant == "tenant-a" && r.workspace == "workspace-a"
}
fn ordered(hits: &[Hit]) -> bool {
    hits.windows(2)
        .all(|w| w[0].score > w[1].score || (w[0].score == w[1].score && key(&w[0]) <= key(&w[1])))
}
fn valid_topk(raw: &[Hit], sorted_pool: &[Hit], k: usize) -> bool {
    let wanted = sorted_pool.len().min(k);
    if raw.len() != wanted
        || !raw.windows(2).all(|w| w[0].score >= w[1].score)
        || !raw.iter().all(|h| sorted_pool.contains(h))
    {
        return false;
    }
    if wanted == 0 {
        return true;
    }
    let cut = sorted_pool[wanted - 1].score;
    raw.iter().all(|h| h.score >= cut)
        && sorted_pool
            .iter()
            .filter(|h| h.score > cut)
            .all(|h| raw.contains(h))
}

fn sort_oracle(hits: &mut [Hit]) {
    hits.sort_by(|a, b| b.score.total_cmp(&a.score).then(key(a).cmp(&key(b))));
}

// This bounded application step consumes only freshly checked candidates. It does
// not refill a truncated native TopK, invent a score, or execute production RRF.
fn prepare_rank_inputs(hits: &[Hit]) -> Result<Vec<Hit>> {
    let mut unique: BTreeMap<(String, String, String), Hit> = BTreeMap::new();
    for hit in hits {
        ensure!(hit.score.is_finite() && hit.score > 0.0, "invalid pre-RRF score");
        let scoped_key = (hit.tenant.clone(), hit.workspace.clone(), hit.id.clone());
        if let Some(previous) = unique.get(&scoped_key) {
            ensure!(previous == hit, "conflicting pre-RRF identity or score");
        } else {
            unique.insert(scoped_key, hit.clone());
        }
    }
    let mut prepared: Vec<_> = unique.into_values().collect();
    prepared.sort_by(|a, b| b.score.total_cmp(&a.score).then(key(a).cmp(&key(b))));
    Ok(prepared)
}

// Deliberately a fixture-only batch API, not a Turso call and not a production security test.
fn authority(hits: &[Hit], rows: &[Row], mutation: &str) -> Value {
    let start = Instant::now();
    let mut current = rows.to_vec();
    let mut key_allowed = true;
    if mutation == "revoked_key" {
        key_allowed = false;
    }
    if let Some(first) = hits.first()
        && let Some(r) = current.iter_mut().find(|r| same(r, first))
    {
        match mutation {
            "version" => r.version += 1,
            "generation" => r.generation += 1,
            "source" => r.source.push_str("-replaced"),
            "withdrawn" => r.eligible = false,
            _ => (),
        }
    }
    let mut retained = Vec::new();
    let mut batches = 0;
    for batch in hits.chunks(BATCH) {
        batches += 1;
        for h in batch {
            if current
                .iter()
                .any(|r| same(r, h) && scoped(r) && r.eligible)
            {
                retained.push(h.clone());
            }
        }
    }
    let clean = key_allowed && retained.len() == hits.len();
    // A fresh mismatch fails the whole response closed. It does not silently refill.
    let status = if !key_allowed {
        "AUTHORITY_REVOKED"
    } else if !clean {
        "SNAPSHOT_CHANGED"
    } else {
        "OK"
    };
    json!({"mutation":mutation, "authority_kind":"in_memory_fixture_only", "authority_native_calls":0,
        "batch_size":BATCH, "fixture_batch_calls":batches, "checked_rows":hits.len(),
        "elapsed_ms":ms(start), "status":status, "hits":if clean { retained } else { Vec::new() }})
}

struct Log {
    file: std::fs::File,
    start: Instant,
    events: usize,
}
impl Log {
    fn emit(&mut self, value: Value) -> Result<()> {
        ensure!(
            self.start.elapsed().as_secs_f64() < 180.0,
            "native soft measurement deadline exceeded"
        );
        serde_json::to_writer(&mut self.file, &value)?;
        self.file.write_all(b"\n")?;
        self.file.flush()?;
        self.events += 1;
        Ok(())
    }
}

async fn metadata(table: &Table, phase: &str, rows: &[Row], log: &mut Log) -> Result<()> {
    let start = Instant::now();
    let indices = table.list_indices().await?;
    let stats = table
        .index_stats(INDEX)
        .await?
        .context("FTS index statistics missing")?;
    let version = table.version().await?;
    let layout = table.stats().await?;
    log.emit(json!({"kind":"snapshot", "phase":phase, "fixture":rows, "table_version":version,
        "indices":indices.iter().map(|i|json!({"name":i.name,"type":i.index_type.to_string(),"columns":i.columns})).collect::<Vec<_>>(),
        "stats":{"indexed_rows":stats.num_indexed_rows,"unindexed_rows":stats.num_unindexed_rows,"type":stats.index_type.to_string()},
        "layout":{"rows":layout.num_rows,"fragments":layout.fragment_stats.num_fragments,"bytes":layout.total_bytes},
        "native_calls":4,"elapsed_ms":ms(start)}))
}

async fn case(
    table: &Table,
    rows: &[Row],
    phase: &str,
    name: &str,
    terms: &[&str],
    k: usize,
    log: &mut Log,
) -> Result<()> {
    let total = Instant::now();
    if terms.is_empty() {
        return log.emit(
            json!({"kind":"empty_terms","phase":phase,"name":name,"native_calls":0,"hits":[]}),
        );
    }
    let enumeration = Instant::now();
    let prefiltered = name == "authority_prefilter";
    let filter = if prefiltered {
        authority_filter(rows)
    } else {
        SCOPE.into()
    };
    let enumeration_ms = ms(enumeration);
    let q = query_filter(table, terms, k, Some(&filter));
    let request = q.clone().into_request();
    ensure!(request.prefilter && !request.fast_search, "unsafe FTS mode");
    let (candidate, candidate_ms) = execute(q.clone()).await?;
    let authority_start = Instant::now();
    let mut checked = Vec::new();
    let mut batch_calls = 0;
    for batch in candidate.chunks(BATCH) {
        batch_calls += 1;
        checked.extend(
            batch
                .iter()
                .filter(|h| rows.iter().any(|r| scoped(r) && r.eligible && same(r, h)))
                .cloned(),
        );
    }
    let authority_ms = ms(authority_start);
    let fresh = authority(&checked, rows, "none");
    ensure!(
        fresh["status"] == "OK",
        "fresh authority did not permit rank preparation"
    );
    let application_started = Instant::now();
    let rank_inputs = prepare_rank_inputs(&checked)?;
    let application_ms = ms(application_started);
    let application_stage = json!({
        "schema":"origence-t0-pre-rrf-v1", "stage":"after_fresh_authority_before_rrf",
        "input":"fresh_check.hits", "input_count":checked.len(), "output_count":rank_inputs.len(),
        "deduplicated_count":checked.len()-rank_inputs.len(), "rank_inputs":rank_inputs,
        "rank_positions":(1..=rank_inputs.len()).collect::<Vec<_>>(),
        "native_calls":0, "elapsed_ms":application_ms, "rrf_executed":false
    });
    let candidate_wall_ms = ms(total);
    // Oracle is outside measured candidate wall, separate native call, bounded to CAP+1.
    let (oracle_raw, oracle_ms) = execute(query(table, terms, CAP + 1, true)).await?;
    let expected_matches = rows
        .iter()
        .filter(|r| {
            scoped(r)
                && terms
                    .iter()
                    .all(|t| r.text.split_whitespace().any(|w| w == *t))
        })
        .count();
    let complete = oracle_raw.len() == expected_matches && oracle_raw.len() <= CAP;
    let mut oracle = oracle_raw.clone();
    sort_oracle(&mut oracle);
    let competition: Vec<_> = oracle
        .iter()
        .filter(|h| !prefiltered || rows.iter().any(|r| scoped(r) && r.eligible && same(r, h)))
        .cloned()
        .collect();
    let topk: Vec<_> = competition.iter().take(k).cloned().collect();
    let eligible_topk: Vec<_> = oracle
        .iter()
        .filter(|h| rows.iter().any(|r| scoped(r) && r.eligible && same(r, h)))
        .cloned()
        .collect();
    let score_ordered = candidate.windows(2).all(|w| w[0].score >= w[1].score);
    let bm25_topk = complete && valid_topk(&candidate, &competition, k);
    let strict = complete && candidate == topk && ordered(&candidate);
    let filtered = complete && valid_topk(&checked, &eligible_topk, k);
    let plan_start = Instant::now();
    let plan = q.explain_plan(true).await?;
    let plan_ms = ms(plan_start);
    log.emit(json!({"kind":"case","phase":phase,"name":name,"terms":terms,"k":k,
        "scope_filter":filter,"prefilter_mode":if prefiltered {"fixture_full_identity"} else {"scope_only"},
        "enumerated_authority_rows":if prefiltered {rows.len()} else {0},"prefilter":true,"fast_search":false,"operator":"AND","fuzziness":0,"wand_factor":1.0,
        "candidate_raw":candidate,"oracle_raw":oracle_raw,"oracle_only":true,"oracle_complete":complete,
        "expected_scoped_matches":expected_matches,"checked_candidates":checked,"fresh_check":fresh,
        "native_bm25_topk":bm25_topk,"native_score_descending":score_ordered,
        "native_strict_topk":strict,"native_filtered_topk":filtered,"application_stage":application_stage,
        "candidate_status":if name == "deep_authority" && !filtered && bm25_topk {"EXPECTED_NEGATIVE_CONTROL"} else if filtered && bm25_topk {"PASS_BOUNDED_FIXTURE"} else {"BLOCKER"},
        "plan":plan,"calls":{"candidate":1,"oracle":1,"plan":1,"authority_native":0,"authority_fixture_batches":batch_calls},
        "timing_ms":{"eligibility_fixture_enumeration":enumeration_ms,"candidate_native":candidate_ms,"candidate_authority_fixture":authority_ms,"application_prepare":application_ms,"candidate_total":candidate_wall_ms,"oracle_native":oracle_ms,"plan":plan_ms}}))?;
    // Independent fresh-authority mutation fixture; these do not claim concurrent native DB writes.
    if phase == "indexed" && name == "rank" {
        for mutation in [
            "version",
            "generation",
            "source",
            "withdrawn",
            "revoked_key",
        ] {
            log.emit(
                json!({"kind":"fresh_authority","phase":phase,"input_hits":checked,
                "result":authority(&checked, rows, mutation)}),
            )?;
        }
    }
    Ok(())
}

async fn lifecycle_cases(table: &Table, rows: &[Row], phase: &str, log: &mut Log) -> Result<()> {
    for (name, term) in [
        ("old_term", "beforetoken"),
        ("new_term", "aftertoken"),
        ("deleted_term", "deleteprobe"),
        ("fresh_term", "freshlexeme"),
    ] {
        case(table, rows, phase, name, &[term], 10, log).await?;
    }
    case(table, rows, phase, "statistics_rank", &["quartz"], 10, log).await?;
    Ok(())
}

async fn probe(log: &mut Log) -> Result<()> {
    let temp = tempfile::tempdir()?;
    let uri = temp.path().join("lance").to_string_lossy().into_owned();
    let mut rows = fixture();
    ensure!(rows.len() < CAP, "fixture exceeded bounded rung");
    log.emit(json!({"kind":"header","schema":SCHEMA,"lancedb":"0.40.0","arrow":"58",
        "max_rows":CAP,"deadline_seconds":180,"authority_kind":"in_memory_fixture_only",
        "contract":{"id":"native-bm25-tied-topk-pre-rrf-v2","cutoff_ties":"any_bm25_legal_members",
            "dedup":"same_scoped_chunk_and_equal_authority_identity_and_score_only",
            "application_order":"score_desc_scoped_chunk_id_asc","rebuild_member_stability_guaranteed":false},
        "serving_strategy":"native_scoped_topk_with_fixture_authority_prefilter_arm","full_enumeration":"diagnostic_oracle_only",
        "not_run":["turso_authority_integration","production_security","summary_cross_document_AND","summary_placement","learning_closure","production_identity","efficient_tie_complete_refill","10000_row_scale","production_rrf_integration","cross_generation_dedup_integration"],
        "tokenizer":{"base":"whitespace","lower_case":false,"stem":false,"remove_stop_words":false,"ascii_folding":false,"max_token_length":null},
        "source_urls":["https://github.com/lancedb/lancedb/blob/v0.40.0/rust/lancedb/src/query.rs","https://github.com/lance-format/lance/blob/v13.0.0/rust/lance/src/dataset/scanner.rs","https://github.com/lance-format/lance/blob/v13.0.0/rust/lance-index/src/scalar/inverted/query.rs","https://github.com/lance-format/lance/blob/v13.0.0/rust/lance-index/src/scalar/inverted/scorer.rs","https://github.com/lance-format/lance/blob/v13.0.0/rust/lance-index/src/scalar/inverted/tokenizer.rs"]}))?;
    let setup = Instant::now();
    let db = lancedb::connect(&uri).execute().await?;
    let table = db.create_table("chunks", reader(&rows)?).execute().await?;
    let tokenizer = FtsIndexBuilder::default()
        .base_tokenizer("whitespace".into())
        .lower_case(false)
        .stem(false)
        .remove_stop_words(false)
        .ascii_folding(false)
        .max_token_length(None);
    let tokenizer_json = serde_json::to_value(&tokenizer)?;
    table
        .create_index(&["text"], Index::FTS(tokenizer))
        .name(INDEX.into())
        .replace(false)
        .execute()
        .await?;
    log.emit(json!({"kind":"setup","native_calls":3,"elapsed_ms":ms(setup),"index_parameters":tokenizer_json}))?;
    metadata(&table, "indexed", &rows, log).await?;
    for (name, terms, k) in [
        ("rank", vec!["quartz"], 1),
        ("and", vec!["alpha", "beta"], 10),
        ("chinese_and", vec!["江南", "城市"], 10),
        ("stopword", vec!["the"], 10),
        ("case_sensitive", vec!["Running"], 10),
        ("no_stem", vec!["run"], 10),
        ("no_ascii_fold", vec!["café"], 10),
        ("literal_operators", vec!["AND", "OR", "alpha-beta"], 10),
        ("missing", vec!["absentterm"], 10),
        ("ties", vec!["tieanchor"], 10),
        ("deep_authority", vec!["authoritytest"], 10),
        ("authority_prefilter", vec!["authoritytest"], 10),
        ("empty", vec![], 10),
    ] {
        case(&table, &rows, "indexed", name, &terms, k, log).await?;
    }
    // Unscoped control proves adversarial foreign rows really match in the native index.
    let (hits, elapsed) = execute(query(&table, &["quartz"], CAP + 1, false)).await?;
    log.emit(json!({"kind":"unscoped_control","terms":["quartz"],"hits":hits,"native_calls":1,"elapsed_ms":elapsed,"oracle_only":true}))?;
    let order = query(&table, &["tieanchor"], 10, true).order_by(Some(vec![
        ColumnOrdering {
            column_name: "_score".into(),
            ascending: false,
            nulls_first: false,
        },
        ColumnOrdering {
            column_name: "id".into(),
            ascending: true,
            nulls_first: false,
        },
    ]));
    let order_start = Instant::now();
    match execute(order).await {
        Ok((hits,_)) => log.emit(json!({"kind":"ordered_api","native_calls":1,"status":"EXECUTED","hits":hits,"elapsed_ms":ms(order_start)}))?,
        Err(error) => log.emit(json!({"kind":"ordered_api","native_calls":1,"status":"UNSUPPORTED","error":error.to_string(),"elapsed_ms":ms(order_start)}))?,
    }
    lifecycle_cases(&table, &rows, "indexed", log).await?;
    let fresh = row("fresh-row", "freshlexeme");
    let start = Instant::now();
    table
        .add(reader(std::slice::from_ref(&fresh))?)
        .execute()
        .await?;
    rows.push(fresh);
    log.emit(json!({"kind":"mutation","action":"insert","native_calls":1,"elapsed_ms":ms(start)}))?;
    metadata(&table, "fresh_before_refresh", &rows, log).await?;
    lifecycle_cases(&table, &rows, "fresh_before_refresh", log).await?;
    let start = Instant::now();
    table
        .update()
        .only_if("id = 'mutable'")
        .column("text", "'aftertoken lifecyclestay'")
        .execute()
        .await?;
    rows.iter_mut()
        .find(|r| r.id == "mutable")
        .context("mutable fixture absent")?
        .text = "aftertoken lifecyclestay".into();
    log.emit(json!({"kind":"mutation","action":"update","native_calls":1,"elapsed_ms":ms(start)}))?;
    let start = Instant::now();
    table.delete("id = 'deletable'").await?;
    rows.retain(|r| r.id != "deletable");
    log.emit(json!({"kind":"mutation","action":"delete","native_calls":1,"elapsed_ms":ms(start)}))?;
    metadata(&table, "mutated_before_refresh", &rows, log).await?;
    lifecycle_cases(&table, &rows, "mutated_before_refresh", log).await?;
    let start = Instant::now();
    // Index refresh only. No Compact, All or Prune, and no claimed compaction.
    table
        .optimize(OptimizeAction::Index(OptimizeOptions::default()))
        .await?;
    log.emit(json!({"kind":"mutation","action":"OptimizeAction::Index","native_calls":1,"elapsed_ms":ms(start),"compaction":false,"prune":false}))?;
    metadata(&table, "after_refresh", &rows, log).await?;
    lifecycle_cases(&table, &rows, "after_refresh", log).await?;
    let start = Instant::now();
    table
        .optimize(OptimizeAction::Compact {
            options: CompactionOptions {
                target_rows_per_fragment: 4096,
                num_threads: Some(2),
                ..Default::default()
            },
            remap_options: None,
        })
        .await?;
    log.emit(json!({"kind":"mutation","action":"OptimizeAction::Compact","native_calls":1,"elapsed_ms":ms(start),"index_refresh":false,"prune":false}))?;
    metadata(&table, "after_compact", &rows, log).await?;
    lifecycle_cases(&table, &rows, "after_compact", log).await?;
    drop(table);
    drop(db);
    let start = Instant::now();
    let reopened_db = lancedb::connect(&uri).execute().await?;
    let reopened = reopened_db.open_table("chunks").execute().await?;
    log.emit(json!({"kind":"reopen","native_calls":2,"elapsed_ms":ms(start),"mode":"new_connection_same_process"}))?;
    metadata(&reopened, "reopened", &rows, log).await?;
    lifecycle_cases(&reopened, &rows, "reopened", log).await?;
    // Storage operation completion is separate from independent capability acceptance.
    log.emit(json!({"kind":"terminal","status":"COMPLETED_NATIVE_MATRIX","elapsed_ms":ms(log.start),"events_before_terminal":log.events}))?;
    Ok(())
}

pub async fn run(output: &Path) -> Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .context("create new FTS output")?;
    let mut log = Log {
        file,
        start: Instant::now(),
        events: 0,
    };
    match probe(&mut log).await {
        Ok(()) => Ok(()),
        Err(error) => {
            // Save a terminal record even when a soft deadline expired.
            serde_json::to_writer(
                &mut log.file,
                &json!({"kind":"terminal","status":"NATIVE_ERROR","error":error.to_string(),"elapsed_ms":ms(log.start)}),
            )?;
            log.file.write_all(b"\n")?;
            log.file.flush()?;
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(id: &str, score: f32) -> Hit {
        Hit {
            tenant: "tenant-a".into(),
            workspace: "workspace-a".into(),
            id: id.into(),
            source: format!("source-{id}"),
            version: 2,
            generation: 7,
            score,
        }
    }

    #[test]
    fn preparation_deduplicates_before_assigning_ranks() {
        let low = hit("a", 1.0);
        let high = hit("z", 2.0);
        let prepared = prepare_rank_inputs(&[low.clone(), high.clone(), low]).unwrap();
        assert_eq!(prepared, vec![high, hit("a", 1.0)]);
        let ranked_ids: Vec<_> = prepared
            .iter()
            .enumerate()
            .map(|(i, h)| (i + 1, h.id.as_str()))
            .collect();
        assert_eq!(ranked_ids, vec![(1, "z"), (2, "a")]);
    }

    #[test]
    fn preparation_sorts_ties_before_rank_assignment() {
        let raw = vec![hit("z", 1.0), hit("a", 1.0)];
        let prepared = prepare_rank_inputs(&raw).unwrap();
        assert_eq!(prepared, vec![hit("a", 1.0), hit("z", 1.0)]);
        // A downstream rank consumer must enumerate this prepared vector, not raw.
        assert_eq!(raw[0].id, "z");
        assert_eq!(prepared[0].id, "a");
    }

    #[test]
    fn preparation_rejects_conflicting_authority_or_score() {
        let original = hit("a", 1.0);
        for field in ["source", "version", "generation", "score"] {
            let mut conflict = original.clone();
            match field {
                "source" => conflict.source.push_str("-conflict"),
                "version" => conflict.version += 1,
                "generation" => conflict.generation += 1,
                "score" => conflict.score += 1.0,
                _ => unreachable!(),
            }
            assert!(prepare_rank_inputs(&[original.clone(), conflict]).is_err());
        }
    }

    #[test]
    fn preparation_preserves_scope_and_rejects_invalid_scores() {
        let original = hit("a", 1.0);
        let mut other = original.clone();
        other.workspace = "workspace-b".into();
        assert_eq!(
            prepare_rank_inputs(&[original.clone(), other]).unwrap().len(),
            2
        );
        for score in [0.0, f32::NAN, f32::INFINITY, -1.0] {
            assert!(prepare_rank_inputs(&[hit("a", score)]).is_err());
        }
        assert!(prepare_rank_inputs(&[]).unwrap().is_empty());
    }
}
