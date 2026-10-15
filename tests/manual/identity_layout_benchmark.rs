//! Historical layout microbenchmark, never a production old-schema opening path.
//! Run: cargo test --locked --release --no-default-features --features manual-diagnostics --test identity-layout identity_layout_before_after -- --ignored --exact --nocapture
//! Single writer, 100 operations/transaction, native defaults. All rows are created;
//! reuse/lookup sample at most 1,000 evenly spaced keys at each dataset size.
//! Counts cover explicit workload SELECT/INSERT statements only, not BEGIN/COMMIT,
//! adapter PRAGMAs, constraints, or setup. Timings include commits, JSON decoding,
//! and progress logging after every ten committed batches;
//! input generation/key hashing is outside timing, including new-path canonical
//! stored-key revalidation. This is not per-request fsync latency. lookup_binding excludes the
//! common asset_view/version/source/publication path. One before/after sample per
//! size, fixed order, no cache reset or concurrency; timing noise/order bias apply.
//! No end-to-end/SLO claim and no migration or historical database support.
use origence::memory_identity::{MemoryIdentity, MemorySubject, SubjectKind};
use origence::storage::{Scope, native_turso as db};
use std::{collections::BTreeMap, path::Path, time::Instant};
use uuid::Uuid;

// Relevant schema frozen from main 4c0862e8; workspace ownership triggers and
// unrelated relations are omitted identically. All relevant FKs/indexes remain.
const WORKSPACES: &str = "CREATE TABLE oc_workspaces (
 tenant_id BLOB NOT NULL, id BLOB PRIMARY KEY, name TEXT NOT NULL,
 owner_workspace_id BLOB, created_at INTEGER NOT NULL,
 UNIQUE (tenant_id,id), CHECK (owner_workspace_id IS NULL OR owner_workspace_id != id),
 FOREIGN KEY (tenant_id,owner_workspace_id) REFERENCES oc_workspaces(tenant_id,id));";
const BEFORE: &str = "CREATE TABLE oc_assets (
 tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, id BLOB NOT NULL,
 kind TEXT NOT NULL CHECK (kind IN ('memory','knowledge')),
 title TEXT NOT NULL, fact_key TEXT, current_version INTEGER,
 deleted INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL,
 PRIMARY KEY (tenant_id,workspace_id,id),
 FOREIGN KEY (tenant_id,workspace_id) REFERENCES oc_workspaces(tenant_id,id));
 CREATE UNIQUE INDEX oc_memory_fact_slot ON oc_assets(tenant_id,workspace_id,fact_key)
 WHERE kind='memory';
 CREATE TABLE oc_memory_identities (
 tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL,
 identity_key TEXT NOT NULL, asset_id BLOB NOT NULL,
 identity_json TEXT NOT NULL, created_at INTEGER NOT NULL,
 PRIMARY KEY (tenant_id,workspace_id,identity_key),
 UNIQUE (tenant_id,workspace_id,asset_id),
 FOREIGN KEY (tenant_id,workspace_id,asset_id) REFERENCES oc_assets(tenant_id,workspace_id,id));";
const AFTER: &str = "CREATE TABLE oc_assets (
 tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, id BLOB NOT NULL,
 kind TEXT NOT NULL CHECK (kind IN ('memory','knowledge')),
 title TEXT NOT NULL, fact_key TEXT, current_version INTEGER,
 deleted INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL,
 identity_key TEXT, identity_json TEXT,
 CHECK ((identity_key IS NULL AND identity_json IS NULL) OR
        (kind='memory' AND identity_key IS NOT NULL AND identity_json IS NOT NULL)),
 PRIMARY KEY (tenant_id,workspace_id,id),
 FOREIGN KEY (tenant_id,workspace_id) REFERENCES oc_workspaces(tenant_id,id));
 CREATE UNIQUE INDEX oc_memory_fact_slot ON oc_assets(tenant_id,workspace_id,fact_key)
 WHERE kind='memory';
 CREATE UNIQUE INDEX oc_memory_identity_slot ON oc_assets(tenant_id,workspace_id,identity_key)
 WHERE identity_key IS NOT NULL;";

