//! Focused regression for the patched Lance local writer's shutdown boundary.
#![cfg(feature = "local-storage")]

use lance_io::{
    object_store::ObjectStore, object_writer::LocalWriter, traits::Writer,
    utils::tracking_store::IOTracker,
};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn failed_shutdown_never_publishes_or_allows_retry() {
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("failed.lance");
    let object_path =
        ObjectStore::extract_path_from_uri(Default::default(), final_path.to_str().unwrap())
            .unwrap();
    let temp_path = tempfile::NamedTempFile::new_in(dir.path())
        .unwrap()
        .into_temp_path();
    let staged_path = temp_path.to_path_buf();
    // The small write fits in LocalWriter's buffer. Its read-only backing
    // handle fails only when shutdown flushes that buffer to the filesystem.
    let file = tokio::fs::File::open(&temp_path).await.unwrap();
    let mut writer = LocalWriter::new(file, object_path, temp_path, Arc::new(IOTracker::default()));
    writer.write_all(b"buffered before failure").await.unwrap();
    assert!(staged_path.exists());
    assert!(!final_path.exists());

    assert!(Writer::shutdown(&mut writer).await.is_err());
    assert!(!final_path.exists());
    assert!(!staged_path.exists());

    // A consumed I/O error must not let a later shutdown publish partial data.
    assert!(Writer::shutdown(&mut writer).await.is_err());
    assert!(writer.write_all(b"retry must fail").await.is_err());
    assert!(!final_path.exists());
    assert!(!staged_path.exists());
}

#[tokio::test]
async fn successful_shutdown_publishes_exact_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("complete.lance");
    let object_path =
        ObjectStore::extract_path_from_uri(Default::default(), final_path.to_str().unwrap())
            .unwrap();
    let (file, temp_path) = tempfile::NamedTempFile::new_in(dir.path())
        .unwrap()
        .into_parts();
    let staged_path = temp_path.to_path_buf();
    let mut writer = LocalWriter::new(
        tokio::fs::File::from_std(file),
        object_path,
        temp_path,
        Arc::new(IOTracker::default()),
    );
    let data = b"complete local writer payload";
    writer.write_all(data).await.unwrap();
    assert!(staged_path.exists());
    assert!(!final_path.exists());

    let result = Writer::shutdown(&mut writer).await.unwrap();
    assert_eq!(result.size, data.len());
    assert_eq!(tokio::fs::read(&final_path).await.unwrap(), data);
    assert!(!staged_path.exists());
}
