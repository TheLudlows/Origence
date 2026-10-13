//! Isolated, synthetic LanceDB 0.23.1 probe. Never opens an application database.
mod fixture;
mod policy;

use anyhow::{Context, Result, ensure};
use arrow_array::{Array, Float32Array, Int32Array, Int64Array, RecordBatch, StringArray};
use fixture::{Bucket, CANDIDATE_LIMIT, Config, ELIGIBILITY_BATCH, FRESH_ROWS, Row};
use futures::TryStreamExt;
use lancedb::{
    DistanceType, Table,
    index::{
        Index,
        vector::{IvfFlatIndexBuilder, IvfHnswPqIndexBuilder, IvfPqIndexBuilder},
    },
    query::{ExecutableQuery, HasQuery, QueryBase, VectorQuery},
    table::{CompactionOptions, OptimizeAction, OptimizeOptions},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    fs::OpenOptions,
    io::Write,
    path::Path,
    time::Instant,
};

const INDEX_NAME: &str = "t0_vector_l2";

#[derive(Debug, Clone, Serialize)]
struct Hit {
    id: String,
    source: String,
    tenant: String,
    workspace: String,
    version: i32,
    generation: i64,
    distance: f32,
    score: f32,
}

fn decode(batches: Vec<RecordBatch>) -> Result<Vec<Hit>> {
    let mut hits = Vec::new();
    for batch in batches {
        let strings = |name: &str| -> Result<&StringArray> {
            batch
                .column_by_name(name)
                .context("missing string column")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("wrong string type")
        };
        let ids = strings("id")?;
        let sources = strings("source_id")?;
        let tenants = strings("tenant")?;
        let workspaces = strings("workspace")?;
        let versions = batch
            .column_by_name("version")
            .context("missing version")?
            .as_any()
            .downcast_ref::<Int32Array>()
            .context("wrong version type")?;
        let generations = batch
            .column_by_name("generation")
            .context("missing generation")?
            .as_any()
            .downcast_ref::<Int64Array>()
            .context("wrong generation type")?;
        let distances = batch
            .column_by_name("_distance")
            .context("missing distance")?
            .as_any()
            .downcast_ref::<Float32Array>()
            .context("wrong distance type")?;
        for i in 0..batch.num_rows() {
            ensure!(!distances.is_null(i), "null distance");
            let distance = distances.value(i);
            hits.push(Hit {
                id: ids.value(i).into(),
                source: sources.value(i).into(),
                tenant: tenants.value(i).into(),
                workspace: workspaces.value(i).into(),
                version: versions.value(i),
                generation: generations.value(i),
                distance,
                score: 1.0 / (1.0 + distance),
            });
        }
    }
    sort_hits(&mut hits);
    Ok(hits)
}

fn sort_hits(hits: &mut [Hit]) {
    // Origence orders by score, then artifact ID. Use that exact mapping, not cosine.
    hits.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
}

fn query(
    table: &Table,
    embedding: &[f32],
    filter: &str,
    ann: bool,
    config: &Config,
) -> Result<VectorQuery> {
    let mut query = table
        .query()
        .nearest_to(embedding)?
        .distance_type(DistanceType::L2)
        .only_if(filter)
        .limit(CANDIDATE_LIMIT);
    // 0.23.1 has postfilter(), but no public prefilter() builder. Be explicit through
    // its public HasQuery trait; do not rely solely on today's default.
    query.mut_query().prefilter = true;
    query.mut_query().fast_search = false;
    if ann {
        query = query.nprobes(config.nprobes);
        if config.index_kind != "ivf_flat" {
            // Quantized distances must never flow directly into 1/(1+L2) scores.
            query = query.refine_factor(config.refine_factor);
        }
        if config.index_kind == "ivf_hnsw_pq" {
            query = query.ef(CANDIDATE_LIMIT * config.refine_factor as usize);
        }
    } else {
        query = query.bypass_vector_index();
    }
    let request = query.clone().into_request();
    ensure!(
        request.base.prefilter && !request.base.fast_search,
        "unsafe query mode"
    );
    ensure!(request.use_index == ann, "wrong exact/ANN mode");
    Ok(query)
}