struct Input {
    asset: Uuid,
    identity: MemoryIdentity,
    key: String,
    json: String,
}
#[derive(Default)]
struct Counts {
    reads: usize,
    writes: usize,
}

async fn operation(
    tx: &mut db::Transaction,
    scope: Scope,
    input: &Input,
    merged: bool,
    phase: &str,
    counts: &mut Counts,
) {
    counts.reads += 1;
    let row = if merged {
        db::query("SELECT id,current_version,deleted,identity_json FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND identity_key=? AND kind='memory'")
    } else {
        db::query("SELECT asset_id FROM oc_memory_identities WHERE tenant_id=? AND workspace_id=? AND identity_key=?")
    }.bind(scope.tenant_id).bind(scope.workspace_id).bind(&input.key)
        .fetch_optional(&mut **tx).await.unwrap();
    if let Some(row) = row {
        assert_ne!(phase, "create");
        let asset: Uuid = row.get(if merged { "id" } else { "asset_id" });
        assert_eq!(asset, input.asset);
        let json: String = if merged {
            assert_eq!(row.get::<Option<i32>>("current_version"), None);
            assert!(!row.get::<bool>("deleted"));
            row.get("identity_json")
        } else {
            if phase == "reuse" {
                counts.reads += 1;
                let asset_row = db::query("SELECT id,current_version,deleted FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND id=? AND kind='memory'")
                    .bind(scope.tenant_id).bind(scope.workspace_id).bind(asset)
                    .fetch_one(&mut **tx).await.unwrap();
                assert_eq!(asset_row.get::<Uuid>("id"), asset);
                assert_eq!(asset_row.get::<Option<i32>>("current_version"), None);
                assert!(!asset_row.get::<bool>("deleted"));
            }
            counts.reads += 1;
            db::query_scalar("SELECT identity_json FROM oc_memory_identities WHERE tenant_id=? AND workspace_id=? AND asset_id=?")
                .bind(scope.tenant_id).bind(scope.workspace_id).bind(asset)
                .fetch_one(&mut **tx).await.unwrap()
        };
        assert_eq!(
            serde_json::from_str::<MemoryIdentity>(&json).unwrap(),
            input.identity
        );
        return;
    }
    assert_eq!(phase, "create");
    counts.reads += 1;
    let collision: bool = db::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND kind='memory' AND fact_key=?)")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(&input.key)
        .fetch_one(&mut **tx).await.unwrap();
    assert!(!collision);
    counts.writes += 1;
    let insert = if merged {
        db::query("INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,fact_key,created_at,identity_key,identity_json) VALUES (?,?,?,'memory',?,?,?,?,?)")
    } else {
        db::query("INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,fact_key,created_at) VALUES (?,?,?,'memory',?,?,?)")
    }.bind(scope.tenant_id).bind(scope.workspace_id).bind(input.asset)
        .bind(&input.identity.predicate).bind(&input.key).bind(1_i64);
    let insert = if merged {
        insert.bind(&input.key).bind(&input.json)
    } else {
        insert
    };
    insert.execute(&mut **tx).await.unwrap();
    if !merged {
        counts.writes += 1;
        db::query("INSERT INTO oc_memory_identities(tenant_id,workspace_id,identity_key,asset_id,identity_json,created_at) VALUES (?,?,?,?,?,?)")
            .bind(scope.tenant_id).bind(scope.workspace_id).bind(&input.key)
            .bind(input.asset).bind(&input.json).bind(1_i64)
            .execute(&mut **tx).await.unwrap();
    }
}

async fn bytes(path: &Path) -> u64 {
    match tokio::fs::metadata(path).await {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => panic!("file metadata: {error}"),
    }
}

