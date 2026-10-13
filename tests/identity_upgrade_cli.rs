#![cfg(feature = "local-storage")]
use origence::storage::native_turso as db;
use origence::storage::{Lifecycle, sqlite::SqliteStore};
use serde_json::Value;
use std::process::{Command, Output};

fn upgrade(dir: &std::path::Path, args: &[&str]) -> Output {
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
    let online = upgrade(dir.path(), &["memory-identity-upgrade"]);
    assert!(!online.status.success());
    assert!(String::from_utf8_lossy(&online.stderr).contains("requires --offline"));
    assert!(!path.exists());
    let missing = upgrade(dir.path(), &["--offline", "memory-identity-upgrade"]);
    assert!(!missing.status.success());
    assert!(!path.exists());
    let store = SqliteStore::open(&path).await.unwrap();
    assert!(
        !upgrade(dir.path(), &["--offline", "memory-identity-upgrade"])
            .status
            .success()
    );
    store.shutdown().await.unwrap();
    drop(store);
    // The historical command now only inspects the complete current format.
    // Neither flag can install missing schema or migrate an older database.
    for args in [
        vec!["--offline", "memory-identity-upgrade", "--dry-run"],
        vec!["--offline", "memory-identity-upgrade"],
        vec!["--offline", "memory-identity-upgrade"],
    ] {
        let output = upgrade(dir.path(), &args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["status"], "already_enabled");
    }
    let store = SqliteStore::open(&path).await.unwrap();
    db::query("DROP TABLE oc_memory_identities")
        .execute(store.pool())
        .await
        .unwrap();
    store.shutdown().await.unwrap();
    drop(store);
    for args in [
        vec!["--offline", "memory-identity-upgrade", "--dry-run"],
        vec!["--offline", "memory-identity-upgrade"],
    ] {
        assert!(!upgrade(dir.path(), &args).status.success());
    }
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
