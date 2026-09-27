//! Pluggable storage interfaces (M1: interfaces & transaction contracts).
//!
//! # Ownership and blocking-call boundary (A2.4)
//! The local API and Worker share one in-process [`StorageEngine`]; the engine
//! (and the Kuzu database file it owns) has a single host process. Business
//! modules hold a shared adapter reference and never open their own Kuzu
//! database file — a second process that opens the same Kuzu file is rejected
//! by its file lock. Synchronous Kuzu calls are confined to the adapter's
//! bounded blocking executor so they cannot starve the async API/Worker
//! scheduler.
//!
//! # Shutdown order
//! The host stops and joins the worker before [`StorageEngine::shutdown`].
//! Native stores close before the queue/relational authority; SQLite backs
//! both queue and relational roles, so shutting down either closes its pool.
//!
//! # Transaction boundary
//! [`RelationalStore::begin`] returns a [`DomainTx`] that carries its scope and
//! exposes only domain operations — no `PgPool`/`SqlitePool`, no raw SQL, no
//! vendor types. `enqueue` is a domain operation on that same transaction;
//! [`JobQueue`] is the consumer side (claim/ack/retry) and never enqueues on a
//! second connection.

pub mod capabilities;
pub mod error;
#[cfg(feature = "local-graph")]
pub mod kuzu;
#[cfg(feature = "local-vector")]
pub mod lancedb;
pub mod ledger;
pub mod local_blob;
pub mod scope;
pub mod sqlite;
pub mod traits;

pub fn hash(bytes: impl AsRef<[u8]>) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

#[cfg(feature = "local-storage")]
pub type LocalEngine = StorageEngine<
    std::sync::Arc<sqlite::SqliteStore>,
    std::sync::Arc<sqlite::SqliteStore>,
    LanceDbStore,
    KuzuStore,
    local_blob::LocalBlobStore,
>;

impl<T: Lifecycle> Lifecycle for std::sync::Arc<T> {
    async fn initialize(&self) -> StorageResult<()> {
        self.as_ref().initialize().await
    }
    async fn check(&self) -> StorageResult<()> {
        self.as_ref().check().await
    }
    async fn shutdown(&self) -> StorageResult<()> {
        self.as_ref().shutdown().await
    }
}

pub use capabilities::{BlobKey, Capabilities, Embedding, VectorEntry, VectorHit, VectorQuery};
pub use error::{StorageError, StorageResult};
#[cfg(feature = "local-graph")]
pub use kuzu::KuzuStore;
#[cfg(feature = "local-vector")]
pub use lancedb::LanceDbStore;
pub use ledger::{LedgerEntry, LedgerKey, LedgerState, Surface, ledger_idempotency_key};
pub use scope::{AuthorizedScope, Permission, Scope, SourceVersion};
pub use sqlite::SqliteStore;
pub use traits::{
    BlobStore, ClaimedJob, DomainTx, GraphStore, IssuedKey, JobFinish, JobQueue, Lifecycle,
    RelationalStore, VectorStore, WorkItem,
};

/// Assembles the four stores behind one interface. Owned by the local host;
/// API and Worker share it in-process (A2.4).
pub struct StorageEngine<R, Q, V, G, B> {
    relational: R,
    queue: Q,
    vector: V,
    graph: G,
    blobs: B,
}

impl<R, Q, V, G, B> StorageEngine<R, Q, V, G, B> {
    pub fn new(relational: R, queue: Q, vector: V, graph: G, blobs: B) -> Self {
        Self {
            relational,
            queue,
            vector,
            graph,
            blobs,
        }
    }

    pub fn relational(&self) -> &R {
        &self.relational
    }
    pub fn queue(&self) -> &Q {
        &self.queue
    }
    pub fn vector(&self) -> &V {
        &self.vector
    }
    pub fn graph(&self) -> &G {
        &self.graph
    }
    pub fn blobs(&self) -> &B {
        &self.blobs
    }
}

impl<R, Q, V, G, B> StorageEngine<R, Q, V, G, B>
where
    R: Lifecycle,
    Q: Lifecycle,
    V: Lifecycle,
    G: Lifecycle,
    B: Lifecycle,
{
    /// Initialize all backends; a failure part-way leaves already-initialized
    /// backends reusable on the next attempt (A2.3).
    pub async fn initialize(&self) -> StorageResult<()> {
        self.relational.initialize().await?;
        self.queue.initialize().await?;
        self.vector.initialize().await?;
        self.graph.initialize().await?;
        self.blobs.initialize().await?;
        Ok(())
    }

    /// Fail fast unless every backend reports a compatible structure.
    pub async fn check(&self) -> StorageResult<()> {
        self.relational.check().await?;
        self.queue.check().await?;
        self.vector.check().await?;
        self.graph.check().await?;
        self.blobs.check().await?;
        Ok(())
    }

    /// After the host has stopped claiming, close native stores before the
    /// queue and relational authority (which may share one physical pool).
    pub async fn shutdown(&self) -> StorageResult<()> {
        self.vector.shutdown().await?;
        self.graph.shutdown().await?;
        self.blobs.shutdown().await?;
        self.queue.shutdown().await?;
        self.relational.shutdown().await?;
        Ok(())
    }
}