#[tokio::test]
#[ignore = "manual native layout measurement; no performance thresholds"]
async fn identity_layout_before_after() {
    println!(
        "identity_layout config engine=turso-0.8.2 declared_rust=1.98.0 os={} arch={} debug_assertions={} native_defaults=true key_hashing=excluded canonical_revalidation=excluded",
        std::env::consts::OS,
        std::env::consts::ARCH,
        cfg!(debug_assertions)
    );
    // Record `rustc --version --verbose` alongside this command in CI; the actual
    // compiler version is not embedded by the crate and must not be guessed.
    let scope = Scope {
        tenant_id: Uuid::from_u128(1),
        workspace_id: Uuid::from_u128(2),
    };
    for n in [1_000, 10_000] {
        let inputs: Vec<_> = (0..n)
            .map(|i| {
                let identity = MemoryIdentity {
                    subject: MemorySubject {
                        kind: SubjectKind::Service,
                        stable_id: format!("service-{i:05}"),
                    },
                    predicate: "release.approval".into(),
                    context: BTreeMap::from([("environment".into(), "production".into())]),
                };
                Input {
                    asset: Uuid::from_u128(i as u128 + 100),
                    key: identity.key(scope).unwrap(),
                    json: serde_json::to_string(&identity).unwrap(),
                    identity,
                }
            })
            .collect();
        for merged in [false, true] {
            let layout = if merged { "after" } else { "before" };
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("identity.db");
            let pool = db::Pool::open(&path).await.unwrap();
            db::raw_sql(WORKSPACES).execute(&pool).await.unwrap();
            db::raw_sql(if merged { AFTER } else { BEFORE })
                .execute(&pool)
                .await
                .unwrap();
            db::query("INSERT INTO oc_workspaces(tenant_id,id,name,created_at) VALUES (?,?,'benchmark',1)")
                .bind(scope.tenant_id).bind(scope.workspace_id).execute(&pool).await.unwrap();
            for phase in ["create", "reuse", "lookup_binding"] {
                let stride = if phase == "create" {
                    1
                } else {
                    (n / 1_000).max(1)
                };
                let selected: Vec<_> = inputs.iter().step_by(stride).collect();
                let operations = selected.len();
                let transactions = operations.div_ceil(100);
                println!(
                    "identity_layout_progress layout={layout} n={n} phase={phase} start operations={operations} transactions={transactions} stride={stride}"
                );
                let mut counts = Counts::default();
                let started = Instant::now();
                for (index, batch) in selected.chunks(100).enumerate() {
                    let mut tx = pool.begin().await.unwrap();
                    for input in batch {
                        operation(&mut tx, scope, input, merged, phase, &mut counts).await;
                    }
                    tx.commit().await.unwrap();
                    if (index + 1) % 10 == 0 || index + 1 == transactions {
                        println!(
                            "identity_layout_progress layout={layout} n={n} phase={phase} committed_batches={} elapsed_us={}",
                            index + 1,
                            started.elapsed().as_micros()
                        );
                    }
                }
                let elapsed = started.elapsed();
                let (reads, writes) = match (merged, phase) {
                    (false, "create") => (2, 2),
                    (true, "create") => (2, 1),
                    (false, "reuse") => (3, 0),
                    (false, _) => (2, 0),
                    (true, _) => (1, 0),
                };
                assert_eq!(
                    (counts.reads, counts.writes),
                    (operations * reads, operations * writes)
                );
                println!(
                    "identity_layout layout={layout} n={n} phase={phase} operations={operations} stride={stride} batch=100 reads={} writes={} transactions={} elapsed_us={}",
                    counts.reads,
                    counts.writes,
                    transactions,
                    elapsed.as_micros()
                );
            }
            let count: i64 = db::query_scalar("SELECT COUNT(*) FROM oc_assets")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(count, n as i64);
            println!("identity_layout_progress layout={layout} n={n} phase=shutdown start");
            pool.shutdown().await.unwrap();
            drop(pool);
            let db_bytes = bytes(&path).await;
            let wal_bytes = bytes(&dir.path().join("identity.db-wal")).await;
            let shm_bytes = bytes(&dir.path().join("identity.db-shm")).await;
            println!(
                "identity_layout layout={layout} n={n} phase=after_shutdown db_bytes={db_bytes} wal_bytes={wal_bytes} shm_bytes={shm_bytes} db_wal_bytes={} total_bytes={}",
                db_bytes + wal_bytes,
                db_bytes + wal_bytes + shm_bytes
            );
        }
    }
}

