//! SQLite relational store, queue and lifecycle (M2).
//!
//! The local relational backend: one SQLite database file holds the
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

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use sqlx::Transaction;
use sqlx::sqlite::SqlitePool;
use uuid::Uuid;

mod aml;
mod app;
#[cfg(feature = "local-storage")]
mod context;
mod domain;
mod identity;
mod improve;
mod job_queue;
mod learning_visibility;
mod ledger;
mod lifecycle;
mod locking;
mod provenance;
mod relational;
mod schema;
mod semantics;
mod session;
mod versions;
mod visibility;

use crate::storage::Lifecycle;
use crate::storage::error::{StorageError, StorageResult};
use crate::storage::ledger::{
    LEDGER_PENDING_TIMEOUT_MS, LedgerEntry, LedgerKey, MAX_LEDGER_ATTEMPTS,
};
use crate::storage::scope::{AuthorizedScope, Permission, Scope, SourceVersion};
use crate::storage::traits::{
    ClaimedJob, DomainTx, IssuedKey, JobFinish, JobQueue, RelationalStore, WorkItem,
};

use locking::LockGuard;
#[cfg(feature = "local-vector")]
pub(super) use locking::lock_for_maintenance;
use schema::identity_table_present;

/// Result of the explicit, offline identity schema upgrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryIdentityUpgrade {
    Required,
    Enabled,
    AlreadyEnabled,
}

/// The local relational store backed by a single SQLite database file.
pub struct SqliteStore {
    pool: SqlitePool,
    path: PathBuf,
    /// In-process lock holder bound to the database path (A2.5). Dropping it
    /// (or crashing the process) releases the OS lock.
    _lock: LockGuard,
}

/// Holds the existing host lock without opening/initializing an application store.
#[cfg(feature = "local-vector")]
pub(super) struct MaintenanceLock {
    _guard: LockGuard,
}

/// Roles an API key may hold (A2.2 role matrix).
const ROLES: &[&str] = &["reader", "writer", "reviewer", "admin"];

fn sqlite_err(e: sqlx::Error) -> StorageError {
    StorageError::Backend(e.to_string())
}

/// SHA-256 hex digest, shared with the PostgreSQL adapter's token hash.
fn hash(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

fn now_ms() -> i64 {
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
    operation: String,
    key: String,
    hash: String,
}

/// A scoped transaction over one SQLite connection. Writes execute inside the
/// underlying transaction, so `commit`/`rollback` make them atomic (A2.2).
pub struct SqliteTx {
    tx: Transaction<'static, sqlx::Sqlite>,
    scope: Scope,
    principal_id: Uuid,
    required: Vec<Permission>,
    command: Option<PendingCommand>,
}
