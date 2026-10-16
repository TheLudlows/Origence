//! Controlled Linux native-write interruption. No production fault hooks.
//! Prepared incomplete indexes exercise explicit adapter retry, not a new
//! automatic repair API or first-publication/job recovery guarantee.
#![cfg(all(feature = "local-storage", target_os = "linux"))]

use super::maintenance_fixture::{self as fixture, Fixture, versions};
use origence::storage::{
    LanceDbStore, Lifecycle, SqliteStore, StorageError, VectorStore, maintenance::compact_vectors,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    os::unix::process::ExitStatusExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const ANN: &str = "vector_l2_ivf64_probe32_v1";
const FTS: &str = "chunk_terms_v1";

struct KillOnDrop(Child);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn copy_tree(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let destination = target.join(entry.file_name());
        assert!(!entry.file_type().unwrap().is_symlink());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), &destination).unwrap();
            assert_eq!(
                fs::read(entry.path()).unwrap(),
                fs::read(destination).unwrap()
            );
        }
    }
}

async fn index_metadata(root: &Path) -> serde_json::Value {
    let db = lancedb::connect(root.join("vectors").to_str().unwrap())
        .execute()
        .await
        .unwrap();
    let mut result = BTreeMap::new();
    for name in db.table_names().execute().await.unwrap() {
        let table = db.open_table(&name).execute().await.unwrap();
        let mut indices: Vec<_> = table
            .list_indices()
            .await
            .unwrap()
            .into_iter()
            .map(|index| format!("{index:?}"))
            .collect();
        indices.sort();
        result.insert(name, indices);
    }
    json!(result)
}

async fn prepare(root: &Path, scenario: &str, value: &Fixture) -> String {
    let mut names: Vec<_> = value
        .profiles
        .iter()
        .map(|p| fixture::table_name(p))
        .collect();
    names.sort();
    if scenario == "compact" {
        return names[1].clone();
    }
    let name = if scenario.starts_with("fts") {
        FTS.into()
    } else {
        names[0].clone()
    };
    let db = lancedb::connect(root.join("vectors").to_str().unwrap())
        .execute()
        .await
        .unwrap();
    let table = db.open_table(&name).execute().await.unwrap();
    let index = match scenario {
        "vector_scalar" | "fts_scalar" => "filter_generation_v1",
        "ann" => ANN,
        "fts" => "chunk_terms_whitespace_v1",
        _ => panic!("unknown scenario"),
    };
    assert!(
        table
            .list_indices()
            .await
            .unwrap()
            .iter()
            .any(|i| i.name == index)
    );
    // Test preparation only: preserve an incomplete set, including earlier
    // scalar indexes, so the existing ensure_index path actually executes.
    table.drop_index(index).await.unwrap();
    name
}

async fn operation(root: &Path, scenario: &str, value: &Fixture) -> Result<(), StorageError> {
    if scenario == "compact" {
        return compact_vectors(root, false).await.map(|_| ());
    }
    let authority = SqliteStore::open(root.join("context.db")).await?;
    let store = LanceDbStore::open(root.join("vectors")).await?;
    let result = if scenario.starts_with("fts") {
        store
            .upsert_full_text(value.scope, value.full_text.clone())
            .await
    } else {
        let profile = value
            .profiles
            .iter()
            .min_by_key(|p| fixture::table_name(p))
            .unwrap();
        store
            .upsert(
                value.scope,
                value
                    .vectors
                    .iter()
                    .filter(|entry| &entry.profile == profile)
                    .cloned()
                    .collect(),
            )
            .await
    };
    store.shutdown().await?;
    authority.shutdown().await?;
    result
}