fn native_ann_plan(plan: &str) -> bool {
    // Fail closed: an unrecognized plan is not accepted as ANN evidence. Full text
    // is retained for human review; this is not inferred from latency or presence of an index.
    plan.contains("ANNIVF") || plan.contains("ANNIvf") || plan.contains("ANNHNSW")
}

async fn execute(query: &VectorQuery) -> Result<(Vec<Hit>, f64)> {
    let started = Instant::now();
    let batches = query
        .execute()
        .await?
        .try_collect::<Vec<RecordBatch>>()
        .await?;
    let hits = decode(batches)?;
    Ok((hits, started.elapsed().as_secs_f64() * 1000.0))
}

fn recall(exact: &[Hit], actual: &[Hit], k: usize) -> Option<f64> {
    let expected: HashSet<_> = exact.iter().take(k).map(|h| &h.id).collect();
    if expected.is_empty() {
        return None;
    }
    let got: HashSet<_> = actual.iter().take(k).map(|h| &h.id).collect();
    Some(expected.intersection(&got).count() as f64 / expected.len() as f64)
}

fn same_exact(a: &[Hit], b: &[Hit]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.id == b.id
                && a.source == b.source
                && a.tenant == b.tenant
                && a.workspace == b.workspace
                && a.version == b.version
                && a.generation == b.generation
                && (a.distance - b.distance).abs() <= 0.0001 * (1.0 + a.distance.abs())
                && (a.score - b.score).abs() <= 0.00001
        })
}

fn invariants(
    hits: &[Hit],
    rows: &[Row],
    by_id: &HashMap<&str, usize>,
    bucket: &Bucket,
    eligible: &HashSet<usize>,
    embedding: &[f32],
) -> Value {
    let mut visibility = 0;
    let mut source = 0;
    let mut distance = 0;
    let mut seen = HashSet::new();
    let mut duplicates = 0;
    for hit in hits {
        if !seen.insert(&hit.id) {
            duplicates += 1;
        }
        let Some(&i) = by_id.get(hit.id.as_str()) else {
            visibility += 1;
            source += 1;
            continue;
        };
        let row = &rows[i];
        if !eligible.contains(&i)
            || !bucket.accepts(row)
            || hit.tenant != row.tenant
            || hit.workspace != row.workspace
        {
            visibility += 1;
        }
        if hit.source != row.source
            || hit.version != row.version
            || hit.generation != row.generation
        {
            source += 1;
        }
        // Lance L2's returned value is squared Euclidean distance. This verifies
        // against stored full vectors, not against another approximate result.
        let l2: f64 = row
            .vector
            .iter()
            .zip(embedding)
            .map(|(&a, &b)| (a as f64 - b as f64).powi(2))
            .sum();
        if !hit.distance.is_finite()
            || hit.distance < 0.0
            || (hit.distance as f64 - l2).abs() > 0.0005 * (1.0 + l2)
            || (hit.score as f64 - 1.0 / (1.0 + l2)).abs() > 0.0005
        {
            distance += 1;
        }
    }
    json!({"visibility_violations":visibility,"source_violations":source,
        "score_distance_violations":distance,"duplicate_ids":duplicates})
}

fn empty_invariants() -> Value {
    json!({"visibility_violations":0,"source_violations":0,"score_distance_violations":0,"duplicate_ids":0})
}

fn add_invariants(total: &mut Value, checks: &Value) {
    for key in [
        "visibility_violations",
        "source_violations",
        "score_distance_violations",
        "duplicate_ids",
    ] {
        total[key] = json!(total[key].as_u64().unwrap_or(0) + checks[key].as_u64().unwrap_or(0));
    }
}

fn clean_invariants(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|o| o.values().all(|v| v.as_u64() == Some(0)))
}

// This is only a synthetic authority surrogate. Production SQLite transactions,
// context routes, identities, withdrawals and source events must be tested separately.
fn final_verify(
    hits: &[Hit],
    rows: &[Row],
    by_id: &HashMap<&str, usize>,
    bucket: &Bucket,
    eligible: &HashSet<usize>,
    withdrawn: &HashSet<String>,
) -> Vec<Hit> {
    hits.iter()
        .filter(|h| {
            by_id.get(h.id.as_str()).is_some_and(|&i| {
                let row = &rows[i];
                eligible.contains(&i)
                    && bucket.accepts(row)
                    && !withdrawn.contains(&h.id)
                    && h.tenant == row.tenant
                    && h.workspace == row.workspace
                    && h.source == row.source
                    && h.version == row.version
                    && h.generation == row.generation
            })
        })
        .cloned()
        .collect()
}