// Exact production SQL, including paired CHECK operand order. Keep the original
// AFTER fixture above unchanged so earlier measurements remain reproducible.
const AFTER_PRODUCTION_CHECK: &str = "CREATE TABLE oc_assets (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, id BLOB NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('memory','knowledge')),
    title TEXT NOT NULL, fact_key TEXT, current_version INTEGER,
    deleted INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL,
    identity_key TEXT, identity_json TEXT,
    CHECK ((identity_key IS NULL AND identity_json IS NULL) OR
           (identity_key IS NOT NULL AND identity_json IS NOT NULL AND kind = 'memory')),
    PRIMARY KEY (tenant_id, workspace_id, id),
    FOREIGN KEY (tenant_id, workspace_id) REFERENCES oc_workspaces(tenant_id, id)
);
CREATE UNIQUE INDEX oc_memory_fact_slot ON oc_assets(tenant_id, workspace_id, fact_key)
WHERE kind = 'memory';
-- Tombstones retain their identity slot; deletion must not release this key.
CREATE UNIQUE INDEX oc_memory_identity_slot ON oc_assets(tenant_id, workspace_id, identity_key)
WHERE identity_key IS NOT NULL;";

// Diagnostic-only ablation: deliberately invalid as a production schema choice.
// Only the newly added paired identity CHECK differs from AFTER_PRODUCTION_CHECK;
// retain its columns, kind CHECK, primary key, both unique indexes, and workspace FK.
const AFTER_NO_IDENTITY_CHECK: &str = "CREATE TABLE oc_assets (
 tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, id BLOB NOT NULL,
 kind TEXT NOT NULL CHECK (kind IN ('memory','knowledge')),
 title TEXT NOT NULL, fact_key TEXT, current_version INTEGER,
 deleted INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL,
 identity_key TEXT, identity_json TEXT,
 PRIMARY KEY (tenant_id,workspace_id,id),
 FOREIGN KEY (tenant_id,workspace_id) REFERENCES oc_workspaces(tenant_id,id));
 CREATE UNIQUE INDEX oc_memory_fact_slot ON oc_assets(tenant_id,workspace_id,fact_key)
 WHERE kind='memory';
 CREATE UNIQUE INDEX oc_memory_identity_slot ON oc_assets(tenant_id,workspace_id,identity_key)
 WHERE identity_key IS NOT NULL;";

#[derive(Default)]
struct CreationTimes {
    identity_miss: std::time::Duration,
    fact_collision: std::time::Duration,
    assets_insert: std::time::Duration,
    identity_insert: std::time::Duration,
    begin: std::time::Duration,
    commit: std::time::Duration,
}

#[derive(Clone, Copy)]
enum IdentityProbe {
    Historical,
    Original,
    ExplicitNotNull,
    ForcedIdentityIndex,
}

fn identity_probe(merged: bool, probe: IdentityProbe) -> db::Query {
    if !merged {
        db::query(
            "SELECT asset_id FROM oc_memory_identities WHERE tenant_id=? AND workspace_id=? AND identity_key=?",
        )
    } else {
        match probe {
            IdentityProbe::Historical => db::query(
                "SELECT id,current_version,deleted,identity_json FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND identity_key=? AND kind='memory'",
            ),
            IdentityProbe::Original => db::query(
                "SELECT id,current_version,deleted,kind,identity_key,identity_json FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND identity_key=? AND kind='memory'",
            ),
            IdentityProbe::ExplicitNotNull => db::query(
                "SELECT id,current_version,deleted,kind,identity_key,identity_json FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND identity_key=? AND kind='memory' AND identity_key IS NOT NULL",
            ),
            IdentityProbe::ForcedIdentityIndex => db::query(
                "SELECT id,current_version,deleted,kind,identity_key,identity_json FROM oc_assets INDEXED BY oc_memory_identity_slot WHERE tenant_id=? AND workspace_id=? AND identity_key=? AND kind='memory' AND identity_key IS NOT NULL",
            ),
        }
    }
}

