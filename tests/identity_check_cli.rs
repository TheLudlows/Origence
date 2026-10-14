#![cfg(feature = "local-storage")]
use origence::storage::native_turso as db;
use origence::storage::{Lifecycle, sqlite::SqliteStore};
use serde_json::Value;
use std::process::{Command, Output};

fn check(dir: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_origence"))
        .arg("--data-dir")
        .arg(dir)
        .args(args)
        .env_remove("OC_API_KEY")
        .output()
        .unwrap()
}

#[tokio::test]
async fn cli_requires_offline_and_does_not_initialize_native_engines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("context.db");
    let online = check(dir.path(), &["identity-check"]);
    assert!(!online.status.success());
    assert!(String::from_utf8_lossy(&online.stderr).contains("requires --offline"));
    assert!(!path.exists());
    let missing = check(dir.path(), &["--offline", "identity-check"]);
    assert!(!missing.status.success());
    assert!(!path.exists());
    let store = SqliteStore::open(&path).await.unwrap();
    assert!(
        !check(dir.path(), &["--offline", "identity-check"])
            .status
            .success()
    );
    store.shutdown().await.unwrap();
    drop(store);
    let bytes = std::fs::read(&path).unwrap();
    for _ in 0..2 {
        let output = check(dir.path(), &["--offline", "identity-check"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            report,
            serde_json::json!({"feature":"memory-identity-v1","valid":true})
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    let store = SqliteStore::open(&path).await.unwrap();
    db::query("DROP TABLE oc_memory_identities")
        .execute(store.pool())
        .await
        .unwrap();
    store.shutdown().await.unwrap();
    drop(store);
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        !check(dir.path(), &["--offline", "identity-check"])
            .status
            .success()
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let raw = db::Pool::open_read_only(&path).await.unwrap();
    let count: i64 =
        db::query_scalar("SELECT count(*) FROM sqlite_master WHERE name='oc_memory_identities'")
            .fetch_one(&raw)
            .await
            .unwrap();
    assert_eq!(count, 0);
    raw.close().await;
    assert!(!dir.path().join("vectors").exists());
    assert!(!dir.path().join("graph.db").exists());
    assert!(!dir.path().join("blobs").exists());
}

#[test]
fn removed_upgrade_command_and_dry_run_flag_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    for args in [
        vec!["--offline", "memory-identity-upgrade"],
        vec!["--offline", "identity-check", "--dry-run"],
    ] {
        let output = check(dir.path(), &args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!dir.path().join("context.db").exists());
    }
}