async fn measured_pair(
    table: &Table,
    rows: &[Row],
    bucket: &Bucket,
    number: usize,
    config: &Config,
    phase: &str,
    version: u64,
) -> Result<Value> {
    let enumerate_started = Instant::now();
    let eligible = bucket.eligible(rows);
    bucket.validate_cardinality(eligible.len())?;
    let eligibility_ms = enumerate_started.elapsed().as_secs_f64() * 1000.0;
    let eligible_set: HashSet<_> = eligible.iter().copied().collect();
    let lookup_started = Instant::now();
    let by_id: HashMap<_, _> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| (r.id.as_str(), i))
        .collect();
    let lookup_ms = lookup_started.elapsed().as_secs_f64() * 1000.0;
    let embedding = fixture::query_vector(rows, &eligible, number, config.dimension);
    let filter_started = Instant::now();
    let filter = bucket.filter(rows, &eligible);
    let filter_ms = filter_started.elapsed().as_secs_f64() * 1000.0;
    let exact_query = query(table, &embedding, &filter, false, config)?;
    let ann_query = query(table, &embedding, &filter, true, config)?;
    let plan_started = Instant::now();
    let exact_plan = exact_query.explain_plan(true).await?;
    let ann_plan = ann_query.explain_plan(true).await?;
    let explain_ms = plan_started.elapsed().as_secs_f64() * 1000.0;
    ensure!(
        table.version().await? == version,
        "snapshot changed before pair"
    );
    // Alternate native order, but plan construction precedes every pair. These
    // prepared smoke samples are neither cold nor statistically balanced at odd counts.
    let (exact, exact_ms, ann, ann_ms) = if number.is_multiple_of(2) {
        let (exact, exact_ms) = execute(&exact_query).await?;
        let (ann, ann_ms) = execute(&ann_query).await?;
        (exact, exact_ms, ann, ann_ms)
    } else {
        let (ann, ann_ms) = execute(&ann_query).await?;
        let (exact, exact_ms) = execute(&exact_query).await?;
        (exact, exact_ms, ann, ann_ms)
    };
    ensure!(
        table.version().await? == version,
        "snapshot changed during pair"
    );
    let exact_checks = invariants(&exact, rows, &by_id, bucket, &eligible_set, &embedding);
    let ann_checks = invariants(&ann, rows, &by_id, bucket, &eligible_set, &embedding);
    let verify_started = Instant::now();
    let verified = final_verify(&ann, rows, &by_id, bucket, &eligible_set, &HashSet::new());
    let verification_ms = verify_started.elapsed().as_secs_f64() * 1000.0;
    // Forced exact route is tested on EVERY bucket, not merely a conditional branch.
    let (fallback, fallback_ms) = execute(&exact_query).await?;
    let fallback_checks = invariants(&fallback, rows, &by_id, bucket, &eligible_set, &embedding);
    let fallback_matches = same_exact(&exact, &fallback) && clean_invariants(&fallback_checks);
    let recall10 = recall(&exact, &verified, 10);
    let recall100 = recall(&exact, &verified, 100);
    let expected_count = eligible.len().min(CANDIDATE_LIMIT);
    let correct_count = exact.len() == expected_count && (expected_count != 0 || ann.is_empty());
    let raw_recall_pass = recall10.is_none_or(|r| r >= 0.95) && recall100.is_none_or(|r| r >= 0.95);
    let ann_evidence = native_ann_plan(&ann_plan);
    let exact_evidence = !native_ann_plan(&exact_plan);
    // Empty predicates can legitimately optimize out every native operator.
    let pass = correct_count
        && raw_recall_pass
        && fallback_matches
        && exact_evidence
        && (eligible.is_empty() || ann_evidence)
        && clean_invariants(&exact_checks)
        && clean_invariants(&ann_checks);
    let mut result = json!({"phase":phase,"bucket":bucket.name,"query":number,"snapshot":version,
        "candidate_limit":CANDIDATE_LIMIT,"eligible_count":eligible.len(),"exact_count":exact.len(),"ann_count":ann.len(),
        "recall_at_10":recall10,"recall_at_100":recall100,"raw_ann_recall_pass":raw_recall_pass,
        "empty_recall_convention":"null; must return zero rows","fallback_exact_match":fallback_matches,
        "native_ann_proved":ann_evidence,"exact_bypass_proved":exact_evidence,
        "exact_invariants":exact_checks,"ann_invariants":ann_checks,"fallback_invariants":fallback_checks,
        "synthetic_eligibility_ms":eligibility_ms,"synthetic_authority_lookup_build_ms":lookup_ms,"filter_build_ms":filter_ms,"explain_ms":explain_ms,
        "exact_native_ms":exact_ms,"ann_native_ms":ann_ms,"fallback_native_ms":fallback_ms,
        "synthetic_final_verification_ms":verification_ms,"exact_first":number.is_multiple_of(2),
        "exact_plan":exact_plan,"ann_plan":ann_plan,"exact_hits":exact,"ann_hits":ann,
        "verified_ann_hits":verified,"fallback_hits":fallback,"passed":pass});
    // Measure the existing 512-ID shape once per bucket/phase. This is not the
    // coarser one-query ANN architecture; report its overhead separately.
    if number == 0 {
        let batching_started = Instant::now();
        let mut batched = Vec::new();
        let mut batched_exact = Vec::new();
        let mut native_ms = 0.0;
        let mut native_exact_ms = 0.0;
        let mut batching_filter_ms = 0.0;
        let mut native_calls = 0;
        let mut raw_ann_checks = empty_invariants();
        let mut raw_exact_checks = empty_invariants();
        for ids in eligible.chunks(ELIGIBILITY_BATCH) {
            let build_started = Instant::now();
            let expression = bucket.filter(rows, ids);
            batching_filter_ms += build_started.elapsed().as_secs_f64() * 1000.0;
            let chunk_ids: HashSet<_> = ids.iter().copied().collect();
            let (hits, elapsed) =
                execute(&query(table, &embedding, &expression, true, config)?).await?;
            add_invariants(
                &mut raw_ann_checks,
                &invariants(&hits, rows, &by_id, bucket, &chunk_ids, &embedding),
            );
            batched.extend(hits);
            native_ms += elapsed;
            let (hits, elapsed) =
                execute(&query(table, &embedding, &expression, false, config)?).await?;
            add_invariants(
                &mut raw_exact_checks,
                &invariants(&hits, rows, &by_id, bucket, &chunk_ids, &embedding),
            );
            batched_exact.extend(hits);
            native_exact_ms += elapsed;
            native_calls += 1;
        }
        let raw_ann_count = batched.len();
        let raw_exact_count = batched_exact.len();
        let merge_started = Instant::now();
        sort_hits(&mut batched);
        batched.truncate(CANDIDATE_LIMIT);
        let ann_merge_ms = merge_started.elapsed().as_secs_f64() * 1000.0;
        let exact_merge_started = Instant::now();
        sort_hits(&mut batched_exact);
        batched_exact.truncate(CANDIDATE_LIMIT);
        let exact_merge_ms = exact_merge_started.elapsed().as_secs_f64() * 1000.0;
        let verification_started = Instant::now();
        let final_hits = final_verify(
            &batched,
            rows,
            &by_id,
            bucket,
            &eligible_set,
            &HashSet::new(),
        );
        let final_ms = verification_started.elapsed().as_secs_f64() * 1000.0;
        let checks = invariants(&batched, rows, &by_id, bucket, &eligible_set, &embedding);
        let batched_exact_match = same_exact(&exact, &batched_exact);
        let r10 = recall(&exact, &final_hits, 10);
        let r100 = recall(&exact, &final_hits, 100);
        let batch_pass = batched_exact_match
            && clean_invariants(&checks)
            && clean_invariants(&raw_ann_checks)
            && clean_invariants(&raw_exact_checks)
            && r10.is_none_or(|r| r >= 0.95)
            && r100.is_none_or(|r| r >= 0.95);
        result["batched_512"] = json!({"native_calls_per_mode":native_calls,"filter_build_ms":batching_filter_ms,
            "ann_native_ms":native_ms,"exact_native_ms":native_exact_ms,"ann_merge_ms":ann_merge_ms,"exact_merge_ms":exact_merge_ms,
            "synthetic_final_verification_ms":final_ms,"all_measured_modes_ms":batching_started.elapsed().as_secs_f64()*1000.0,
            "selected_ann_component_sum_ms":eligibility_ms+lookup_ms+batching_filter_ms+native_ms+ann_merge_ms+final_ms,
            "recall_at_10":r10,"recall_at_100":r100,"exact_matches_one_query":batched_exact_match,
            "invariants":checks,"raw_ann_invariants":raw_ann_checks,"raw_exact_invariants":raw_exact_checks,
            "raw_ann_count":raw_ann_count,"raw_exact_count":raw_exact_count,
            "ann_hits":batched,"verified_ann_hits":final_hits,"exact_hits":batched_exact,"passed":batch_pass});
        result["passed"] = json!(pass && batch_pass);
        // Adversarial final-authority checks, including same ID with stale source/version.
        if let Some(hit) = exact.first() {
            let mut attacks = Vec::new();
            let mut altered = hit.clone();
            altered.source = "incorrect-source".into();
            attacks.push(altered);
            let mut altered = hit.clone();
            altered.version += 1;
            attacks.push(altered);
            let mut altered = hit.clone();
            altered.generation += 1;
            attacks.push(altered);
            let mut altered = hit.clone();
            altered.tenant = "incorrect-tenant".into();
            attacks.push(altered);
            let mut altered = hit.clone();
            altered.workspace = "incorrect-workspace".into();
            attacks.push(altered);
            let rejected = final_verify(
                &attacks,
                rows,
                &by_id,
                bucket,
                &eligible_set,
                &HashSet::new(),
            )
            .is_empty();
            let withdrawn = HashSet::from([hit.id.clone()]);
            let withdrew = final_verify(
                std::slice::from_ref(hit),
                rows,
                &by_id,
                bucket,
                &eligible_set,
                &withdrawn,
            )
            .is_empty();
            result["synthetic_authority_adversaries"] = json!({"wrong_source_version_generation_scope_rejected":rejected,
                "withdrawal_between_native_and_final_rejected":withdrew});
            result["passed"] =
                json!(result["passed"].as_bool() == Some(true) && rejected && withdrew);
        }
    }
    Ok(result)
}