async fn timed_creation(
    tx: &mut db::Transaction,
    scope: Scope,
    input: &Input,
    merged: bool,
    probe: IdentityProbe,
    times: &mut CreationTimes,
) {
    let started = Instant::now();
    let row = identity_probe(merged, probe)
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(&input.key)
        .fetch_optional(&mut **tx)
        .await
        .unwrap();
    times.identity_miss += started.elapsed();
    assert!(row.is_none());
    let started = Instant::now();
    let collision: bool = db::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND kind='memory' AND fact_key=?)")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(&input.key)
        .fetch_one(&mut **tx).await.unwrap();
    times.fact_collision += started.elapsed();
    assert!(!collision);
    let started = Instant::now();
    let insert = if merged {
        db::query("INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,fact_key,created_at,identity_key,identity_json) VALUES (?,?,?,'memory',?,?,?,?,?)")
    } else {
        db::query("INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,fact_key,created_at) VALUES (?,?,?,'memory',?,?,?)")
    }.bind(scope.tenant_id).bind(scope.workspace_id).bind(input.asset)
        .bind(&input.identity.predicate).bind(&input.key).bind(1_i64);
    let insert = if merged {
        insert.bind(&input.key).bind(&input.json)
    } else {
        insert
    };
    insert.execute(&mut **tx).await.unwrap();
    times.assets_insert += started.elapsed();
    if !merged {
        let started = Instant::now();
        db::query("INSERT INTO oc_memory_identities(tenant_id,workspace_id,identity_key,asset_id,identity_json,created_at) VALUES (?,?,?,?,?,?)")
            .bind(scope.tenant_id).bind(scope.workspace_id).bind(&input.key)
            .bind(input.asset).bind(&input.json).bind(1_i64)
            .execute(&mut **tx).await.unwrap();
        times.identity_insert += started.elapsed();
    }
}

