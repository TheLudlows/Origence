//! native SQLite relational store, queue and lifecycle (M2).
//!
//! The local relational backend: one native SQLite database file holds the
//! authoritative tenant/workspace objects, the idempotency ledger, audit, job
//! registration and the self-polled job queue (A5). Vector and graph data live
//! in other backends (M4); raw file bytes live on the local filesystem.
//!
//! Existing responsibilities are private sibling modules: `lifecycle`/`schema`/
//! `locking` own opening and resource lifetime; `relational` owns privileged
//! entry points; `domain` keeps the authorization/idempotency transaction envelope
//! and delegates row operations to `identity`, `versions`, `provenance`, `ledger`
//! and `job_queue`. Delegates borrow the same `SqliteTx`; they never open another
//! transaction. Feature-specific modules keep their existing boundaries.
//!
//! Encodings are fixed in [`sqlite-schema.sql`](include_str!("sqlite-schema.sql")).

// Domain operations map 1:1 onto A5 table rows: wide row tuples and wide
// signatures are inherent to the adapter boundary and are clearer inline than
// factored into a per-query struct.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::storage::sqlite_db::{self, Connection, Pool, Transaction};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

mod app;
#[cfg(feature = "host")]
mod archive_restore;
mod catalog;
#[cfg(feature = "host")]
mod consolidation;
#[cfg(feature = "host")]
mod context;
#[cfg(all(test, feature = "host"))]
mod context_route_tests;
mod deployment;
#[cfg(feature = "host")]
pub use deployment::DeploymentKeyOperation;
mod domain;
#[cfg(feature = "host")]
mod full_text;
pub(crate) mod global_knowledge;
mod identity;
mod improve;
#[cfg(test)]
mod job_improve_tests;
mod job_queue;
mod learning_visibility;
mod ledger;
mod lifecycle;
mod locking;
mod private_archive;
mod private_session;

mod provenance;
mod publication;
#[cfg(test)]
mod publication_shape_tests;
mod read_model;
#[cfg(test)]
mod read_model_tests;
mod relational;
#[cfg(feature = "host")]
pub(crate) mod retrieval;
mod run_metering;
mod run_verification;
mod runs;
mod schema;
mod semantics;
mod session;
#[cfg(test)]
mod session_read_tests;
mod skill;
#[cfg(test)]
mod skill_tests;
mod versions;
#[cfg(test)]
mod versions_tests;
mod visibility;
mod workspace_grants;
mod workspace_key;

use crate::storage::error::{StorageError, StorageResult};
use crate::storage::ledger::{LEDGER_PENDING_TIMEOUT_MS, LedgerEntry, LedgerKey};
use crate::storage::scope::{AuthorizedScope, KeyCapabilities, Permission, Scope, SourceVersion};
use crate::storage::traits::{
    ClaimedJob, DomainTx, IssuedKey, JobFinish, JobQueue, RelationalStore, WorkItem,
};
use crate::storage::{ChunkRef, Lifecycle};

use locking::LockGuard;

/// The local relational store backed by a single native SQLite database file.
pub struct SqliteStore {
    pool: Pool,
    #[cfg(feature = "host")]
    pub(crate) archive_dependencies: tokio::sync::Mutex<()>,
    path: PathBuf,
    /// In-process lock holder bound to the database path (A2.5). Dropping it
    /// (or crashing the process) releases the OS lock.
    _lock: std::sync::Arc<LockGuard>,
}

/// Holds the existing host lock without opening/initializing an application store.
#[cfg(test)]
pub(super) struct MaintenanceLock {
    _guard: std::sync::Arc<LockGuard>,
}

fn sqlite_err(e: sqlite_db::Error) -> StorageError {
    match e {
        sqlite_db::Error::Incompatible(message) => StorageError::Unavailable(message.into()),
        error => StorageError::Backend(error.to_string()),
    }
}

/// SHA-256 hex digest, shared with the PostgreSQL adapter's token hash.
fn hash(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Exponential retry delay in milliseconds, clamped (A2.5).
fn retry_delay(attempt: i32) -> i64 {
    (1i64 << attempt.clamp(0, 20) as u32) * 1000
}

/// The idempotency key opened by [`DomainTx::begin_command`] and closed by
/// [`DomainTx::finish_command`].
struct PendingCommand {
    dedup_scope: String,
    operation: String,
    key: String,
    hash: String,
}

/// A scoped transaction over one native SQLite connection. Writes execute inside the
/// underlying transaction, so `commit`/`rollback` make them atomic (A2.2).
pub struct SqliteTx {
    tx: Transaction,
    scope: Scope,
    principal_id: Uuid,
    required: Vec<Permission>,
    command: Option<PendingCommand>,
}