async fn layout(table: &Table) -> Result<Value> {
    let stats = table.stats().await?;
    Ok(
        json!({"version":table.version().await?,"rows":stats.num_rows,
        "fragments":stats.fragment_stats.num_fragments,"bytes":stats.total_bytes}),
    )
}

async fn index_evidence(table: &Table, config: &Config) -> Result<Value> {
    let indices = table.list_indices().await?;
    let expected_type = match config.index_kind.as_str() {
        "ivf_flat" => "IVF_FLAT",
        "ivf_pq" => "IVF_PQ",
        "ivf_hnsw_pq" => "IVF_HNSW_PQ",
        _ => unreachable!(),
    };
    ensure!(
        indices.len() == 1 && indices[0].name == INDEX_NAME,
        "unexpected index list"
    );
    ensure!(
        indices[0].index_type.to_string() == expected_type,
        "created wrong index type"
    );
    ensure!(indices[0].columns == ["vector"], "indexed wrong column");
    let stats = table
        .index_stats(INDEX_NAME)
        .await?
        .context("index statistics missing")?;
    ensure!(
        stats.index_type.to_string() == expected_type,
        "index stats type mismatch"
    );
    ensure!(
        stats.distance_type == Some(DistanceType::L2),
        "index metric mismatch"
    );
    Ok(
        json!({"list_indices":indices.iter().map(|i| json!({"name":i.name,"type":i.index_type.to_string(),"columns":i.columns})).collect::<Vec<_>>(),
        "index_stats":{"indexed_rows":stats.num_indexed_rows,"unindexed_rows":stats.num_unindexed_rows,
        "type":stats.index_type.to_string(),"distance_type":"L2","num_indices":stats.num_indices,"loss":stats.loss}}),
    )
}