// Use a separate pool outside measurement. Unsupported EXPLAIN/decode/cleanup
// errors are explicit diagnostics, never substituted for successful plan output.
async fn creation_plans(
    path: &Path,
    scope: Scope,
    input: &Input,
    merged: bool,
    layout: &str,
    probe: IdentityProbe,
) {
    let pool = match db::Pool::open(path).await {
        Ok(pool) => pool,
        Err(error) => {
            println!("identity_creation_plan layout={layout} status=open_error error={error}");
            return;
        }
    };
    let identity = if merged {
        match probe {
            IdentityProbe::Historical => db::query("EXPLAIN QUERY PLAN SELECT id,current_version,deleted,identity_json FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND identity_key=? AND kind='memory'"),
            IdentityProbe::Original => db::query("EXPLAIN QUERY PLAN SELECT id,current_version,deleted,kind,identity_key,identity_json FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND identity_key=? AND kind='memory'"),
            IdentityProbe::ExplicitNotNull => db::query("EXPLAIN QUERY PLAN SELECT id,current_version,deleted,kind,identity_key,identity_json FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND identity_key=? AND kind='memory' AND identity_key IS NOT NULL"),
            IdentityProbe::ForcedIdentityIndex => db::query("EXPLAIN QUERY PLAN SELECT id,current_version,deleted,kind,identity_key,identity_json FROM oc_assets INDEXED BY oc_memory_identity_slot WHERE tenant_id=? AND workspace_id=? AND identity_key=? AND kind='memory' AND identity_key IS NOT NULL"),
        }
    } else {
        db::query("EXPLAIN QUERY PLAN SELECT asset_id FROM oc_memory_identities WHERE tenant_id=? AND workspace_id=? AND identity_key=?")
    }.bind(scope.tenant_id).bind(scope.workspace_id).bind(&input.key);
    let collision = db::query("EXPLAIN QUERY PLAN SELECT EXISTS(SELECT 1 FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND kind='memory' AND fact_key=?)")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(&input.key);
    let assets = if merged {
        db::query("EXPLAIN QUERY PLAN INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,fact_key,created_at,identity_key,identity_json) VALUES (?,?,?,'memory',?,?,?,?,?)")
    } else {
        db::query("EXPLAIN QUERY PLAN INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,fact_key,created_at) VALUES (?,?,?,'memory',?,?,?)")
    }.bind(scope.tenant_id).bind(scope.workspace_id).bind(input.asset)
        .bind(&input.identity.predicate).bind(&input.key).bind(1_i64);
    let assets = if merged {
        assets.bind(&input.key).bind(&input.json)
    } else {
        assets
    };
    let mut queries = vec![
        ("identity_miss", identity),
        ("fact_collision", collision),
        ("assets_insert", assets),
    ];
    if !merged {
        queries.push(("identity_insert", db::query("EXPLAIN QUERY PLAN INSERT INTO oc_memory_identities(tenant_id,workspace_id,identity_key,asset_id,identity_json,created_at) VALUES (?,?,?,?,?,?)")
            .bind(scope.tenant_id).bind(scope.workspace_id).bind(&input.key)
            .bind(input.asset).bind(&input.json).bind(1_i64)));
    }
    for (stage, query) in queries {
        match query.fetch_all(&pool).await {
            Ok(rows) => {
                println!(
                    "identity_creation_plan layout={layout} stage={stage} status=ok rows={}",
                    rows.len()
                );
                for row in rows {
                    match row.try_get::<String>(3_usize) {
                        Ok(detail) => println!(
                            "identity_creation_plan layout={layout} stage={stage} detail={detail:?}"
                        ),
                        Err(error) => println!(
                            "identity_creation_plan layout={layout} stage={stage} status=decode_error error={error}"
                        ),
                    }
                }
            }
            Err(error) => println!(
                "identity_creation_plan layout={layout} stage={stage} status=unsupported_or_error error={error}"
            ),
        }
    }
    if let Err(error) = pool.shutdown().await {
        println!("identity_creation_plan layout={layout} status=shutdown_error error={error}");
    }
}

/// Run separately from the original benchmark, in release mode, with --exact:
/// cargo test --locked --release --no-default-features --test local identity_layout_benchmark::identity_layout_creation_diagnostics -- --ignored --exact --nocapture
/// Creation only; unchanged 100-operation transactions. Three cyclic orders at
/// 1k, forward/reverse at 10k. The 10k order cannot fully balance three variants.
/// Stage clocks include query construction/binding/await, not input hashing or
/// JSON encoding. Total additionally includes assertions, clocks and progress.
/// Plans run once per variant after its first 1k sample, outside all timing.
/// Only after_production_check versus after_no_identity_check isolates one CHECK
/// in the execution path, not necessarily its expression-evaluation cost. Before
/// versus either after still combines row width, index and FK layout differences.
/// No performance assertions; no conclusion about production constraint removal.
#[tokio::test]
#[ignore = "manual repeated creation/CHECK ablation diagnosis; fixtures only"]
async fn identity_layout_creation_diagnostics() {
    run_creation_diagnostics(false).await;
}

/// Same creation workload and stage clocks, with identical production schema in
/// every variant. Explicit IS NOT NULL is redundant for identity_key=?; the
/// forced variant adds only INDEXED BY to that predicate. No deleted filter.
/// Unsupported hints fail explicitly during untimed empty-database preflight;
/// they never silently fall back. Plans are emitted before first timed samples.
/// Run with --release --test local identity_layout_benchmark::identity_layout_query_diagnostics -- --ignored --exact --nocapture.
#[tokio::test]
#[ignore = "manual query-plan diagnosis; no production query changes"]
async fn identity_layout_query_diagnostics() {
    run_creation_diagnostics(true).await;
}

