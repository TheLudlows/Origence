//! Frozen, opt-in count-based routing experiment. Not an application policy.
use super::*;
use sha2::{Digest, Sha256};

pub const SCHEMA: &str = "origence-t0-filter-policy-v1";
pub const POLICY_ID: &str = "eligible4096-fraction40-ivf32of64-v1";
const CONTRACT: &str = include_str!("../policy-contract.json");

#[derive(Clone)]
struct Case {
    name: String,
    bucket: Bucket,
    rotation: usize,
    stride: usize,
    cap: Option<usize>,
}

impl Case {
    fn eligible(&self, rows: &[Row]) -> Vec<usize> {
        let mut ids = self.bucket.eligible(rows);
        if !ids.is_empty() {
            let rotation = self.rotation % ids.len();
            ids.rotate_left(rotation);
        }
        ids.into_iter().step_by(self.stride).take(self.cap.unwrap_or(usize::MAX)).collect()
    }
}

fn cases(include_fresh: bool) -> Vec<Case> {
    let mut cases: Vec<_> = fixture::buckets(include_fresh).into_iter().map(|bucket| Case {
        name: bucket.name.into(), bucket, rotation: 0, stride: 1, cap: None,
    }).collect();
    let dense = cases.iter().find(|c| c.name == "dense_current").expect("fixed dense case").bucket.clone();
    for count in [100, 101, 512, 513, 2048, 4095, 4096, 4097] {
        cases.push(Case { name: format!("count_{count}"), bucket: dense.clone(), rotation: 0, stride: 1, cap: Some(count) });
    }
    for count in [4095, 4096, 4097] {
        cases.push(Case { name: format!("rotated_{count}"), bucket: dense.clone(), rotation: 541, stride: 1, cap: Some(count) });
    }
    for stride in [2, 3] {
        cases.push(Case { name: format!("subsample_{stride}"), bucket: dense.clone(), rotation: 0, stride, cap: None });
    }
    cases
}

// No query vector, answer, returned candidate count or measured recall enters routing.
fn route(eligible: usize, snapshot_rows: usize, force_exact: bool) -> &'static str {
    if eligible == 0 { "empty" }
    else if !force_exact && eligible >= 4096 && eligible * 5 >= snapshot_rows * 2 { "ann" }
    else { "exact" }
}