/// A subprocess is intentional: hard death must discard the native runtime,
/// release the OS lock, and bypass Rust destructors. This is not application CLI.
#[tokio::test]
async fn native_io_child() {
    let Some(root) = std::env::var_os("OC_MAINTENANCE_CHILD_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let scenario = std::env::var("OC_MAINTENANCE_SCENARIO").unwrap();
    let value: Fixture =
        serde_json::from_slice(&fs::read(root.join("fixture.json")).unwrap()).unwrap();
    let result = operation(&root, &scenario, &value).await;
    let expected_error = std::env::var("OC_MAINTENANCE_FAULT_MODE").unwrap() == "enospc";
    fs::write(root.join("operation-result.json"), json!({
        "returned_success": result.is_ok(), "error": result.as_ref().err().map(ToString::to_string)
    }).to_string()).unwrap();
    assert!(
        expected_error && result.is_err(),
        "fault must reach the native operation and propagate"
    );
}

async fn interrupted(
    root: &Path,
    scenario: &str,
    match_path: &str,
    mode: &str,
    after: usize,
    shim: &Path,
    evidence: &Path,
) -> std::process::ExitStatus {
    fs::write(
        root.join(".maintenance-io-fault-ok"),
        b"synthetic-test-only",
    )
    .unwrap();
    let marker = evidence.join("fault.txt");
    assert!(!marker.exists(), "fault marker must be new for this child");
    let output = fs::File::create(evidence.join("child.log")).unwrap();
    let mut child = KillOnDrop(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "maintenance_recovery::native_io_child",
                "--nocapture",
            ])
            .env("OC_MAINTENANCE_CHILD_ROOT", root)
            .env("OC_MAINTENANCE_SCENARIO", scenario)
            .env("LD_PRELOAD", shim)
            .env("OC_MAINTENANCE_FAULT_ROOT", root)
            .env("OC_MAINTENANCE_FAULT_MATCH", match_path)
            .env("OC_MAINTENANCE_FAULT_MODE", mode)
            .env("OC_MAINTENANCE_FAULT_AFTER", after.to_string())
            .env("OC_MAINTENANCE_FAULT_MARKER", &marker)
            .stdout(Stdio::from(output.try_clone().unwrap()))
            .stderr(Stdio::from(output))
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut stopped_and_killed = false;
    let status = loop {
        if mode == "pause" && marker.exists() {
            let state = fs::read_to_string(format!("/proc/{}/status", child.0.id())).unwrap();
            if state
                .lines()
                .any(|line| line.starts_with("State:") && line.contains('T'))
            {
                fs::write(evidence.join("stopped-process.txt"), state).unwrap();
                assert!(
                    matches!(
                        SqliteStore::open(root.join("context.db")).await,
                        Err(StorageError::Conflict(_))
                    ),
                    "native child must retain exclusive authority lock"
                );
                child.0.kill().unwrap();
                stopped_and_killed = true;
                break child.0.wait().unwrap();
            }
        }
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "native syscall target was not reached in time"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert!(
        marker.exists(),
        "no positive-length native-write evidence; this is NOT a covered interruption"
    );
    let marker_text = fs::read_to_string(&marker).unwrap();
    let fields: BTreeMap<_, _> = marker_text
        .lines()
        .map(|line| line.split_once('=').unwrap())
        .collect();
    assert_eq!(fields["mode"], mode);
    assert_eq!(fields["pid"], child.0.id().to_string());
    assert!(fields["requested_bytes"].parse::<usize>().unwrap() > 0);
    assert_eq!(fields["successful_matching_writes"], after.to_string());
    assert!(fields["relative_path"].contains(match_path));
    assert_eq!(fields["errno"], if mode == "enospc" { "28" } else { "0" });
    if mode == "pause" {
        assert!(
            stopped_and_killed,
            "marker alone cannot establish stopped-process interruption"
        );
        assert_eq!(
            status.signal(),
            Some(9),
            "controller must terminate the stopped child with SIGKILL"
        );
    }
    fs::write(evidence.join("exit.txt"), format!("{status:?}\n")).unwrap();
    if root.join("operation-result.json").exists() {
        fs::copy(
            root.join("operation-result.json"),
            evidence.join("operation-result.json"),
        )
        .unwrap();
    }
    // Both normal error return and hard death must release the exclusive lock.
    let reopened = SqliteStore::open(root.join("context.db")).await.unwrap();
    reopened.shutdown().await.unwrap();
    status
}

#[tokio::test]
#[ignore = "explicit Linux syscall-injection acceptance; requires compiled test shim"]
async fn native_io_failure_matrix() {
    let shim =
        fs::canonicalize(std::env::var_os("OC_MAINTENANCE_SHIM").expect("shim required")).unwrap();
    let evidence = PathBuf::from(
        std::env::var_os("OC_MAINTENANCE_EVIDENCE").expect("evidence directory required"),
    );
    fs::create_dir_all(&evidence).unwrap();
    // Keep on failure/abort for diagnosis; delete only after all assertions pass.
    let temp = tempfile::Builder::new()
        .prefix("synthetic-fixtures-")
        .tempdir_in(&evidence)
        .unwrap()
        .keep();
    let baseline = temp.join("baseline");
    let value = tokio::time::timeout(Duration::from_secs(600), fixture::seed(&baseline))
        .await
        .expect("fixture seed/check exceeded 600 seconds; this is a harness timeout");
    let initial_indices = index_metadata(&baseline).await;
    fs::write(
        baseline.join("fixture.json"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    // Boundaries are deliberately separate: first output write, a later output
    // write, and manifest-file write. None establishes atomic rename/fsync or
    // arbitrary power-loss safety. No timed kill is accepted as evidence.
    let mut cases = vec![];
    for scenario in ["vector_scalar", "fts_scalar", "ann", "fts", "compact"] {
        cases.push((
            scenario,
            "output-first",
            if scenario == "compact" {
                "data"
            } else {
                "_indices"
            },
            0,
        ));
    }
    cases.push(("compact", "output-later", "data", 1));
    cases.push(("compact", "manifest-write", "_versions", 0));
    for (scenario, point, suffix, after_writes) in cases {
        for mode in ["pause", "enospc"] {
            let label = format!("{scenario}-{mode}-{point}");
            eprintln!("maintenance case start {label}");
            let root = temp.join(&label);
            copy_tree(&baseline, &root);
            let table = prepare(&root, scenario, &value).await;
            let before = versions(&root).await;
            let directory = evidence.join(&label);
            fs::create_dir(&directory).expect("case evidence directory must be fresh");
            fs::write(
                directory.join("before.json"),
                serde_json::to_vec(&before).unwrap(),
            )
            .unwrap();
            fs::write(
                directory.join("indexes-before-prepare.json"),
                initial_indices.to_string(),
            )
            .unwrap();
            fs::write(
                directory.join("indexes-prepared.json"),
                index_metadata(&root).await.to_string(),
            )
            .unwrap();
            let layout_before = compact_vectors(&root, true).await.unwrap();
            let status = interrupted(
                &root,
                scenario,
                &format!("{table}.lance/{suffix}/"),
                mode,
                after_writes,
                &shim,
                &directory,
            )
            .await;
            eprintln!("maintenance case {label}: fault observed; pre-retry oracles start");
            fixture::assert_authority(&root, &value).await;
            fixture::assert_native(&root, &value).await;
            // Preserve the stronger data oracle even if the child rejects an
            // unexpected successful API return after a real native write error.
            assert_eq!(status.success(), mode == "enospc");
            if scenario.starts_with("fts") {
                let native = LanceDbStore::open(root.join("vectors")).await.unwrap();
                assert!(
                    native.check_full_text_available().await.is_err(),
                    "incomplete required FTS indexes must fail closed before explicit retry"
                );
                native.shutdown().await.unwrap();
            }
            let after = versions(&root).await;
            fs::write(
                directory.join("indexes-after-fault.json"),
                index_metadata(&root).await.to_string(),
            )
            .unwrap();
            let layout_after = compact_vectors(&root, true).await.unwrap();
            fs::write(
                directory.join("layouts.json"),
                json!({"before":layout_before,"after_fault":layout_after}).to_string(),
            )
            .unwrap();
            if scenario == "compact" {
                let first = before.keys().find(|name| name.starts_with("vec_")).unwrap();
                assert!(
                    after[first] > before[first],
                    "first table must commit before second-table fault"
                );
                assert_eq!(
                    layout_after.tables[0].before.rows,
                    layout_before.tables[0].before.rows
                );
                assert!(
                    layout_after.tables[0].before.fragments
                        < layout_before.tables[0].before.fragments,
                    "first table must actually compact, not merely advance a version"
                );
                assert_eq!(
                    after[&table], before[&table],
                    "blocked native output/manifest write must precede snapshot publication"
                );
                assert_eq!(after[FTS], before[FTS], "Compact must not touch FTS");
            }
            fs::write(
                directory.join("after-fault.json"),
                serde_json::to_vec(&after).unwrap(),
            )
            .unwrap();
            // Preserve the stopped interrupted snapshot before any repair/retry.
            let restored = temp.join(format!("{label}-restored"));
            copy_tree(&root, &restored);
            fixture::assert_authority(&restored, &value).await;
            fixture::assert_native(&restored, &value).await;
            for directory_root in [&root, &restored] {
                for retry in 1..=2 {
                    let progress = format!("root={} retry={retry}", directory_root.display());
                    fs::write(directory.join("progress.txt"), &progress).unwrap();
                    eprintln!("maintenance case {label}: {progress} operation start");
                    tokio::time::timeout(
                        Duration::from_secs(120),
                        operation(directory_root, scenario, &value),
                    )
                    .await
                    .expect("maintenance retry exceeded 120 seconds")
                    .unwrap();
                    eprintln!("maintenance case {label}: {progress} oracle start");
                    tokio::time::timeout(
                        Duration::from_secs(120),
                        fixture::check(directory_root, &value),
                    )
                    .await
                    .expect("recovery oracle exceeded 120 seconds");
                    eprintln!("maintenance case {label}: {progress} complete");
                }
            }
            fs::write(directory.join("result.json"), json!({"status":"PASS", "scenario":scenario,
                "mode":mode, "point":point, "successful_matching_writes_before_fault":after_writes, "retries":2,
                "restored_interrupted_directory":true}).to_string()).unwrap();
            eprintln!("maintenance case PASS {label}");
            fs::remove_dir_all(&root).unwrap();
            fs::remove_dir_all(&restored).unwrap();
        }
    }
    fs::remove_dir_all(temp).unwrap();
}
