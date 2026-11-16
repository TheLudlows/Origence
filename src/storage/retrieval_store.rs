//! Compose the existing vector backend with an independently selected lexical store.
use super::{
    anyseek::{AnySeekConfig, AnySeekFullTextStore},
    *,
};
use std::{collections::HashMap, path::Path};
use uuid::Uuid;

pub struct RetrievalStore {
    local: LanceDbStore,
    lexical: Option<AnySeekFullTextStore>,
}

impl RetrievalStore {
    pub async fn open(
        dir: impl AsRef<Path>,
        anyseek: Option<AnySeekConfig>,
    ) -> StorageResult<Self> {
        Ok(Self {
            local: LanceDbStore::open(dir).await?,
            lexical: anyseek.map(AnySeekFullTextStore::new).transpose()?,
        })
    }
    pub fn uses_anyseek(&self) -> bool {
        self.lexical.is_some()
    }
    pub async fn check_full_text_available(&self) -> StorageResult<()> {
        match &self.lexical {
            Some(store) => store.check().await,
            None => self.local.check_full_text_available().await,
        }
    }
    pub async fn check_vector_available(
        &self,
        profile: &str,
        dimension: usize,
    ) -> StorageResult<()> {
        self.local.check_vector_available(profile, dimension).await
    }
    pub(crate) async fn search_authorized(
        &self,
        query: VectorQuery,
        expected: &HashMap<Uuid, SourceVersion>,
    ) -> StorageResult<Vec<VectorHit>> {
        self.local.search_authorized(query, expected).await
    }
    pub(crate) async fn search_full_text_authorized(
        &self,
        query: FullTextQuery,
        expected: &HashMap<Uuid, SourceVersion>,
    ) -> StorageResult<Vec<FullTextHit>> {
        match &self.lexical {
            Some(store) => store.search(query, expected).await,
            None => {
                self.local
                    .search_full_text_authorized(query, expected)
                    .await
            }
        }
    }
    pub(crate) async fn upsert_publication_full_text(
        &self,
        scope: Scope,
        entries: Vec<FullTextEntry>,
        epoch: Option<u64>,
    ) -> StorageResult<()> {
        match &self.lexical {
            Some(store) => {
                store
                    .upsert(
                        scope,
                        entries,
                        epoch.ok_or(StorageError::InvalidArgument(
                            "AnySeek publication requires a saved retry epoch",
                        ))?,
                    )
                    .await
            }
            None => self.local.upsert_full_text(scope, entries).await,
        }
    }
}

impl Lifecycle for RetrievalStore {
    async fn initialize(&self) -> StorageResult<()> {
        self.check().await
    }
    async fn check(&self) -> StorageResult<()> {
        self.local.check().await?;
        if let Some(store) = &self.lexical {
            store.check().await?;
        }
        Ok(())
    }
    async fn shutdown(&self) -> StorageResult<()> {
        self.local.shutdown().await
    }
}

impl VectorStore for RetrievalStore {
    async fn upsert(&self, scope: Scope, entries: Vec<VectorEntry>) -> StorageResult<()> {
        self.local.upsert(scope, entries).await
    }
    async fn search(&self, query: VectorQuery) -> StorageResult<Vec<VectorHit>> {
        self.local.search(query).await
    }
    async fn upsert_full_text(
        &self,
        scope: Scope,
        entries: Vec<FullTextEntry>,
    ) -> StorageResult<()> {
        if self.lexical.is_some() && !entries.is_empty() {
            return Err(StorageError::InvalidArgument(
                "AnySeek writes require the durable publication path",
            ));
        }
        self.local.upsert_full_text(scope, entries).await
    }
    async fn search_full_text(&self, query: FullTextQuery) -> StorageResult<Vec<FullTextHit>> {
        if self.lexical.is_some() {
            return Err(StorageError::InvalidArgument(
                "AnySeek queries require authoritative candidate sources",
            ));
        }
        self.local.search_full_text(query).await
    }
    async fn delete_objects(&self, scope: Scope, ids: Vec<Uuid>) -> StorageResult<()> {
        if let Some(store) = &self.lexical {
            store.delete_objects(scope, ids.clone()).await?;
        }
        self.local.delete_objects(scope, ids).await
    }
    async fn delete_source(&self, scope: Scope, source: SourceVersion) -> StorageResult<()> {
        if self.lexical.is_some() {
            return Err(StorageError::InvalidArgument(
                "AnySeek cleanup requires authoritative artifact IDs",
            ));
        }
        self.local.delete_source(scope, source).await
    }
}