fn hash_bytes(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
fn hash_vector(vector: &[f32]) -> String {
    let bytes: Vec<_> = vector.iter().flat_map(|v| v.to_bits().to_le_bytes()).collect();
    hash_bytes(&bytes)
}
fn hash_ids(rows: &[Row], eligible: &[usize]) -> String {
    let mut hash = Sha256::new();
    hash.update((eligible.len() as u64).to_le_bytes());
    for &i in eligible {
        hash.update((rows[i].id.len() as u64).to_le_bytes());
        hash.update(rows[i].id.as_bytes());
    }
    format!("{:x}", hash.finalize())
}

fn embedding(rows: &[Row], eligible: &[usize], slot: usize) -> Vec<f32> {
    if eligible.is_empty() { return vec![0.25; 128]; }
    if slot < 4 {
        let offset = 17 + slot;
        let index = eligible[(offset * 997) % eligible.len()];
        rows[index].vector.iter().enumerate().map(|(d, v)| v + ((d + offset) % 11) as f32 * 0.00013).collect()
    } else {
        let bridge = slot - 4;
        let a = &rows[2 * (3 * bridge + 1)].vector;
        let b = &rows[2 * (3 * bridge + 8)].vector;
        a.iter().zip(b).enumerate().map(|(d, (&a, &b))| a * 0.5 + b * 0.5 + ((d + bridge) % 13) as f32 * 0.00017).collect()
    }
}

#[derive(Serialize)]
struct Pipeline {
    route: &'static str,
    wall_ms: f64,
    native_calls: usize,
    eligible_count: usize,
    raw_hits: Vec<Hit>,
    verified_hits: Vec<Hit>,
    components_ms: Value,
}

async fn pipeline(table: &Table, rows: &[Row], case: &Case, vector: &[f32], config: &Config, force_exact: bool) -> Result<Pipeline> {
    // Both arms use this exact boundary and independently build all authority state.
    // Input embedding/table snapshot are provided by the caller, not timed setup.
    let started = Instant::now();
    let component = Instant::now();
    let eligible = case.eligible(rows);
    let eligible_count = eligible.len();
    let eligibility = component.elapsed().as_secs_f64() * 1000.0;
    let component = Instant::now();
    let by_id: HashMap<_, _> = rows.iter().enumerate().map(|(i, r)| (r.id.as_str(), i)).collect();
    let allowed: HashSet<_> = eligible.iter().copied().collect();
    let withdrawn = HashSet::new();
    let authority_map = component.elapsed().as_secs_f64() * 1000.0;
    let component = Instant::now();
    let selected = route(eligible_count, rows.len(), force_exact);
    let prepared = if selected == "empty" { None } else {
        let filter = case.bucket.filter(rows, &eligible);
        Some(query(table, vector, &filter, selected == "ann", config)?)
    };
    let selection_filter_query = component.elapsed().as_secs_f64() * 1000.0;
    let component = Instant::now();
    let raw_hits = match &prepared {
        Some(query) => execute(query).await?.0,
        None => Vec::new(),
    };
    let native_decode_sort = component.elapsed().as_secs_f64() * 1000.0;
    let component = Instant::now();
    let verified_hits = final_verify(&raw_hits, rows, &by_id, &case.bucket, &allowed, &withdrawn);
    let verification = component.elapsed().as_secs_f64() * 1000.0;
    // Include temporary request/authority-state cleanup in the complete wall clock.
    drop(prepared); drop(withdrawn); drop(allowed); drop(by_id); drop(eligible);
    let wall_ms = started.elapsed().as_secs_f64() * 1000.0;
    Ok(Pipeline { route: selected, wall_ms, native_calls: usize::from(selected != "empty"), eligible_count, raw_hits, verified_hits,
        components_ms: json!({"eligibility":eligibility,"authority_map":authority_map,"selection_filter_query":selection_filter_query,"native_decode_sort":native_decode_sort,"verification":verification}) })
}

async fn measure_case(table: &Table, rows: &[Row], case: &Case, config: &Config, phase: &str, snapshot: u64, report: &mut Value) -> Result<()> {
    // This metadata selection is used only for query generation and after-timing audit.
    // Neither measured arm receives it or reuses its costs.
    let eligible = case.eligible(rows);
    let allowed: HashSet<_> = eligible.iter().copied().collect();
    let by_id: HashMap<_, _> = rows.iter().enumerate().map(|(i, r)| (r.id.as_str(), i)).collect();
    let case_id = format!("{phase}/{}", case.name);
    let chosen = route(eligible.len(), rows.len(), false);
    for slot in 0..8 {
        let vector = embedding(rows, &eligible, slot);
        let (selected, exact) = if slot.is_multiple_of(2) {
            let selected = pipeline(table, rows, case, &vector, config, false).await?;
            let exact = pipeline(table, rows, case, &vector, config, true).await?;
            (selected, exact)
        } else {
            let exact = pipeline(table, rows, case, &vector, config, true).await?;
            let selected = pipeline(table, rows, case, &vector, config, false).await?;
            (selected, exact)
        };
        // Oracle execution and all distance/recall/authority auditing are after both timers.
        let filter = case.bucket.filter(rows, &eligible);
        let oracle = if eligible.is_empty() { Vec::new() } else {
            execute(&query(table, &vector, &filter, false, config)?).await?.0
        };
        let selected_raw = invariants(&selected.raw_hits, rows, &by_id, &case.bucket, &allowed, &vector);
        let selected_verified = invariants(&selected.verified_hits, rows, &by_id, &case.bucket, &allowed, &vector);
        let exact_raw = invariants(&exact.raw_hits, rows, &by_id, &case.bucket, &allowed, &vector);
        let exact_verified = invariants(&exact.verified_hits, rows, &by_id, &case.bucket, &allowed, &vector);
        let oracle_checks = invariants(&oracle, rows, &by_id, &case.bucket, &allowed, &vector);
        let recall10 = recall(&oracle, &selected.verified_hits, 10);
        let recall100 = recall(&oracle, &selected.verified_hits, 100);
        let exact_match = same_exact(&oracle, &exact.verified_hits);
        let selected_exact_match = (chosen != "ann").then(|| same_exact(&oracle, &selected.verified_hits));
        let passed = [&selected_raw, &selected_verified, &exact_raw, &exact_verified, &oracle_checks].iter().all(|v| clean_invariants(v))
            && oracle.len() == eligible.len().min(CANDIDATE_LIMIT)
            && same_exact(&selected.raw_hits, &selected.verified_hits)
            && same_exact(&exact.raw_hits, &exact.verified_hits)
            && exact_match && selected_exact_match.is_none_or(|v| v)
            && recall10.is_none_or(|v| v >= 0.95) && recall100.is_none_or(|v| v >= 0.95)
            && selected.eligible_count == eligible.len() && exact.eligible_count == eligible.len()
            && selected.route == chosen && exact.route == route(eligible.len(), rows.len(), true);
        report["call_counts"]["selected_native"] = json!(report["call_counts"]["selected_native"].as_u64().unwrap_or(0) + selected.native_calls as u64);
        report["call_counts"]["exact_native"] = json!(report["call_counts"]["exact_native"].as_u64().unwrap_or(0) + exact.native_calls as u64);
        report["call_counts"]["oracle_native"] = json!(report["call_counts"]["oracle_native"].as_u64().unwrap_or(0) + u64::from(!eligible.is_empty()));
        report["queries"].as_array_mut().context("missing policy queries")?.push(json!({
            "case_id":case_id,"phase":phase,"bucket":case.name,"slot":slot,"query_kind":if slot < 4 {"near"} else {"bridge"},
            "query_vector":vector,"query_sha256":hash_vector(&vector),"selected_first":slot.is_multiple_of(2),
            "selected":selected,"exact":exact,"oracle_hits":oracle,
            "selected_raw_invariants":selected_raw,"selected_verified_invariants":selected_verified,
            "exact_raw_invariants":exact_raw,"exact_verified_invariants":exact_verified,"oracle_invariants":oracle_checks,
            "recall_at_10":recall10,"recall_at_100":recall100,"exact_matches_oracle":exact_match,
            "selected_exact_matches_oracle":selected_exact_match,"quality_pass":passed
        }));
    }
    // Every vector gets actual selected/exact plans, only after all timed pairs.
    // Full identical text is stored once globally, with per-slot hash references.
    let filter = case.bucket.filter(rows, &eligible);
    let mut slots = Vec::new();
    let mut plan_pass = true;
    for slot in 0..8 {
        let vector = embedding(rows, &eligible, slot);
        let (selected_plan, exact_plan) = if eligible.is_empty() {
            ("EMPTY_NO_NATIVE_QUERY".into(), "EMPTY_NO_NATIVE_QUERY".into())
        } else {
            let selected_plan = query(table, &vector, &filter, chosen == "ann", config)?.explain_plan(true).await?;
            let exact_plan = query(table, &vector, &filter, false, config)?.explain_plan(true).await?;
            report["call_counts"]["explain_plans"] = json!(report["call_counts"]["explain_plans"].as_u64().unwrap_or(0) + 2);
            (selected_plan, exact_plan)
        };
        plan_pass &= eligible.is_empty() || (exact_plan.contains("KNNVectorDistance") && !native_ann_plan(&exact_plan)
            && native_ann_plan(&selected_plan) == (chosen == "ann")
            && (chosen == "ann" || selected_plan.contains("KNNVectorDistance"))
            && (chosen != "ann" || (selected_plan.contains("minimum_nprobes=32") && selected_plan.contains("maximum_nprobes=Some(32)"))));
        let selected_hash = hash_bytes(selected_plan.as_bytes());
        let exact_hash = hash_bytes(exact_plan.as_bytes());
        let pool = report["plan_pool"].as_object_mut().context("missing plan pool")?;
        pool.entry(selected_hash.clone()).or_insert(json!(selected_plan));
        pool.entry(exact_hash.clone()).or_insert(json!(exact_plan));
        slots.push(json!({"slot":slot,"selected":selected_hash,"exact":exact_hash}));
    }
    report["cases"].as_array_mut().context("missing policy cases")?.push(json!({
        "case_id":case_id,"phase":phase,"bucket":case.name,"snapshot":snapshot,"snapshot_rows":rows.len(),
        "eligible_count":eligible.len(),"eligible_ids":eligible.iter().map(|&i| &rows[i].id).collect::<Vec<_>>(),
        "eligible_ids_sha256":hash_ids(rows,&eligible),"selected_route":chosen,
        "selected_plan_sha256":slots[0]["selected"],"exact_plan_sha256":slots[0]["exact"],
        "slot_plan_hashes":slots,"plan_pass":plan_pass,
        "scope":{"tenant":case.bucket.tenant,"workspace":case.bucket.workspace,"version":case.bucket.version,"generation":case.bucket.generation,"source":case.bucket.source},
        "base_stride":case.bucket.stride,"base_cap":case.bucket.cap,"rotation":case.rotation,"case_stride":case.stride,"case_cap":case.cap
    }));
    println!("{}", json!({"phase":phase,"bucket":case.name,"route":chosen,"queries":8,"plan_pass":plan_pass}));
    Ok(())
}

pub async fn measure_phase(table: &Table, rows: &[Row], config: &Config, phase: &str, report: &mut Value) -> Result<()> {
    let version = table.version().await?;
    table.checkout(version).await?;
    for case in cases(phase != "indexed") { measure_case(table, rows, &case, config, phase, version, report).await?; }
    ensure!(table.version().await? == version, "policy snapshot changed");
    table.checkout_latest().await?;
    Ok(())
}

pub async fn reopened(table: &Table, rows: &[Row], config: &Config, report: &mut Value) -> Result<()> {
    let version = table.version().await?;
    table.checkout(version).await?;
    let case = cases(true).into_iter().find(|c| c.name == "fresh_unindexed").context("fresh policy case missing")?;
    measure_case(table, rows, &case, config, "reopened", version, report).await?;
    ensure!(table.version().await? == version, "reopened policy snapshot changed");
    Ok(())
}

pub fn initial_report(config: &Config) -> Result<Value> {
    let contract: Value = serde_json::from_str(CONTRACT)?;
    ensure!(serde_json::to_value(config)? == contract["config"], "policy configuration differs from frozen contract");
    Ok(json!({"schema":SCHEMA,"policy_id":POLICY_ID,"contract_sha256":hash_bytes(CONTRACT.as_bytes()),"status":"RUNNING","config":config,
        "lancedb":"0.23.1","application_baseline":"d0521c0935083e71890ab02c100920c2edaa7227","candidate_limit":100,
        "queries":[],"cases":[],"plan_pool":{},"call_counts":{"selected_native":0,"exact_native":0,"oracle_native":0,"explain_plans":0},
        "timing_protocol":"pair_then_oracle_audit_then_bucket_plans",
        "timing_boundary":"Independent eligibility, authority map/set, routing/filter/query build, native execution/decode/sort, final verification and temporary state cleanup; excludes input query generation, oracle, plan, independent audit and report serialization",
        "score_semantics":"1/(1+returned_L2); returned_L2 verified against squared Euclidean full-vector distance",
        "limits":contract["limits"],"training":"two fresh rebuilds; training seed is not controlled and statistical independence is not claimed"}))
}

pub fn passed(report: &Value) -> bool {
    report["queries"].as_array().is_some_and(|q| q.len()==576 && q.iter().all(|q| q["quality_pass"]==true))
        && report["cases"].as_array().is_some_and(|c| c.len()==72 && c.iter().all(|c| c["plan_pass"]==true))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frozen_routing_boundaries() {
        assert_eq!(route(0,10000,false),"empty");
        assert_eq!(route(4095,10000,false),"exact");
        assert_eq!(route(4096,10000,false),"ann");
        assert_eq!(route(4097,10000,false),"ann");
        assert_eq!(route(4096,10000,true),"exact");
        assert_eq!(route(4096,20000,false),"exact");
    }
    #[test]
    fn frozen_case_matrix_and_new_queries() {
        let config: Config = serde_json::from_value(serde_json::from_str::<Value>(CONTRACT).unwrap()["config"].clone()).unwrap();
        let rows: Vec<_> = (0..9984).map(|i| fixture::make_row(i,&config,false)).collect();
        assert_eq!(cases(false).len(),23); assert_eq!(cases(true).len(),24);
        for name in ["count_4095","count_4096","count_4097","rotated_4095","rotated_4096","rotated_4097"] {
            let case = cases(false).into_iter().find(|c| c.name==name).unwrap();
            assert_eq!(case.eligible(&rows).len(),name.rsplit('_').next().unwrap().parse::<usize>().unwrap());
        }
        let case = cases(false).into_iter().find(|c| c.name=="dense_current").unwrap();
        let ids=case.eligible(&rows);
        let hashes: HashSet<_>=(0..8).map(|slot| hash_vector(&embedding(&rows,&ids,slot))).collect();
        assert_eq!(hashes.len(),8);
        for old in 0..3 { assert!(!hashes.contains(&hash_vector(&fixture::query_vector(&rows,&ids,old,128)))); }
    }
}
