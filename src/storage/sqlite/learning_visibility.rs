//! One fail-closed guard for every published Learning read and worker write.
//!
//! Exact archive checks are local (nonrecursive). Learning dependencies are
//! traversed iteratively, so a malformed cycle cannot recurse on the async stack
//! and an over-limit closure is rejected rather than partially validated.
use super::*;
use crate::session::ArchivedEvidence;
use std::collections::HashSet;

const MAX_LEARNING_CLOSURE: usize = 64;
const LEARNING_READ_BATCH: usize = 256;

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

impl TursoTx {
    async fn learning_catalog_present(&mut self) -> StorageResult<bool> {
        native_turso::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='oc_learnings')",
        )
        .fetch_one(&mut *self.tx)
        .await
        .map_err(turso_err)
    }

    async fn learning_record(&mut self, asset: Uuid) -> StorageResult<Option<LearningRecord>> {
        let row: Option<(Uuid, Uuid, Uuid, String, Uuid)> = native_turso::query_as("SELECT id,source_event_id,session_id,status,improve_id FROM oc_learnings WHERE tenant_id=? AND workspace_id=? AND asset_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(asset)
            .fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        let Some((id, source, session, status, improve)) = row else {
            return Ok(None);
        };
        let run_status: Option<String> = native_turso::query_scalar(
            "SELECT status FROM oc_improve_runs WHERE tenant_id=? AND workspace_id=? AND improve_id=?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(improve)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        let run_active = matches!(run_status.as_deref(), Some("accepted" | "publishing"));
        Ok(Some(LearningRecord {
            id,
            asset,
            source,
            session,
            published: status == "published",
            accepted: status == "accepted" && run_active,
        }))
    }

    async fn has_learning_source_version(&mut self, asset: Uuid) -> StorageResult<bool> {
        Ok(self
            .learning_source_assets(&[asset])
            .await?
            .contains(&asset))
    }

    /// Classify all history without one event lookup per version or loading an
    /// unbounded history at once. Retraction does not erase Learning identity.
    async fn learning_source_assets(&mut self, assets: &[Uuid]) -> StorageResult<HashSet<Uuid>> {
        let mut found = HashSet::new();
        for batch in assets.chunks(LEARNING_READ_BATCH) {
            let placeholders = vec!["?"; batch.len()].join(",");
            let mut cursor: Option<(Uuid, i64)> = None;
            loop {
                let continuation = if cursor.is_some() {
                    " AND (asset_id>? OR (asset_id=? AND version>?))"
                } else {
                    ""
                };
                let sql = format!(
                    "SELECT asset_id,version,source_event_id FROM oc_versions \
                     WHERE tenant_id=? AND workspace_id=? AND asset_id IN ({placeholders})\
                     {continuation} ORDER BY asset_id,version LIMIT {LEARNING_READ_BATCH}"
                );
                let mut query = native_turso::query_as::<(Uuid, i64, Uuid)>(&sql)
                    .bind(self.scope.tenant_id)
                    .bind(self.scope.workspace_id);
                for asset in batch {
                    query = query.bind(*asset);
                }
                if let Some((asset, version)) = cursor {
                    query = query.bind(asset).bind(asset).bind(version);
                }
                let versions = query.fetch_all(&mut *self.tx).await.map_err(turso_err)?;
                let Some((last_asset, last_version, _)) = versions.last() else {
                    break;
                };
                cursor = Some((*last_asset, *last_version));
                let mut sources: Vec<_> = versions.iter().map(|(_, _, source)| *source).collect();
                sources.sort_unstable();
                sources.dedup();
                let placeholders = vec!["?"; sources.len()].join(",");
                let sql = format!(
                    "SELECT id FROM oc_events WHERE tenant_id=? AND workspace_id=? \
                     AND kind='learning' AND id IN ({placeholders})"
                );
                let mut query = native_turso::query_scalar::<Uuid>(&sql)
                    .bind(self.scope.tenant_id)
                    .bind(self.scope.workspace_id);
                for source in sources {
                    query = query.bind(source);
                }
                let learning_sources: HashSet<_> = query
                    .fetch_all(&mut *self.tx)
                    .await
                    .map_err(turso_err)?
                    .into_iter()
                    .collect();
                for (asset, _, source) in &versions {
                    if learning_sources.contains(source) {
                        found.insert(*asset);
                    }
                }
                if versions.len() < LEARNING_READ_BATCH
                    || batch.iter().all(|asset| found.contains(asset))
                {
                    break;
                }
            }
        }
        Ok(found)
    }

    /// One read-call classification for retrieval batches. Ordinary assets are
    /// checked in bulk; only actual Learning records enter the existing bounded
    /// dependency traversal. This does not establish asset existence or local
    /// root/support activity. Nothing is cached across calls or writes.
    pub(super) async fn learning_assets_visible(
        &mut self,
        assets: &[Uuid],
    ) -> StorageResult<HashMap<Uuid, bool>> {
        let mut assets = assets.to_vec();
        assets.sort_unstable();
        assets.dedup();
        let mut registered = HashSet::new();
        if !assets.is_empty() && self.learning_catalog_present().await? {
            for batch in assets.chunks(LEARNING_READ_BATCH) {
                let placeholders = vec!["?"; batch.len()].join(",");
                let sql = format!(
                    "SELECT asset_id FROM oc_learnings WHERE tenant_id=? AND workspace_id=? \
                     AND asset_id IN ({placeholders})"
                );
                let mut query = native_turso::query_scalar::<Uuid>(&sql)
                    .bind(self.scope.tenant_id)
                    .bind(self.scope.workspace_id);
                for asset in batch {
                    query = query.bind(*asset);
                }
                registered.extend(query.fetch_all(&mut *self.tx).await.map_err(turso_err)?);
            }
        }
        let ordinary: Vec<_> = assets
            .iter()
            .copied()
            .filter(|asset| !registered.contains(asset))
            .collect();
        let orphaned = self.learning_source_assets(&ordinary).await?;
        let mut visible = HashMap::new();
        for asset in assets {
            let allowed = if registered.contains(&asset) {
                self.learning_asset_visible(asset).await?
            } else {
                !orphaned.contains(&asset)
            };
            visible.insert(asset, allowed);
        }
        Ok(visible)
    }

    /// Batch the Learning layer for semantic support sources as well as the
    /// version root. A normal S2 asset can still depend on a Learning source.
    pub(super) async fn learning_source_sets_visible(
        &mut self,
        source_sets: &HashMap<(Uuid, i32), Vec<Uuid>>,
    ) -> StorageResult<HashMap<(Uuid, i32), bool>> {
        let mut sources: Vec<_> = source_sets.values().flatten().copied().collect();
        sources.sort_unstable();
        sources.dedup();
        let catalog = !sources.is_empty() && self.learning_catalog_present().await?;
        let mut source_kinds = HashMap::new();
        let mut source_assets = HashMap::new();
        for batch in sources.chunks(LEARNING_READ_BATCH) {
            let placeholders = vec!["?"; batch.len()].join(",");
            let sql = format!(
                "SELECT id,kind FROM oc_events WHERE tenant_id=? AND workspace_id=? \
                 AND id IN ({placeholders})"
            );
            let mut query = native_turso::query(&sql)
                .bind(self.scope.tenant_id)
                .bind(self.scope.workspace_id);
            for source in batch {
                query = query.bind(*source);
            }
            for row in query.fetch_all(&mut *self.tx).await.map_err(turso_err)? {
                source_kinds.insert(row.try_get::<Uuid>("id").map_err(turso_err)?, row);
            }
            if catalog {
                let sql = format!(
                    "SELECT source_event_id,asset_id FROM oc_learnings \
                     WHERE tenant_id=? AND workspace_id=? AND source_event_id IN ({placeholders})"
                );
                let mut query = native_turso::query(&sql)
                    .bind(self.scope.tenant_id)
                    .bind(self.scope.workspace_id);
                for source in batch {
                    query = query.bind(*source);
                }
                for row in query.fetch_all(&mut *self.tx).await.map_err(turso_err)? {
                    source_assets.insert(
                        row.try_get::<Uuid>("source_event_id").map_err(turso_err)?,
                        row,
                    );
                }
            }
        }
        let mut visible = HashMap::new();
        let mut checked = HashMap::new();
        for (version, sources) in source_sets {
            let mut roots = Vec::new();
            let mut orphaned = false;
            // Match learning_asset_for_source in each original source order:
            // decode kind, decode any catalog asset, then reject an orphan.
            // Later corrupt rows must not override an earlier NotFound.
            for source in sources {
                let kind = source_kinds
                    .get(source)
                    .map(|row| row.try_get::<String>("kind"))
                    .transpose()
                    .map_err(turso_err)?;
                let asset = source_assets
                    .get(source)
                    .map(|row| row.try_get::<Uuid>("asset_id"))
                    .transpose()
                    .map_err(turso_err)?;
                if kind.as_deref() == Some("learning") && asset.is_none() {
                    orphaned = true;
                    break;
                }
                if let Some(asset) = asset {
                    roots.push(asset);
                }
            }
            if orphaned {
                visible.insert(*version, false);
                continue;
            }
            roots.sort_unstable();
            roots.dedup();
            // The closure bound applies to the whole original support set, not
            // separately to each root. Cache only identical sets in this call.
            let allowed = if roots.is_empty() {
                true
            } else if let Some(allowed) = checked.get(&roots) {
                *allowed
            } else {
                let allowed = match self.guard_learning_dependencies(roots.clone(), None).await {
                    Ok(()) => true,
                    Err(StorageError::NotFound) => false,
                    Err(error) => return Err(error),
                };
                checked.insert(roots, allowed);
                allowed
            };
            visible.insert(*version, allowed);
        }
        Ok(visible)
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
        let kind: Option<String> = native_turso::query_scalar(
            "SELECT kind FROM oc_events WHERE tenant_id=? AND workspace_id=? AND id=?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(source)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        let asset = if self.learning_catalog_present().await? {
            native_turso::query_scalar("SELECT asset_id FROM oc_learnings WHERE tenant_id=? AND workspace_id=? AND source_event_id=?")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(source)
                .fetch_optional(&mut *self.tx).await.map_err(turso_err)?
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
            let active_asset: Option<Uuid> = native_turso::query_scalar(
                "SELECT id FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND id=? \
                 AND kind='knowledge' AND NOT deleted",
            )
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .bind(record.asset)
            .fetch_optional(&mut *self.tx)
            .await
            .map_err(turso_err)?;
            let active_source: Option<Uuid> = native_turso::query_scalar(
                "SELECT id FROM oc_events WHERE tenant_id=? AND workspace_id=? AND id=? \
                 AND kind='learning' AND state='active'",
            )
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .bind(record.source)
            .fetch_optional(&mut *self.tx)
            .await
            .map_err(turso_err)?;
            if active_asset.is_none() || active_source.is_none() {
                return Err(StorageError::NotFound);
            }
            // Fetch one extra row to detect truncation, never authorize a prefix.
            let supports: Vec<(Uuid, Uuid)> = native_turso::query_as("SELECT DISTINCT session_id,turn_id FROM oc_learning_support WHERE tenant_id=? AND workspace_id=? AND learning_id=? ORDER BY session_id,turn_id LIMIT 65")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(record.id)
                .fetch_all(&mut *self.tx).await.map_err(turso_err)?;
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
        let records: Vec<(Uuid, Uuid)> = native_turso::query_as("SELECT asset_id,source_event_id FROM oc_learnings WHERE tenant_id=? AND workspace_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id)
            .fetch_all(&mut *self.tx).await.map_err(turso_err)?;
        for (asset, source) in records {
            if !self.learning_asset_visible(asset).await? {
                native_turso::query("DELETE FROM oc_artifact_owners WHERE tenant_id=? AND workspace_id=? AND source_id=?")
                    .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(source)
                    .execute(&mut *self.tx).await.map_err(turso_err)?;
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
                return self
                    .guard_learning_dependencies(vec![asset], Some(asset))
                    .await;
            }
            self.require_learning_asset_visible(asset).await?;
        }
        if let Some(source) = source {
            self.require_learning_source_visible(source).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn support_kind_transaction(pool: &Pool, scope: Scope) -> TursoTx {
        TursoTx {
            tx: pool.begin().await.unwrap(),
            scope,
            principal_id: Uuid::nil(),
            required: Vec::new(),
            command: None,
        }
    }

    #[tokio::test]
    async fn learning_source_batches_keep_strict_kind_and_source_order() {
        let dir = tempfile::tempdir().unwrap();
        let pool = Pool::open(dir.path().join("support-kind.db")).await.unwrap();
        // The production TEXT column also permits BLOB storage. A catalog is
        // deliberately absent in the first pass, then present but empty.
        for sql in [
            "CREATE TABLE oc_events(tenant_id BLOB,workspace_id BLOB,id BLOB,kind TEXT NOT NULL,state TEXT)",
            "CREATE TABLE oc_assets(tenant_id BLOB,workspace_id BLOB,id BLOB,kind TEXT,title TEXT,current_version INTEGER,deleted INTEGER)",
            "CREATE TABLE oc_versions(tenant_id BLOB,workspace_id BLOB,asset_id BLOB,version INTEGER,title TEXT,source_event_id BLOB)",
            "CREATE TABLE oc_chunks(tenant_id BLOB,workspace_id BLOB,id BLOB,asset_id BLOB,version INTEGER,ordinal INTEGER,content TEXT,locator TEXT,search_terms TEXT)",
        ] {
            native_turso::query(sql).execute(&pool).await.unwrap();
        }
        let scope = Scope {
            tenant_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
        };
        let asset = Uuid::new_v4();
        let chunk = Uuid::new_v4();
        let root = Uuid::new_v4();
        let bad = Uuid::new_v4();
        let orphan = Uuid::new_v4();
        let missing = Uuid::new_v4();
        let bad_catalog = Uuid::new_v4();
        for (source, kind) in [(root, "memory"), (orphan, "learning")] {
            native_turso::query("INSERT INTO oc_events VALUES(?,?,?,?,'active')")
                .bind(scope.tenant_id)
                .bind(scope.workspace_id)
                .bind(source)
                .bind(kind)
                .execute(&pool)
                .await
                .unwrap();
        }
        native_turso::query("INSERT INTO oc_events VALUES(?,?,?,?,'active')")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(bad)
            .bind(b"memory".to_vec())
            .execute(&pool)
            .await
            .unwrap();
        native_turso::query("INSERT INTO oc_assets VALUES(?,?,?,'knowledge','fixture',1,0)")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(asset)
            .execute(&pool)
            .await
            .unwrap();
        // Historical asset classification retains its SQL kind filter and
        // must not start decoding this unrelated version's BLOB kind.
        for (version, source) in [(1, root), (2, bad)] {
            native_turso::query("INSERT INTO oc_versions VALUES(?,?,?,?,'fixture',?)")
                .bind(scope.tenant_id)
                .bind(scope.workspace_id)
                .bind(asset)
                .bind(version)
                .bind(source)
                .execute(&pool)
                .await
                .unwrap();
        }
        native_turso::query("INSERT INTO oc_chunks VALUES(?,?,?,?,1,0,'fixture',?,'fixture')")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(chunk)
            .bind(asset)
            .bind(json!({"parser":"memory-semantics:v1","support_sources":[bad]}).to_string())
            .execute(&pool)
            .await
            .unwrap();
        let version = (asset, 1);
        for catalog in [false, true] {
            if catalog {
                native_turso::query("CREATE TABLE oc_learnings(tenant_id BLOB,workspace_id BLOB,source_event_id BLOB,asset_id BLOB)")
                    .execute(&pool).await.unwrap();
                // A missing event still requires the catalog UUID decode.
                // This deliberately malformed asset blob tests stored damage.
                native_turso::query("INSERT INTO oc_learnings VALUES(?,?,?,?)")
                    .bind(scope.tenant_id)
                    .bind(scope.workspace_id)
                    .bind(bad_catalog)
                    .bind(vec![1_u8])
                    .execute(&pool)
                    .await
                    .unwrap();
            }
            let mut tx = support_kind_transaction(&pool, scope).await;
            assert!(tx.local_support_visibility(&[version]).await.unwrap()[&version]);
            assert!(!tx.has_learning_source_version(asset).await.unwrap());
            assert!(tx.learning_assets_visible(&[asset]).await.unwrap()[&asset]);
            // Exercise the retrieval path, with a valid ordinary current root
            // and a distinct active semantic support whose kind is not text.
            assert!(matches!(
                tx.visible_hits(&[chunk]).await,
                Err(StorageError::Backend(_))
            ));
            tx.tx.rollback().await.unwrap();

            let mut cases = vec![
                (vec![], Some(true)),
                (vec![root, root], Some(true)),
                (vec![missing], Some(true)),
                (vec![orphan], Some(false)),
                (vec![orphan, bad], Some(false)),
                (vec![bad, orphan], None),
                (vec![bad], None),
            ];
            if catalog {
                cases.extend([
                    (vec![orphan, bad_catalog], Some(false)),
                    (vec![bad_catalog, orphan], None),
                    (vec![bad_catalog], None),
                ]);
            }
            for (sources, expected) in cases {
                let mut tx = support_kind_transaction(&pool, scope).await;
                let original = tx.require_learning_sources_visible(&sources).await;
                match expected {
                    Some(true) => assert!(original.is_ok()),
                    Some(false) => assert!(matches!(original, Err(StorageError::NotFound))),
                    None => assert!(matches!(original, Err(StorageError::Backend(_)))),
                }
                // The original typed query's decode error leaves a lease that
                // must be rolled back before the independent batch comparison.
                tx.tx.rollback().await.unwrap();

                let mut tx = support_kind_transaction(&pool, scope).await;
                let result = tx
                    .learning_source_sets_visible(&HashMap::from([(version, sources)]))
                    .await;
                match expected {
                    Some(visible) => assert_eq!(result.unwrap()[&version], visible),
                    None => assert!(matches!(result, Err(StorageError::Backend(_)))),
                }
                tx.tx.rollback().await.unwrap();
            }
        }
        pool.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn learning_batches_preserve_all_history_retraction_and_scope() {
        let dir = tempfile::tempdir().unwrap();
        let store = TursoStore::open(dir.path().join("learning-batches.db"))
            .await
            .unwrap();
        let tenant = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        let other_workspace = Uuid::new_v4();
        for id in [workspace, other_workspace] {
            store.create_workspace(tenant, id, "fixture").await.unwrap();
        }
        let key = store
            .issue_key(
                Scope {
                    tenant_id: tenant,
                    workspace_id: workspace,
                },
                "admin",
            )
            .await
            .unwrap();
        let auth = store.authenticate(&key.token).await.unwrap();
        let principal = auth.principal_id;
        let mut tx = store.begin(auth).await.unwrap();
        let ordinary_source = Uuid::new_v4();
        let learning_source = Uuid::new_v4();
        for (scope, source, kind, state) in [
            (workspace, ordinary_source, "memory", "active"),
            (workspace, learning_source, "learning", "retracted"),
            (other_workspace, ordinary_source, "learning", "active"),
        ] {
            native_turso::query("INSERT INTO oc_events(tenant_id,workspace_id,id,content,kind,state,created_by,created_at) VALUES(?,?,?,'fixture',?,?,?,0)")
                .bind(tenant).bind(scope).bind(source).bind(kind).bind(state).bind(principal)
                .execute(&mut *tx.tx).await.unwrap();
        }
        // More than one ID batch, with many versions sharing one source ID.
        let mut assets: Vec<_> = (1..=258).map(Uuid::from_u128).collect();
        for asset in &assets {
            native_turso::query("INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,created_at) VALUES(?,?,?,'knowledge','fixture',0)")
                .bind(tenant).bind(workspace).bind(*asset)
                .execute(&mut *tx.tx).await.unwrap();
            native_turso::query("INSERT INTO oc_versions(tenant_id,workspace_id,asset_id,version,content,content_hash,source_event_id,title,created_by,created_at) VALUES(?,?,?,1,'fixture','hash',?,'fixture',?,0)")
                .bind(tenant).bind(workspace).bind(*asset).bind(ordinary_source).bind(principal)
                .execute(&mut *tx.tx).await.unwrap();
        }
        let orphan = assets[0];
        for version in 2..=257 {
            native_turso::query("INSERT INTO oc_versions(tenant_id,workspace_id,asset_id,version,content,content_hash,source_event_id,title,created_by,created_at) VALUES(?,?,?,?,'fixture','hash',?,'fixture',?,0)")
                .bind(tenant).bind(workspace).bind(orphan).bind(version).bind(ordinary_source).bind(principal)
                .execute(&mut *tx.tx).await.unwrap();
        }
        // The schema permits a version beyond i32; it must not make this
        // identity-only read fail or hide a later Learning source.
        native_turso::query("INSERT INTO oc_versions(tenant_id,workspace_id,asset_id,version,content,content_hash,source_event_id,title,created_by,created_at) VALUES(?,?,?,?,'fixture','hash',?,'fixture',?,0)")
            .bind(tenant).bind(workspace).bind(orphan).bind(i64::from(i32::MAX) + 1)
            .bind(learning_source).bind(principal).execute(&mut *tx.tx).await.unwrap();
        assets.push(orphan); // Duplicate inputs must not multiply domain checks.
        assert!(tx.has_learning_source_version(orphan).await.unwrap());
        assert!(!tx.has_learning_source_version(assets[1]).await.unwrap());
        let visible = tx.learning_assets_visible(&assets).await.unwrap();
        assert_eq!(visible.len(), 258);
        for asset in &assets {
            assert_eq!(visible[asset], *asset != orphan);
        }
        let source_sets = HashMap::from([
            (
                (orphan, 1),
                vec![ordinary_source, learning_source, ordinary_source],
            ),
            ((assets[1], 1), vec![ordinary_source]),
        ]);
        let sources = tx.learning_source_sets_visible(&source_sets).await.unwrap();
        assert_eq!(sources.len(), 2);
        assert!(sources[&(assets[1], 1)]);
        assert!(!sources[&(orphan, 1)]);
        // Results are scoped to this read call, never cached across writes.
        native_turso::query(
            "UPDATE oc_events SET kind='memory' WHERE tenant_id=? AND workspace_id=? AND id=?",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(learning_source)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
        assert!(tx.learning_assets_visible(&[orphan]).await.unwrap()[&orphan]);
        assert!(tx.learning_source_sets_visible(&source_sets).await.unwrap()[&(orphan, 1)]);
        tx.rollback().await.unwrap();
        store.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn learning_source_sets_keep_the_combined_closure_limit() {
        let dir = tempfile::tempdir().unwrap();
        let store = TursoStore::open(dir.path().join("learning-closure.db"))
            .await
            .unwrap();
        let scope = Scope {
            tenant_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
        };
        store
            .create_workspace(scope.tenant_id, scope.workspace_id, "fixture")
            .await
            .unwrap();
        let key = store.issue_key(scope, "admin").await.unwrap();
        let auth = store.authenticate(&key.token).await.unwrap();
        let principal = auth.principal_id;
        let mut tx = store.begin(auth).await.unwrap();
        let session = Uuid::new_v4();
        let turn = Uuid::new_v4();
        let feedback = Uuid::new_v4();
        let turn_source = tx
            .create_event("session_turn", "fixture", None)
            .await
            .unwrap();
        native_turso::query("INSERT INTO oc_session_turns(tenant_id,workspace_id,session_id,turn_id,source_event_id,request_hash,created_by,created_at) VALUES(?,?,?,?,?,'fixture',?,0)")
            .bind(scope.tenant_id).bind(scope.workspace_id).bind(session).bind(turn)
            .bind(turn_source).bind(principal).execute(&mut *tx.tx).await.unwrap();
        native_turso::query("INSERT INTO oc_feedback(tenant_id,workspace_id,principal_id,feedback_id,session_id,turn_id,request_hash,feedback,created_at,feedback_sequence) VALUES(?,?,?,?,?,?,'fixture','{}',0,1)")
            .bind(scope.tenant_id).bind(scope.workspace_id).bind(principal).bind(feedback)
            .bind(session).bind(turn).execute(&mut *tx.tx).await.unwrap();
        native_turso::query(
            "INSERT INTO oc_improve_cursors(tenant_id,workspace_id,session_id) VALUES(?,?,?)",
        )
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(session)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
        let mut sources = Vec::new();
        let mut improve = Uuid::nil();
        for index in 0..64 {
            let job = Uuid::new_v4();
            native_turso::query("INSERT INTO oc_jobs(tenant_id,workspace_id,id,created_by,operation,payload,created_at,updated_at) VALUES(?,?,?,?,'fixture','{}',0,0)")
                .bind(scope.tenant_id).bind(scope.workspace_id).bind(job).bind(principal)
                .execute(&mut *tx.tx).await.unwrap();
            if index % 8 == 0 {
                improve = Uuid::new_v4();
                native_turso::query("INSERT INTO oc_improve_runs(tenant_id,workspace_id,improve_id,session_id,job_id,request_hash,request,snapshot,lower_sequence,target_sequence,status,created_by,created_at) VALUES(?,?,?,?,?,'fixture','{}','{}',0,1,'completed',?,0)")
                    .bind(scope.tenant_id).bind(scope.workspace_id).bind(improve)
                    .bind(session).bind(job).bind(principal).execute(&mut *tx.tx).await.unwrap();
            }
            let asset = Uuid::new_v4();
            let learning = Uuid::new_v4();
            let source = tx.create_event("learning", "fixture", None).await.unwrap();
            sources.push(source);
            native_turso::query("INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,created_at) VALUES(?,?,?,'knowledge','fixture',0)")
                .bind(scope.tenant_id).bind(scope.workspace_id).bind(asset)
                .execute(&mut *tx.tx).await.unwrap();
            native_turso::query("INSERT INTO oc_learnings(tenant_id,workspace_id,id,improve_id,session_id,candidate_index,asset_id,source_event_id,ingest_job_id,candidate,fingerprint,status,created_by,created_at) VALUES(?,?,?,?,?,?,?,?,?,'{}','fixture','published',?,0)")
                .bind(scope.tenant_id).bind(scope.workspace_id).bind(learning).bind(improve)
                .bind(session).bind(index % 8).bind(asset).bind(source).bind(job)
                .bind(principal).execute(&mut *tx.tx).await.unwrap();
            native_turso::query("INSERT INTO oc_learning_support(tenant_id,workspace_id,learning_id,session_id,turn_id,support_ordinal,principal_id,feedback_id) VALUES(?,?,?,?,?,0,?,?)")
                .bind(scope.tenant_id).bind(scope.workspace_id).bind(learning)
                .bind(session).bind(turn).bind(principal).bind(feedback)
                .execute(&mut *tx.tx).await.unwrap();
        }
        let asset = Uuid::new_v4();
        let sets = HashMap::from([
            ((asset, 1), sources[..32].to_vec()),
            ((asset, 2), sources[32..].to_vec()),
            ((asset, 3), sources.clone()),
            ((asset, 4), sources[..63].to_vec()),
        ]);
        let visible = tx.learning_source_sets_visible(&sets).await.unwrap();
        assert!(visible[&(asset, 1)]);
        assert!(visible[&(asset, 2)]);
        assert!(!visible[&(asset, 3)]); // 64 Learning nodes plus one shared turn.
        assert!(visible[&(asset, 4)]); // 63 Learning nodes plus one shared turn.
        tx.rollback().await.unwrap();
        store.shutdown().await.unwrap();
    }
}
