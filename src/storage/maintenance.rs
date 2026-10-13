//! Explicit offline physical compaction. No scheduler, model, worker or pruning.
use super::{StorageError, StorageResult, lancedb::schema_for, sqlite::lock_for_maintenance};
use arrow_schema::DataType;
use lancedb::table::{CompactionOptions, OptimizeAction};
use serde::Serialize;
use std::{
    path::Path,
    time::{Duration, Instant},
};

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct TableLayout {
    pub version: u64,
    pub rows: usize,
    pub fragments: usize,
    pub bytes: usize,
}
#[derive(Debug, Serialize)]
pub struct TableCompaction {
    pub table_index: usize,
    pub before: TableLayout,
    pub after: Option<TableLayout>,
}
#[derive(Debug, Serialize)]
pub struct CompactionReport {
    pub dry_run: bool,
    pub target_rows_per_fragment: usize,
    pub concurrency: usize,
    pub elapsed_ms: f64,
    pub historical_versions_retained: bool,
    pub tables: Vec<TableCompaction>,
}

async fn layout(table: &lancedb::Table) -> StorageResult<TableLayout> {
    let stats = table
        .stats()
        .await
        .map_err(|_| StorageError::Unavailable("vector statistics unavailable".into()))?;
    let version = table
        .version()
        .await
        .map_err(|_| StorageError::Unavailable("vector version unavailable".into()))?;
    Ok(TableLayout {
        version,
        rows: stats.num_rows,
        fragments: stats.fragment_stats.num_fragments,
        bytes: stats.total_bytes,
    })
}

/// Requires a stopped host and OS access to its existing data directory.
/// Validates every table before changing any. Each table commits independently;
/// on failure earlier tables may already be compacted. Retry is explicit.
/// No hard deadline or memory cap: two compaction tasks, 4096 target rows/file.
/// Only physical files change; historical versions/files are not pruned.
pub async fn compact_vectors(data_dir: &Path, dry_run: bool) -> StorageResult<CompactionReport> {
    compact_inner(data_dir, dry_run)
        .await
        .map_err(|error| match error {
            StorageError::NotFound => StorageError::NotFound,
            StorageError::Io(ref e) if e.kind() == std::io::ErrorKind::NotFound => {
                StorageError::NotFound
            }
            StorageError::Conflict(_) => StorageError::Conflict(
                "offline vector maintenance requires exclusive access to a stopped host".into(),
            ),
            StorageError::InvalidArgument(_) => {
                StorageError::InvalidArgument("incompatible vector maintenance input")
            }
            _ => StorageError::Unavailable(
                "vector maintenance failed; earlier tables may have completed".into(),
            ),
        })
}

async fn compact_inner(data_dir: &Path, dry_run: bool) -> StorageResult<CompactionReport> {
    let started = Instant::now();
    let data_dir = tokio::fs::canonicalize(data_dir).await?;
    let _guard = lock_for_maintenance(&data_dir.join("context.db")).await?;
    let vectors = data_dir.join("vectors");
    if !tokio::fs::metadata(&vectors).await?.is_dir() {
        return Err(StorageError::NotFound);
    }
    let uri = vectors
        .to_str()
        .ok_or(StorageError::InvalidArgument("vector path must be UTF-8"))?;
    let db = lancedb::connect(uri)
        .read_consistency_interval(Duration::ZERO)
        .execute()
        .await
        .map_err(|_| StorageError::Unavailable("vector database unavailable".into()))?;
    let mut names = db
        .table_names()
        .execute()
        .await
        .map_err(|_| StorageError::Unavailable("vector tables unavailable".into()))?;
    names.sort();
    let mut tables = Vec::new();
    for name in names {
        if name == super::lancedb::FULL_TEXT_TABLE {
            let table = db
                .open_table(&name)
                .execute()
                .await
                .map_err(|_| StorageError::Unavailable("full-text table unavailable".into()))?;
            let schema = table
                .schema()
                .await
                .map_err(|_| StorageError::Unavailable("full-text schema unavailable".into()))?;
            if schema.as_ref() != super::lancedb::full_text_schema().as_ref() {
                return Err(StorageError::InvalidArgument(
                    "incompatible full-text table",
                ));
            }
            // Vector maintenance does not rebuild or prune the independent FTS surface.
            continue;
        }
        if name.len() != 36
            || !name.starts_with("vec_")
            || !name[4..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(StorageError::InvalidArgument("unexpected vector table"));
        }
        let table = db
            .open_table(&name)
            .execute()
            .await
            .map_err(|_| StorageError::Unavailable("vector table unavailable".into()))?;
        let schema = table
            .schema()
            .await
            .map_err(|_| StorageError::Unavailable("vector schema unavailable".into()))?;
        let dim = match schema.field_with_name("vector").map(|f| f.data_type()) {
            Ok(DataType::FixedSizeList(_, dim)) if *dim > 0 => *dim as usize,
            _ => return Err(StorageError::InvalidArgument("incompatible vector schema")),
        };
        if schema.as_ref() != schema_for(dim).as_ref() {
            return Err(StorageError::InvalidArgument("incompatible vector schema"));
        }
        let before = layout(&table).await?;
        tables.push((table, before));
    }
    let mut report = CompactionReport {
        dry_run,
        target_rows_per_fragment: 4096,
        concurrency: 2,
        elapsed_ms: 0.0,
        historical_versions_retained: true,
        tables: Vec::new(),
    };
    for (index, (table, before)) in tables.into_iter().enumerate() {
        let after = if dry_run {
            None
        } else {
            if before.fragments > 1 {
                // Compact preserves indexed/unindexed coverage boundaries; a
                // small table can legitimately retain multiple fragments. Do
                // not refresh or drop indexes merely to force one fragment.
                table
                    .optimize(OptimizeAction::Compact {
                        options: CompactionOptions {
                            target_rows_per_fragment: 4096,
                            num_threads: Some(2),
                            ..Default::default()
                        },
                        remap_options: None,
                    })
                    .await
                    .map_err(|_| StorageError::Unavailable("vector compaction failed".into()))?;
            }
            let after = layout(&table).await?;
            if after.rows != before.rows {
                return Err(StorageError::Unavailable("vector row count changed".into()));
            }
            Some(after)
        };
        report.tables.push(TableCompaction {
            table_index: index,
            before,
            after,
        });
    }
    report.elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    Ok(report)
}