async fn measure_phase(
    table: &Table,
    rows: &[Row],
    config: &Config,
    phase: &str,
    report: &mut Value,
) -> Result<()> {
    if report["schema"] == policy::SCHEMA {
        return policy::measure_phase(table, rows, config, phase, report).await;
    }
    let version = table.version().await?;
    table.checkout(version).await?;
    for bucket in fixture::buckets(phase != "indexed") {
        for query in 0..config.queries_per_bucket {
            let result = measured_pair(table, rows, &bucket, query, config, phase, version).await?;
            println!(
                "{}",
                json!({"phase":phase,"bucket":bucket.name,"query":query,"passed":result["passed"]})
            );
            report["queries"]
                .as_array_mut()
                .context("report queries missing")?
                .push(result);
        }
    }
    ensure!(
        table.version().await? == version,
        "pinned phase snapshot changed"
    );
    table.checkout_latest().await?;
    Ok(())
}

async fn probe(config: &Config, report: &mut Value) -> Result<()> {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("runs");
    std::fs::create_dir_all(&parent)?;
    let directory = tempfile::Builder::new()
        .prefix("synthetic-db-")
        .tempdir_in(&parent)?;
    let location = directory.path().to_owned();
    let db = lancedb::connect(location.to_str().context("non-UTF8 temporary path")?)
        .execute()
        .await?;
    let table = db
        .create_empty_table("t0_vectors", fixture::schema(config.dimension))
        .execute()
        .await?;
    let generate_started = Instant::now();
    let mut rows: Vec<_> = (0..config.rows - FRESH_ROWS)
        .map(|i| fixture::make_row(i, config, false))
        .collect();
    report["fixture_generate_ms"] = json!(generate_started.elapsed().as_secs_f64() * 1000.0);
    ensure!(fixture::unique_ids(&rows), "fixture IDs duplicated");
    report["fixture_indexed_sha256"] = json!(fixture::digest(&rows));
    let write_started = Instant::now();
    for chunk in rows.chunks(256) {
        table
            .add(fixture::reader(chunk, config.dimension)?)
            .execute()
            .await?;
    }
    report["initial_write_ms"] = json!(write_started.elapsed().as_secs_f64() * 1000.0);
    report["layout_before_compact"] = layout(&table).await?;
    let compact_started = Instant::now();
    // Separate physical compaction. Never invoke All, Prune or an implicit default action.
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
    report["compact_ms"] = json!(compact_started.elapsed().as_secs_f64() * 1000.0);
    report["layout_after_compact"] = layout(&table).await?;
    ensure!(
        report["layout_before_compact"]["rows"] == report["layout_after_compact"]["rows"],
        "compaction changed row count"
    );
    let index = match config.index_kind.as_str() {
        "ivf_flat" => Index::IvfFlat(
            IvfFlatIndexBuilder::default()
                .distance_type(DistanceType::L2)
                .num_partitions(config.partitions),
        ),
        "ivf_pq" => Index::IvfPq(
            IvfPqIndexBuilder::default()
                .distance_type(DistanceType::L2)
                .num_partitions(config.partitions)
                .num_sub_vectors((config.dimension / 8) as u32),
        ),
        "ivf_hnsw_pq" => Index::IvfHnswPq(
            IvfHnswPqIndexBuilder::default()
                .distance_type(DistanceType::L2)
                .num_partitions(config.partitions)
                .num_sub_vectors((config.dimension / 8) as u32)
                .ef_construction(2000),
        ),
        _ => unreachable!(),
    };
    let build_started = Instant::now();
    table
        .create_index(&["vector"], index)
        .name(INDEX_NAME.into())
        .replace(false)
        .execute()
        .await?;
    report["index_build_ms"] = json!(build_started.elapsed().as_secs_f64() * 1000.0);
    report["indexed_evidence"] = index_evidence(&table, config).await?;
    ensure!(
        report["indexed_evidence"]["index_stats"]["indexed_rows"].as_u64()
            == Some(rows.len() as u64)
            && report["indexed_evidence"]["index_stats"]["unindexed_rows"].as_u64() == Some(0),
        "initial index coverage incomplete"
    );
    report["layout_after_index"] = layout(&table).await?;
    ensure!(
        report["layout_after_index"]["fragments"] == report["layout_after_compact"]["fragments"],
        "index build changed compacted layout"
    );
    measure_phase(&table, &rows, config, "indexed", report).await?;
    let fresh: Vec<_> = (config.rows - FRESH_ROWS..config.rows)
        .map(|i| fixture::make_row(i, config, true))
        .collect();
    let append_started = Instant::now();
    table
        .add(fixture::reader(&fresh, config.dimension)?)
        .execute()
        .await?;
    report["fresh_append_ms"] = json!(append_started.elapsed().as_secs_f64() * 1000.0);
    rows.extend(fresh);
    report["fixture_final_sha256"] = json!(fixture::digest(&rows));
    report["fresh_evidence_before_refresh"] = index_evidence(&table, config).await?;
    report["layout_before_index_refresh"] = layout(&table).await?;
    ensure!(
        report["fresh_evidence_before_refresh"]["index_stats"]["unindexed_rows"].as_u64()
            == Some(FRESH_ROWS as u64),
        "fresh rows were not demonstrably unindexed"
    );
    measure_phase(&table, &rows, config, "fresh_before_refresh", report).await?;
    let refresh_started = Instant::now();
    // This adds fresh rows to the existing index. It is NOT compaction.
    table
        .optimize(OptimizeAction::Index(OptimizeOptions::default()))
        .await?;
    report["index_refresh_ms"] = json!(refresh_started.elapsed().as_secs_f64() * 1000.0);
    report["fresh_evidence_after_refresh"] = index_evidence(&table, config).await?;
    report["layout_after_index_refresh"] = layout(&table).await?;
    ensure!(
        report["fresh_evidence_after_refresh"]["index_stats"]["unindexed_rows"].as_u64() == Some(0),
        "fresh rows still unindexed after refresh"
    );
    ensure!(
        report["layout_before_index_refresh"]["fragments"]
            == report["layout_after_index_refresh"]["fragments"],
        "Index action unexpectedly compacted data"
    );
    ensure!(
        report["layout_after_index_refresh"]["rows"].as_u64() == Some(config.rows as u64),
        "lifecycle changed rows"
    );
    measure_phase(&table, &rows, config, "fresh_after_refresh", report).await?;
    // Reopen only for durable index metadata and fresh-row visibility, not another benchmark.
    let reopened = db.open_table("t0_vectors").execute().await?;
    report["reopened_evidence"] = index_evidence(&reopened, config).await?;
    if report["schema"] == policy::SCHEMA {
        policy::reopened(&reopened, &rows, config, report).await?;
    } else {
    let fresh_bucket = fixture::buckets(true)
        .pop()
        .context("missing fresh bucket")?;
    report["reopened_fresh_query"] = measured_pair(
        &reopened,
        &rows,
        &fresh_bucket,
        0,
        config,
        "reopened",
        reopened.version().await?,
    )
    .await?;
    }
    drop(reopened);
    drop(table);
    drop(db);
    directory.close()?;
    report["temporary_database_removed"] = json!(!location.exists());
    if report["schema"] == policy::SCHEMA {
        report["setup_sdk_calls"] = json!({"connect":1,"create_empty_table":1,"add":40,"compact":1,"create_index":1,"index_refresh":1,"open_table":1,"list_indices":4,"index_stats":4,"table_stats":5,"table_version":13,"checkout":4,"checkout_latest":3});
        report["call_count_scope"] = json!("Completed explicit LanceDB SDK calls, not underlying file reads/RPCs. Vector execution and explain calls are separate in call_counts.");
    }
    Ok(())
}

