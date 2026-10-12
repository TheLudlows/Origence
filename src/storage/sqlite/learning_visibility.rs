//! One fail-closed guard for every published Learning read and worker write.
//!
//! Exact archive checks are local (nonrecursive). Learning dependencies are
//! traversed iteratively, so a malformed cycle cannot recurse on the async stack
//! and an over-limit closure is rejected rather than partially validated.
use super::*;
use crate::session::ArchivedEvidence;
use std::collections::HashSet;

const MAX_LEARNING_CLOSURE: usize = 64;

#[derive(Clone, Copy)]
struct LearningRecord {
    id: Uuid,
    asset: Uuid,
    source: Uuid,
    session: Uuid,
    published: bool,
    accepted: bool,
}

enum Visit {
    Enter(Uuid),
    Leave(Uuid),
}

impl SqliteTx {
    async fn learning_catalog_present(&mut self) -> StorageResult<bool> {
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='oc_learnings')",
        )
        .fetch_one(&mut *self.tx)
        .await
        .map_err(sqlite_err)
    }

    async fn learning_record(&mut self, asset: Uuid) -> StorageResult<Option<LearningRecord>> {
        let row: Option<(Uuid, Uuid, Uuid, String, bool)> = sqlx::query_as("SELECT l.id,l.source_event_id,l.session_id,l.status,EXISTS(SELECT 1 FROM oc_improve_runs r WHERE r.tenant_id=l.tenant_id AND r.workspace_id=l.workspace_id AND r.improve_id=l.improve_id AND r.status IN ('accepted','publishing')) FROM oc_learnings l WHERE l.tenant_id=? AND l.workspace_id=? AND l.asset_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(asset)
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        Ok(row.map(|(id, source, session, status, run_active)| LearningRecord {
            id,
            asset,
            source,
            session,
            published: status == "published",
            accepted: status == "accepted" && run_active,
        }))
    }

    async fn has_learning_source_version(&mut self, asset: Uuid) -> StorageResult<bool> {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_versions v JOIN oc_events e ON e.tenant_id=v.tenant_id AND e.workspace_id=v.workspace_id AND e.id=v.source_event_id WHERE v.tenant_id=? AND v.workspace_id=? AND v.asset_id=? AND e.kind='learning')")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(asset)
            .fetch_one(&mut *self.tx).await.map_err(sqlite_err)
    }

    /// Also catches an orphaned source mapping, so generic edits cannot turn a
    /// Learning into an ordinary asset by losing its catalog row.
    pub async fn is_learning_asset(&mut self, asset: Uuid) -> StorageResult<bool> {
        if self.learning_catalog_present().await? && self.learning_record(asset).await?.is_some() {
            return Ok(true);
        }
        self.has_learning_source_version(asset).await
    }

    async fn learning_asset_for_source(&mut self, source: Uuid) -> StorageResult<Option<Uuid>> {
        let kind: Option<String> = sqlx::query_scalar("SELECT kind FROM oc_events WHERE tenant_id=? AND workspace_id=? AND id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(source)
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        let asset = if self.learning_catalog_present().await? {
            sqlx::query_scalar("SELECT asset_id FROM oc_learnings WHERE tenant_id=? AND workspace_id=? AND source_event_id=?")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(source)
                .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?
        } else {
            None
        };
        if kind.as_deref() == Some("learning") && asset.is_none() {
            return Err(StorageError::NotFound);
        }
        Ok(asset)
    }

    /// All archived references matter, including references the candidate did
    /// not quote and original S2 member sources different from the asset root.
    async fn learning_roots_for_archives(
        &mut self,
        evidence: &[ArchivedEvidence],
    ) -> StorageResult<Vec<Uuid>> {
        let mut assets = HashSet::new();
        let mut sources = HashSet::new();
        for item in evidence {
            assets.insert(item.reference.asset_id);
            sources.insert(item.source_event_id);
            sources.insert(item.version_source_event_id);
            if item.locator["parser"] == crate::memory_semantics::PARSER {
                let support = item.locator["support_sources"]
                    .as_array()
                    .ok_or(StorageError::NotFound)?;
                if support.is_empty() {
                    return Err(StorageError::NotFound);
                }
                for source in support {
                    let source = source
                        .as_str()
                        .and_then(|value| Uuid::parse_str(value).ok())
                        .ok_or(StorageError::NotFound)?;
                    sources.insert(source);
                }
            }
        }
        for source in sources {
            if let Some(asset) = self.learning_asset_for_source(source).await? {
                assets.insert(asset);
            }
        }
        let mut assets: Vec<_> = assets.into_iter().collect();
        assets.sort_unstable();
        Ok(assets)
    }

    /// DFS colors distinguish a shared dependency from a cycle. The accepted
    /// exception applies only to the exact worker root, never its dependencies.
    async fn guard_learning_dependencies(
        &mut self,
        roots: Vec<Uuid>,
        accepted_root: Option<Uuid>,
    ) -> StorageResult<()> {
        let catalog = self.learning_catalog_present().await?;
        let mut stack: Vec<_> = roots.into_iter().rev().map(Visit::Enter).collect();
        let mut visiting = HashSet::new();
        let mut complete = HashSet::new();
        let mut learning_count = 0;
        let mut turn_dependencies: HashMap<(Uuid, Uuid), Vec<Uuid>> = HashMap::new();
        while let Some(visit) = stack.pop() {
            let asset = match visit {
                Visit::Leave(asset) => {
                    visiting.remove(&asset);
                    complete.insert(asset);
                    continue;
                }
                Visit::Enter(asset) => asset,
            };
            if complete.contains(&asset) {
                continue;
            }
            if visiting.contains(&asset) {
                return Err(StorageError::NotFound);
            }
            let record = if catalog {
                self.learning_record(asset).await?
            } else {
                None
            };
            let Some(record) = record else {
                if self.has_learning_source_version(asset).await? {
                    return Err(StorageError::NotFound);
                }
                complete.insert(asset);
                continue;
            };
            learning_count += 1;
            if learning_count + turn_dependencies.len() > MAX_LEARNING_CLOSURE
                || !(record.published || (accepted_root == Some(asset) && record.accepted))
            {
                return Err(StorageError::NotFound);
            }
            let active: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_assets a JOIN oc_events e ON e.tenant_id=a.tenant_id AND e.workspace_id=a.workspace_id AND e.id=? WHERE a.tenant_id=? AND a.workspace_id=? AND a.id=? AND a.kind='knowledge' AND NOT a.deleted AND e.kind='learning' AND e.state='active')")
                .bind(record.source).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(record.asset)
                .fetch_one(&mut *self.tx).await.map_err(sqlite_err)?;
            if !active {
                return Err(StorageError::NotFound);
            }
            // Fetch one extra row to detect truncation, never authorize a prefix.
            let supports: Vec<(Uuid, Uuid)> = sqlx::query_as("SELECT DISTINCT session_id,turn_id FROM oc_learning_support WHERE tenant_id=? AND workspace_id=? AND learning_id=? ORDER BY session_id,turn_id LIMIT 65")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(record.id)
                .fetch_all(&mut *self.tx).await.map_err(sqlite_err)?;
            if supports.is_empty() || supports.len() > MAX_LEARNING_CLOSURE {
                return Err(StorageError::NotFound);
            }
            let mut dependencies = HashSet::new();
            for (session, turn) in supports {
                if session != record.session {
                    return Err(StorageError::NotFound);
                }
                if let Some(roots) = turn_dependencies.get(&(session, turn)) {
                    dependencies.extend(roots.iter().copied());
                } else {
                    if learning_count + turn_dependencies.len() >= MAX_LEARNING_CLOSURE {
                        return Err(StorageError::NotFound);
                    }
                    let (_, evidence) = self.require_turn_visible_local(session, turn).await?;
                    let roots = self.learning_roots_for_archives(&evidence).await?;
                    dependencies.extend(roots.iter().copied());
                    turn_dependencies.insert((session, turn), roots);
                }
            }
            let mut dependencies: Vec<_> = dependencies.into_iter().collect();
            dependencies.sort_unstable();
            visiting.insert(asset);
            stack.push(Visit::Leave(asset));
            stack.extend(dependencies.into_iter().rev().map(Visit::Enter));
        }
        Ok(())
    }

    pub(super) async fn require_archived_evidence_set_visible(
        &mut self,
        evidence: &[ArchivedEvidence],
    ) -> StorageResult<()> {
        let roots = self.learning_roots_for_archives(evidence).await?;
        self.guard_learning_dependencies(roots, None).await
    }

    pub(super) async fn require_archived_evidence_visible(
        &mut self,
        evidence: &ArchivedEvidence,
    ) -> StorageResult<()> {
        self.require_archived_evidence_set_visible(std::slice::from_ref(evidence))
            .await
    }

    pub async fn require_learning_asset_visible(&mut self, asset: Uuid) -> StorageResult<()> {
        self.guard_learning_dependencies(vec![asset], None).await
    }

    pub async fn learning_asset_visible(&mut self, asset: Uuid) -> StorageResult<bool> {
        match self.require_learning_asset_visible(asset).await {
            Ok(()) => Ok(true),
            Err(StorageError::NotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub(super) async fn require_learning_sources_visible(
        &mut self,
        sources: &[Uuid],
    ) -> StorageResult<()> {
        let mut roots = HashSet::new();
        for source in sources {
            if let Some(asset) = self.learning_asset_for_source(*source).await? {
                roots.insert(asset);
            }
        }
        let mut roots: Vec<_> = roots.into_iter().collect();
        roots.sort_unstable();
        self.guard_learning_dependencies(roots, None).await
    }

    pub async fn require_learning_source_visible(&mut self, source: Uuid) -> StorageResult<()> {
        self.require_learning_sources_visible(&[source]).await
    }

    /// Privileged maintenance uses the same read guard in its existing write
    /// transaction. Logical withdrawal is immediate; native cleanup is eventual.
    pub(super) async fn detach_unsupported_learning_owners(&mut self) -> StorageResult<()> {
        if !self.learning_catalog_present().await? {
            return Ok(());
        }
        let records: Vec<(Uuid, Uuid)> = sqlx::query_as("SELECT asset_id,source_event_id FROM oc_learnings WHERE tenant_id=? AND workspace_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id)
            .fetch_all(&mut *self.tx).await.map_err(sqlite_err)?;
        for (asset, source) in records {
            if !self.learning_asset_visible(asset).await? {
                sqlx::query("DELETE FROM oc_artifact_owners WHERE tenant_id=? AND workspace_id=? AND source_id=?")
                    .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(source)
                    .execute(&mut *self.tx).await.map_err(sqlite_err)?;
            }
        }
        Ok(())
    }

    /// Called inside each existing worker guard, including the final write
    /// transaction. Accepted roots may publish; unsupported dependencies may not.
    pub async fn guard_learning_publication(
        &mut self,
        asset: Option<Uuid>,
        source: Option<Uuid>,
    ) -> StorageResult<()> {
        if let Some(asset) = asset {
            let record = if self.learning_catalog_present().await? {
                self.learning_record(asset).await?
            } else {
                None
            };
            if let Some(record) = record {
                if source != Some(record.source) {
                    return Err(StorageError::NotFound);
                }
                return self.guard_learning_dependencies(vec![asset], Some(asset)).await;
            }
            self.require_learning_asset_visible(asset).await?;
        }
        if let Some(source) = source {
            self.require_learning_source_visible(source).await?;
        }
        Ok(())
    }
}
