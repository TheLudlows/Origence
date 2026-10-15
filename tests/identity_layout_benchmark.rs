//! Historical layout microbenchmark, never a production old-schema opening path.
//! Run: cargo test --locked --release --no-default-features --test local identity_layout_benchmark::identity_layout_before_after -- --ignored --exact --nocapture
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
