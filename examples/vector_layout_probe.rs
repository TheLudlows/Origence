//! Synthetic layout experiment, restricted to its own temporary directory.
//! Calls the public store and LanceDB APIs; never opens an application data path.
use lancedb::table::{CompactionOptions, OptimizeAction};
use origence::storage::{
    LanceDbStore, Scope, SourceVersion, VectorEntry, VectorQuery, VectorStore,
};
use serde_json::{Value, json};
use std::{fs::OpenOptions, io::Write, time::Instant};
use uuid::Uuid;

const DIM: usize = 1024;
const PROFILE: &str = "layout-probe-dim1024-v1";

fn entry(i: u128) -> VectorEntry {
    let mut embedding = vec![0.0; DIM];
    embedding[0] = i as f32 / 1000.0;
    VectorEntry {
        id: Uuid::from_u128(i),
        embedding,
        profile: PROFILE.into(),
        dimension: DIM,
        generation: 1,
        source: SourceVersion {
            source_id: Uuid::from_u128(100_000 + i),
            version: 1,
        },
    }
}

async fn measure(
    store: &LanceDbStore,
    scope: Scope,
    ids: &[Uuid],
    phase: &str,
    expected: &mut Vec<Value>,
    rows: &mut Vec<Value>,
) -> anyhow::Result<()> {
    for n in 0..8 {
        let limit = if n % 2 == 0 { 5 } else { 100 };
        let mut embedding = vec![0.0; DIM];
        embedding[0] = -0.123 - (n as f32) / 10.0;
        let started = Instant::now();
        let mut hits = Vec::new();
        for batch in ids.chunks(512) {
            hits.extend(
                store
                    .search(VectorQuery {
                        scope,
                        artifact_ids: Some(batch.to_vec()),
                        profile: PROFILE.into(),
                        dimension: DIM,
                        generation: 1,
                        embedding: embedding.clone(),
                        limit: 100,
                    })
                    .await?,
            );
        }
        hits.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
        hits.truncate(limit);
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        anyhow::ensure!(hits.len() == limit, "hit_count_mismatch");
        anyhow::ensure!(
            hits.iter().enumerate().all(|(i, h)| h.id == ids[i]),
            "scope_or_rank_mismatch"
        );
        let output = serde_json::to_value(&hits)?;
        if phase == "fragmented" {
            expected.push(output);
        } else {
            anyhow::ensure!(
                output == expected[n],
                "changed_hit_id_source_score_or_order"
            );
        }
        rows.push(json!({"phase":phase,"iteration":n,"top_k":limit,"native_ms":elapsed}));
    }
    Ok(())
}

async fn probe(report: &mut Value) -> anyhow::Result<()> {
    let parent = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/vector-profile");
    std::fs::create_dir_all(&parent)?;
    let directory = tempfile::Builder::new()
        .prefix("layout-")
        .tempdir_in(&parent)?;
    let location = directory.path().to_owned();
    let store = LanceDbStore::open(&location).await?;
    let scope = Scope {
        tenant_id: Uuid::from_u128(1),
        workspace_id: Uuid::from_u128(2),
    };
    let entries: Vec<_> = (1..=563).map(entry).collect();
    let ids: Vec<_> = entries.iter().map(|e| e.id).collect();
    for batch in entries.chunks(12) {
        store.upsert(scope, batch.to_vec()).await?;
    }
    for i in 0..512 {
        let other = Scope {
            workspace_id: Uuid::from_u128(10 + i % 4),
            ..scope
        };
        // Foreign entries are closer than every allowed entry; prefilter must exclude them.
        let mut value = entry(1000 + i);
        value.embedding[0] = -0.123;
        store.upsert(other, vec![value]).await?;
        if i % 128 == 127 {
            println!("{}", json!({"background_added":i+1}));
        }
    }
    let db = lancedb::connect(
        location
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("invalid_temp_path"))?,
    )
    .read_consistency_interval(std::time::Duration::ZERO)
    .execute()
    .await?;
    let names = db.table_names().execute().await?;
    anyhow::ensure!(names.len() == 1, "unexpected_table_count");
    let table = db.open_table(&names[0]).execute().await?;
    let before = table.stats().await?;
    report["before"] = json!({"rows":before.num_rows,"fragments":before.fragment_stats.num_fragments,
                             "small_fragments":before.fragment_stats.num_small_fragments,"bytes":before.total_bytes});
    let mut expected = Vec::new();
    let mut rows = Vec::new();
    measure(&store, scope, &ids, "fragmented", &mut expected, &mut rows).await?;
    let started = Instant::now();
    // Compact only: never OptimizeAction::All or Prune (historical files stay intact).
    table
        .optimize(OptimizeAction::Compact {
            options: CompactionOptions {
                num_threads: Some(2),
                target_rows_per_fragment: 4096,
                ..Default::default()
            },
            remap_options: None,
        })
        .await?;
    report["compact_ms"] = json!(started.elapsed().as_secs_f64() * 1000.0);
    let after = table.stats().await?;
    anyhow::ensure!(after.num_rows == before.num_rows, "row_count_changed");
    report["after"] = json!({"rows":after.num_rows,"fragments":after.fragment_stats.num_fragments,
                            "small_fragments":after.fragment_stats.num_small_fragments,"bytes":after.total_bytes});
    measure(&store, scope, &ids, "compacted", &mut expected, &mut rows).await?;
    let reopened = LanceDbStore::open(&location).await?;
    measure(&reopened, scope, &ids, "reopened", &mut expected, &mut rows).await?;
    report["searches"] = json!(rows);
    report["exact_id_source_score_order_match"] = json!(true);
    drop(reopened);
    drop(table);
    drop(db);
    drop(store);
    directory.close()?;
    report["temporary_tree_removed"] = json!(!location.exists());
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("report_path_required"))?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let mut report = json!({"schema":"vector-layout-probe-v1","dimension":DIM,
        "foreground_rows":563,"foreground_batch_size":12,"background_batches":512,"background_workspaces":4,
        "limitations":["synthetic vectors, not embedding quality or HTTP latency","debug build; no production SLO",
                       "compaction only in isolated fixture; no production maintenance API","first queries retained; no concurrent writes"]});
    match probe(&mut report).await {
        Ok(()) => report["status"] = json!("completed"),
        Err(_) => report["status"] = json!("probe_failed"),
    }
    serde_json::to_writer_pretty(&mut file, &report)?;
    writeln!(file)?;
    println!("{}", json!({"status":report["status"]}));
    anyhow::ensure!(report["status"] == "completed", "probe_failed");
    Ok(())
}
