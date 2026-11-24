//! Pluggable storage interfaces (M1: interfaces & transaction contracts).
//!
//! # Ownership boundary (A2.4)
//! The local API and Worker share one in-process [`StorageEngine`]. The
//! relational store holds the data directory's single-host OS lock; business
//! modules share adapters rather than opening independent databases. Graph authority
//! stays in the relational store; graph candidates share the AnySeek projection.
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

pub mod anyseek;
pub mod capabilities;
pub mod error;
pub mod ledger;
pub mod local_blob;
#[cfg(feature = "local-storage")]
pub(crate) mod retrieval_store;
pub mod scope;
#[cfg(feature = "sqlite")]
pub mod sqlite;
#[cfg(feature = "sqlite")]
pub mod sqlite_db;
pub mod traits;

pub fn hash(bytes: impl AsRef<[u8]>) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

#[cfg(feature = "local-storage")]
pub type LocalEngine = StorageEngine<
    std::sync::Arc<sqlite::SqliteStore>,
    std::sync::Arc<sqlite::SqliteStore>,
    RetrievalStore,
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

pub use capabilities::{
    BlobKey, ChunkPayload, ChunkRef, ChunkSummary, Embedding, FullTextEntry, FullTextHit,
    FullTextQuery, VectorEntry, VectorHit, VectorQuery,
};
pub use error::{StorageError, StorageResult};
pub use ledger::{LedgerEntry, LedgerKey, LedgerState, Surface};
#[cfg(feature = "local-storage")]
pub use retrieval_store::RetrievalStore;
pub use scope::{
    AuthorizedScope, KeyCapabilities, Permission, Scope, SourceVersion, WorkspaceAccess,
};
#[cfg(feature = "sqlite")]
pub use sqlite::SqliteStore;
pub use traits::{
    BlobStore, ClaimedJob, DomainTx, IssuedKey, JobFinish, JobQueue, Lifecycle, RelationalStore,
    VectorStore, WorkItem,
};

/// Assembles the four stores behind one interface. Owned by the local host;
/// API and Worker share it in-process (A2.4).
pub struct StorageEngine<R, Q, V, B> {
    relational: R,
    queue: Q,
    vector: V,
    blobs: B,
}

impl<R, Q, V, B> StorageEngine<R, Q, V, B> {
    pub fn new(relational: R, queue: Q, vector: V, blobs: B) -> Self {
        Self {
            relational,
            queue,
            vector,
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
    pub fn blobs(&self) -> &B {
        &self.blobs
    }
}

impl<R, Q, V, B> StorageEngine<R, Q, V, B>
where
    R: Lifecycle,
    Q: Lifecycle,
    V: Lifecycle,
    B: Lifecycle,
{
    /// Initialize all backends; a failure part-way leaves already-initialized
    /// backends reusable on the next attempt (A2.3).
    pub async fn initialize(&self) -> StorageResult<()> {
        self.relational.initialize().await?;
        self.queue.initialize().await?;
        self.vector.initialize().await?;
        self.blobs.initialize().await?;
        Ok(())
    }

    /// Fail fast unless every backend reports a compatible structure.
    pub async fn check(&self) -> StorageResult<()> {
        self.relational.check().await?;
        self.queue.check().await?;
        self.vector.check().await?;
        self.blobs.check().await?;
        Ok(())
    }

    /// After the host has stopped claiming, close native stores before the
    /// queue and relational authority (which may share one physical pool).
    pub async fn shutdown(&self) -> StorageResult<()> {
        self.vector.shutdown().await?;
        self.blobs.shutdown().await?;
        self.queue.shutdown().await?;
        self.relational.shutdown().await?;
        Ok(())
    }
}

#[cfg(test)]
mod engine_lifecycle_tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    struct Backend(&'static str, Arc<Mutex<Vec<String>>>);
    impl Backend {
        fn record(&self, action: &str) -> StorageResult<()> {
            self.1.lock().unwrap().push(format!("{}:{action}", self.0));
            Ok(())
        }
    }
    impl Lifecycle for Backend {
        async fn initialize(&self) -> StorageResult<()> {
            self.record("initialize")
        }
        async fn check(&self) -> StorageResult<()> {
            self.record("check")
        }
        async fn shutdown(&self) -> StorageResult<()> {
            self.record("shutdown")
        }
    }
    #[tokio::test]
    async fn projection_and_blob_shutdown_precede_authority_without_graph_backend() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let engine = StorageEngine::new(
            Backend("authority", events.clone()),
            Backend("queue", events.clone()),
            Backend("projection", events.clone()),
            Backend("blob", events.clone()),
        );
        engine.initialize().await.unwrap();
        engine.check().await.unwrap();
        engine.shutdown().await.unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            [
                "authority:initialize",
                "queue:initialize",
                "projection:initialize",
                "blob:initialize",
                "authority:check",
                "queue:check",
                "projection:check",
                "blob:check",
                "projection:shutdown",
                "blob:shutdown",
                "queue:shutdown",
                "authority:shutdown"
            ]
        );
    }
}
