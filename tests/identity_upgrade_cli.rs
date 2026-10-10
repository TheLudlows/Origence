#![cfg(feature = "local-storage")]
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
    sqlx::query("DROP TABLE oc_memory_identities")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(
        !upgrade(dir.path(), &["--offline", "memory-identity-upgrade"])
            .status
            .success()
    );
    store.shutdown().await.unwrap();
    drop(store);
    for (args, status) in [
        (
            vec!["--offline", "memory-identity-upgrade", "--dry-run"],
            "required",
        ),
        (vec!["--offline", "memory-identity-upgrade"], "enabled"),
        (
            vec!["--offline", "memory-identity-upgrade"],
            "already_enabled",
        ),
    ] {
        let output = upgrade(dir.path(), &args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["status"], status);
    }
    assert!(!dir.path().join("vectors").exists());
    assert!(!dir.path().join("graph.db").exists());
    assert!(!dir.path().join("blobs").exists());
}