#[tokio::main(worker_threads = 2)]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    ensure!(
        args.len() == 3 || (args.len() == 4 && args[3] == "filter-policy-v1"),
        "usage: origence-t0-ann-probe CONFIG.json NEW_REPORT.json [filter-policy-v1]"
    );
    let config: Config = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    config.validate()?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[2])?;
    let mut report = json!({"schema":"origence-t0-ann-v1","status":"RUNNING","config":config,
        "lancedb":"0.23.1","application_baseline":"d0521c0935083e71890ab02c100920c2edaa7227",
        "candidate_limit":CANDIDATE_LIMIT,"score_semantics":"1/(1+returned_L2); returned_L2 verified against squared Euclidean full-vector distance",
        "queries":[],"limitations":["synthetic clustered geometry; not embedding/production recall",
            "in-memory authority surrogate; SQLite, source events, route/identity contracts NOT_RUN",
            "1024D historical 563-row/49.44-second result is not reproduced or comparable",
            "no hard memory limit; row/dimension/query bounds and external process deadline required",
            "no paid compute, models, providers or production database; no production SLO claim",
            "no delete/rebuild crash recovery, concurrent writes, real-corpus replay or multi-seed statistical claim",
            "explain precedes every pair; prepared timings only, not cold/unbiased performance acceptance",
            "fixture seed and SHA256 are fixed; native index-training randomness is not explicitly seeded",
            "selected ANN component sum excludes some setup; it is not complete pipeline wall time",
            "full authorized ID predicate retained; coarser-predicate authorization architecture NOT_IMPLEMENTED"]});
    let policy_mode = args.len() == 4;
    if policy_mode { report = policy::initial_report(&config)?; }
    let started = Instant::now();
    match probe(&config, &mut report).await {
        Ok(()) => {
            let passed = if policy_mode { policy::passed(&report) } else { report["queries"]
                .as_array()
                .is_some_and(|q| !q.is_empty() && q.iter().all(|q| q["passed"] == true))
                && report["reopened_fresh_query"]["passed"] == true };
            report["status"] = json!(if policy_mode { if passed { "PASS_QUALITY" } else { "FAIL_QUALITY" } } else if passed {
                "PASS_SYNTHETIC_T0"
            } else {
                "FAIL_GATE"
            });
        }
        Err(error) => {
            report["status"] = json!("FAILED_RUNTIME");
            // Only synthetic IDs/paths exist here. Do not copy this error reporting into production.
            report["error"] = json!(format!("{error:#}"));
        }
    }
    report["elapsed_ms"] = json!(started.elapsed().as_secs_f64() * 1000.0);
    serde_json::to_writer_pretty(&mut file, &report)?;
    writeln!(file)?;
    println!("{}", json!({"status":report["status"]}));
    ensure!(
        report["status"] == "PASS_SYNTHETIC_T0" || report["status"] == "PASS_QUALITY",
        "T0 did not pass; inspect report"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> Config {
        Config {
            rows: 10_000,
            dimension: 128,
            queries_per_bucket: 2,
            partitions: 64,
            nprobes: 32,
            refine_factor: 10,
            index_kind: "ivf_flat".into(),
            seed: 52,
        }
    }
    #[test]
    fn deterministic_fixture_and_cardinality_edges() {
        let config = config();
        let rows: Vec<_> = (0..config.rows - FRESH_ROWS)
            .map(|i| fixture::make_row(i, &config, false))
            .collect();
        assert!(fixture::unique_ids(&rows));
        assert_eq!(
            rows[17].vector,
            fixture::make_row(17, &config, false).vector
        );
        let buckets = fixture::buckets(false);
        assert_eq!(buckets[0].eligible(&rows).len(), 0);
        assert_eq!(buckets[1].eligible(&rows).len(), 1);
        assert_eq!(buckets[2].eligible(&rows).len(), 7);
        assert_eq!(buckets[3].eligible(&rows).len(), 37);
        assert!(buckets[4].eligible(&rows).len() > 512);
        assert!(buckets[5].eligible(&rows).len() < 100);
    }
    #[test]
    fn merged_filter_keeps_scope_source_version_generation_and_ids() {
        let rows = vec![fixture::make_row(17, &config(), false)];
        let bucket = fixture::buckets(false).remove(8);
        let sql = bucket.filter(&rows, &[0]);
        for required in [
            "tenant = 'tenant-a'",
            "workspace = 'workspace-a'",
            "generation = 1",
            "version = 2",
            "source_id = 'source-0000001'",
            "id IN ('artifact-000000017')",
        ] {
            assert!(sql.contains(required));
        }
        assert!(bucket.filter(&rows, &[]).contains("1 = 0"));
        assert_eq!(fixture::quote("a'b"), "a''b");
    }
    #[test]
    fn recall_counts_missing_results_and_empty_is_not_invented_success() {
        assert_eq!(recall(&[], &[], 10), None);
        assert!(!native_ann_plan("LanceScan: projection=[vector]"));
        assert!(native_ann_plan("ANNIVFPartition: partitions=64"));
        let mut bad = config();
        bad.nprobes = 0;
        assert!(bad.validate().is_err());
        bad = config();
        bad.rows = 10_001;
        assert!(bad.validate().is_err());
    }
}