async fn run_creation_diagnostics(query_variants: bool) {
    println!("identity_creation_suite query_variants={query_variants}");
    println!(
        "identity_creation config engine=turso-0.8.2 os={} arch={} debug_assertions={} batch=100 key_hashing=excluded canonical_revalidation=excluded check_ablation=fixture_only",
        std::env::consts::OS,
        std::env::consts::ARCH,
        cfg!(debug_assertions)
    );
    let scope = Scope {
        tenant_id: Uuid::from_u128(1),
        workspace_id: Uuid::from_u128(2),
    };
    for (n, rounds) in [(1_000, 3), (10_000, 2)] {
        let inputs: Vec<_> = (0..n)
            .map(|i| {
                let identity = MemoryIdentity {
                    subject: MemorySubject {
                        kind: SubjectKind::Service,
                        stable_id: format!("service-{i:05}"),
                    },
                    predicate: "release.approval".into(),
                    context: BTreeMap::from([("environment".into(), "production".into())]),
                };
                Input {
                    asset: Uuid::from_u128(i as u128 + 100),
                    key: identity.key(scope).unwrap(),
                    json: serde_json::to_string(&identity).unwrap(),
                    identity,
                }
            })
            .collect();
        for round in 0..rounds {
            let order = if n == 1_000 {
                match round {
                    0 => [0, 1, 2],
                    1 => [1, 2, 0],
                    _ => [2, 0, 1],
                }
            } else if round == 0 {
                [0, 1, 2]
            } else {
                [2, 1, 0]
            };
            for (position, variant) in order.into_iter().enumerate() {
                let (layout, probe) = if query_variants {
                    match variant {
                        0 => ("query_original", IdentityProbe::Original),
                        1 => ("query_explicit_not_null", IdentityProbe::ExplicitNotNull),
                        _ => (
                            "query_forced_identity_index",
                            IdentityProbe::ForcedIdentityIndex,
                        ),
                    }
                } else {
                    let layout = match variant {
                        0 => "before",
                        1 => "after_production_check",
                        _ => "after_no_identity_check",
                    };
                    (layout, IdentityProbe::Historical)
                };
                let merged = query_variants || variant != 0;
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("creation.db");
                let pool = db::Pool::open(&path).await.unwrap();
                db::raw_sql(WORKSPACES).execute(&pool).await.unwrap();
                db::raw_sql(if query_variants {
                    AFTER_PRODUCTION_CHECK
                } else if variant == 0 {
                    BEFORE
                } else if variant == 1 {
                    AFTER_PRODUCTION_CHECK
                } else {
                    AFTER_NO_IDENTITY_CHECK
                })
                .execute(&pool)
                .await
                .unwrap();
                db::query("INSERT INTO oc_workspaces(tenant_id,id,name,created_at) VALUES (?,?,'benchmark',1)")
                    .bind(scope.tenant_id).bind(scope.workspace_id).execute(&pool).await.unwrap();
                if query_variants {
                    // Fail here with the variant label if native syntax/index
                    // selection is unsupported, before recording any timing.
                    let preflight = identity_probe(true, probe)
                        .bind(scope.tenant_id)
                        .bind(scope.workspace_id)
                        .bind(&inputs[0].key)
                        .fetch_optional(&pool)
                        .await;
                    match preflight {
                        Ok(row) => assert!(row.is_none()),
                        Err(error) => panic!("query preflight layout={layout}: {error}"),
                    }
                    if n == 1_000 && round == 0 {
                        creation_plans(&path, scope, &inputs[0], true, layout, probe).await;
                    }
                }
                println!(
                    "identity_creation_progress layout={layout} n={n} round={round} position={position} phase=start"
                );
                let mut times = CreationTimes::default();
                let total = Instant::now();
                for (batch_index, batch) in inputs.chunks(100).enumerate() {
                    let started = Instant::now();
                    let mut tx = pool.begin().await.unwrap();
                    times.begin += started.elapsed();
                    for input in batch {
                        timed_creation(&mut tx, scope, input, merged, probe, &mut times).await;
                    }
                    let started = Instant::now();
                    tx.commit().await.unwrap();
                    times.commit += started.elapsed();
                    if (batch_index + 1) % 10 == 0 {
                        println!(
                            "identity_creation_progress layout={layout} n={n} round={round} committed_batches={} elapsed_us={}",
                            batch_index + 1,
                            total.elapsed().as_micros()
                        );
                    }
                }
                let total = total.elapsed();
                println!(
                    "identity_creation layout={layout} n={n} round={round} position={position} identity_miss_ops={n} fact_collision_ops={n} assets_insert_ops={n} identity_insert_ops={} reads={} writes={} transactions={} total_us={} identity_miss_us={} fact_collision_us={} assets_insert_us={} identity_insert_us={} begin_us={} commit_us={}",
                    if merged { 0 } else { n },
                    n * 2,
                    n * if merged { 1 } else { 2 },
                    n / 100,
                    total.as_micros(),
                    times.identity_miss.as_micros(),
                    times.fact_collision.as_micros(),
                    times.assets_insert.as_micros(),
                    times.identity_insert.as_micros(),
                    times.begin.as_micros(),
                    times.commit.as_micros()
                );
                let count: i64 = db::query_scalar("SELECT COUNT(*) FROM oc_assets")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
                assert_eq!(count, n as i64);
                if query_variants {
                    verify_query_results(&pool, scope, &inputs[0], probe).await;
                }
                if !merged {
                    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM oc_memory_identities")
                        .fetch_one(&pool)
                        .await
                        .unwrap();
                    assert_eq!(count, n as i64);
                }
                println!(
                    "identity_creation_progress layout={layout} n={n} round={round} phase=shutdown"
                );
                pool.shutdown().await.unwrap();
                drop(pool);
                let db_bytes = bytes(&path).await;
                let wal_bytes = bytes(&dir.path().join("creation.db-wal")).await;
                println!(
                    "identity_creation_size layout={layout} n={n} round={round} db_bytes={db_bytes} wal_bytes={wal_bytes}"
                );
                if round == 0
                    && ((!query_variants && n == 1_000) || (query_variants && n == 10_000))
                {
                    println!("identity_creation_plan_context layout={layout} populated_rows={n}");
                    creation_plans(&path, scope, &inputs[0], merged, layout, probe).await;
                }
            }
        }
    }
}

