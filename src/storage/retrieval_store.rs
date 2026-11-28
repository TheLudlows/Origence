//! AnySeek owns immutable chunk content and its derived retrieval indexes.
use super::{
    anyseek::{AnySeekConfig, AnySeekFullTextStore, GraphEdgeCandidate, GraphEntityCandidate},
    *,
};
use std::collections::HashMap;
use uuid::Uuid;
pub(crate) mod binding;

pub struct RetrievalStore {
    backend: AnySeekFullTextStore,
}
impl RetrievalStore {
    pub(crate) async fn open_bound(
        anyseek: AnySeekConfig,
        binding: binding::Binding,
    ) -> StorageResult<Self> {
        #[cfg(feature = "embedded-anyseek")]
        let backend = AnySeekFullTextStore::open_configured(
            anyseek,
            &binding.dir.join("anyseek"),
            !binding.existing,
        )
        .await?;
        #[cfg(not(feature = "embedded-anyseek"))]
        let backend = AnySeekFullTextStore::new(anyseek)?;
        let initialized = async {
            backend.check().await?;
            binding.commit_initial().await
        }
        .await;
        if let Err(error) = initialized {
            let _ = backend.shutdown().await;
            return Err(error);
        }
        Ok(Self {
            backend: backend.with_binding(binding),
        })
    }
    pub(crate) fn private_archive(&self) -> &AnySeekFullTextStore {
        &self.backend
    }
    pub(crate) async fn check_scope(&self, scope: Scope) -> StorageResult<()> {
        self.backend.check_scope(scope, false).await
    }
    pub(crate) async fn read_payloads(
        &self,
        scope: Scope,
        ids: &[Uuid],
    ) -> StorageResult<HashMap<Uuid, ChunkPayload>> {
        self.backend.read_payloads(scope, ids).await
    }
    pub async fn open(anyseek: AnySeekConfig) -> StorageResult<Self> {
        Ok(Self {
            backend: AnySeekFullTextStore::new(anyseek)?,
        })
    }
    pub async fn check_full_text_available(&self) -> StorageResult<()> {
        self.backend.check().await
    }
    pub async fn check_vector_available(
        &self,
        profile: &str,
        dimension: usize,
    ) -> StorageResult<()> {
        self.backend.check_vector(profile, dimension).await
    }
    pub(crate) async fn search_authorized(
        &self,
        query: VectorQuery,
        expected: &HashMap<Uuid, SourceVersion>,
    ) -> StorageResult<Vec<VectorHit>> {
        self.backend.search_vector(query, expected).await
    }
    pub(crate) async fn search_full_text_authorized(
        &self,
        query: FullTextQuery,
        expected: &HashMap<Uuid, SourceVersion>,
    ) -> StorageResult<Vec<FullTextHit>> {
        self.backend.search(query, expected).await
    }
    pub(crate) async fn search_summaries_authorized(
        &self,
        query: FullTextQuery,
        expected: &HashMap<Uuid, SourceVersion>,
    ) -> StorageResult<Vec<FullTextHit>> {
        self.backend.search_summaries(query, expected).await
    }
    pub(crate) async fn graph_seed_candidates(
        &self,
        scope: Scope,
        terms: &str,
    ) -> StorageResult<Vec<GraphEntityCandidate>> {
        self.backend.graph_seed_candidates(scope, terms).await
    }
    pub(crate) async fn graph_neighbors(
        &self,
        scope: Scope,
        seeds: &[Uuid],
    ) -> StorageResult<Vec<GraphEdgeCandidate>> {
        self.backend.graph_neighbors(scope, seeds).await
    }
    pub(crate) async fn upsert_publication(
        &self,
        scope: Scope,
        entries: Vec<FullTextEntry>,
        vectors: Vec<VectorEntry>,
        graph: Option<super::anyseek::GraphPublication>,
        epoch: Option<u64>,
    ) -> StorageResult<()> {
        self.backend
            .upsert_with_graph(
                scope,
                entries,
                vectors,
                graph,
                epoch.ok_or(StorageError::InvalidArgument(
                    "AnySeek publication requires a saved retry epoch",
                ))?,
            )
            .await
    }
}
impl Lifecycle for RetrievalStore {
    async fn initialize(&self) -> StorageResult<()> {
        self.check().await
    }
    async fn check(&self) -> StorageResult<()> {
        self.backend.check().await
    }
    async fn shutdown(&self) -> StorageResult<()> {
        self.backend.shutdown().await
    }
}
impl VectorStore for RetrievalStore {
    async fn upsert(&self, _scope: Scope, entries: Vec<VectorEntry>) -> StorageResult<()> {
        if entries.is_empty() {
            Ok(())
        } else {
            Err(StorageError::InvalidArgument(
                "AnySeek writes require the durable publication path",
            ))
        }
    }
    async fn search(&self, _query: VectorQuery) -> StorageResult<Vec<VectorHit>> {
        Err(StorageError::InvalidArgument(
            "AnySeek queries require authoritative candidate sources",
        ))
    }
    async fn upsert_full_text(
        &self,
        _scope: Scope,
        entries: Vec<FullTextEntry>,
    ) -> StorageResult<()> {
        if entries.is_empty() {
            Ok(())
        } else {
            Err(StorageError::InvalidArgument(
                "AnySeek writes require the durable publication path",
            ))
        }
    }
    async fn search_full_text(&self, _query: FullTextQuery) -> StorageResult<Vec<FullTextHit>> {
        Err(StorageError::InvalidArgument(
            "AnySeek queries require authoritative candidate sources",
        ))
    }
    async fn delete_objects(&self, scope: Scope, ids: Vec<Uuid>) -> StorageResult<()> {
        self.backend.delete_objects(scope, ids).await
    }
    async fn delete_source(&self, _scope: Scope, _source: SourceVersion) -> StorageResult<()> {
        Err(StorageError::InvalidArgument(
            "AnySeek cleanup requires authoritative artifact IDs",
        ))
    }
}