// Outside all measurements. Each variant must retain the exact row, complete
// identity and tombstone; missing keys and a different scope return no row.
async fn verify_query_results(pool: &db::Pool, scope: Scope, input: &Input, probe: IdentityProbe) {
    for deleted in [false, true] {
        if deleted {
            db::query(
                "UPDATE oc_assets SET deleted=1 WHERE tenant_id=? AND workspace_id=? AND id=?",
            )
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(input.asset)
            .execute(pool)
            .await
            .unwrap();
        }
        let row = identity_probe(true, probe)
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(&input.key)
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(row.get::<Uuid>("id"), input.asset);
        assert_eq!(row.get::<bool>("deleted"), deleted);
        assert_eq!(row.get::<Option<i32>>("current_version"), None);
        assert_eq!(row.get::<String>("identity_json"), input.json);
        assert_eq!(row.get::<String>("identity_key"), input.key);
        assert_eq!(row.get::<String>("kind"), "memory");
    }
    for (tenant, workspace, key) in [
        (
            scope.tenant_id,
            scope.workspace_id,
            format!("{}-absent", input.key),
        ),
        (scope.tenant_id, Uuid::from_u128(999), input.key.clone()),
        (Uuid::from_u128(999), scope.workspace_id, input.key.clone()),
    ] {
        let row = identity_probe(true, probe)
            .bind(tenant)
            .bind(workspace)
            .bind(key)
            .fetch_optional(pool)
            .await
            .unwrap();
        assert!(row.is_none());
    }
    // Restore logical contents; diagnostic size includes these two extra writes.
    db::query("UPDATE oc_assets SET deleted=0 WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(input.asset)
        .execute(pool)
        .await
        .unwrap();
}
