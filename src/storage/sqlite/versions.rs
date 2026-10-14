//! Assets, immutable versions, chunks and publication projections.
use super::read_model::{self, AssociationKey};
use super::*;
use std::collections::HashSet;

const DELETED_ASSETS: &str = "SELECT id,(id=?) AS m0,(id=?) AS m1,(id=?) AS m2,(id=?) AS m3,(id=?) AS m4,(id=?) AS m5,(id=?) AS m6,(id=?) AS m7 FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND deleted AND ((id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?))";

const HISTORICAL_ROOTS: &str = "SELECT source_event_id,(asset_id=? AND version=?) AS m0,(asset_id=? AND version=?) AS m1,(asset_id=? AND version=?) AS m2,(asset_id=? AND version=?) AS m3,(asset_id=? AND version=?) AS m4,(asset_id=? AND version=?) AS m5,(asset_id=? AND version=?) AS m6,(asset_id=? AND version=?) AS m7 FROM oc_versions WHERE tenant_id=? AND workspace_id=? AND ((asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?))";

const RETRACTED_EVENTS: &str = "SELECT id,(id=?) AS m0,(id=?) AS m1,(id=?) AS m2,(id=?) AS m3,(id=?) AS m4,(id=?) AS m5,(id=?) AS m6,(id=?) AS m7 FROM oc_events WHERE tenant_id=? AND workspace_id=? AND state='retracted' AND ((id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?))";

impl TursoTx {
    pub(super) async fn insert_knowledge_asset_inner(
        &mut self,
        title: &str,
    ) -> StorageResult<Uuid> {
        let id = Uuid::new_v4();
        native_turso::query(
            "INSERT INTO oc_assets(tenant_id, workspace_id, id, kind, title, created_at) \
             VALUES (?, ?, ?, 'knowledge', ?, ?)",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(id)
        .bind(title)
        .bind(now_ms())
        .execute(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        Ok(id)
    }

    pub(super) async fn restore_source_inner(
        &mut self,
        asset: Uuid,
        version: i32,
    ) -> StorageResult<Value> {
        let asset_row = native_turso::query(
            "SELECT current_version FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND id=? AND NOT deleted",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(asset)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        let requests = vec![vec![
            AssociationKey::from_uuid(asset),
            AssociationKey::from_optional_i32(Some(version)),
        ]];
        let roots = read_model::live_root_rows(&mut self.tx, self.scope, &requests).await?;
        let title_row = native_turso::query(
            "SELECT title FROM oc_versions WHERE tenant_id=? AND workspace_id=? AND asset_id=? AND version=?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(asset)
        .bind(version)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?
        .ok_or(StorageError::NotFound)?;
        let asset_row = asset_row.ok_or(StorageError::NotFound)?;
        let root = roots.first().ok_or(StorageError::NotFound)?;
        // Original tuple order, only after native undeleted/active filtering.
        let current_version: Option<i32> =
            asset_row.try_get("current_version").map_err(turso_err)?;
        let source_event_id: Uuid = root.1.try_get("source_event_id").map_err(turso_err)?;
        let title: String = title_row.try_get("title").map_err(turso_err)?;
        Ok(json!({
            "current_version": current_version,
            "source_event_id": source_event_id,
            "title": title,
        }))
    }

    pub(super) async fn delete_asset_inner(&mut self, id: Uuid) -> StorageResult<u64> {
        native_turso::query(
            "UPDATE oc_assets SET deleted = 1, current_version = NULL WHERE tenant_id = ? AND workspace_id = ? AND id = ?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(id)
        .execute(&mut *self.tx)
        .await
        .map_err(turso_err)
        .map(|r| r.rows_affected())
    }

    pub(super) async fn asset_view_inner(
        &mut self,
        id: Uuid,
        version: Option<i32>,
    ) -> StorageResult<Value> {
        let asset = native_turso::query(
            "SELECT id,kind,COALESCE(?,current_version) AS requested_version FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND id=? AND NOT deleted",
        )
        .bind(version)
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(id)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        let requested = asset
            .as_ref()
            .map(|row| {
                row.try_get::<AssociationKey>("requested_version")
                    .map_err(turso_err)
            })
            .transpose()?
            .unwrap_or_else(|| AssociationKey::from_optional_i32(None));
        let requests = vec![vec![AssociationKey::from_uuid(id), requested.clone()]];
        let roots = read_model::live_root_rows(&mut self.tx, self.scope, &requests).await?;
        let row = native_turso::query(
            "SELECT title,version,content,restored_from,content_hash FROM oc_versions WHERE tenant_id=? AND workspace_id=? AND asset_id=? AND version=?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(id)
        .bind(requested)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?
        .ok_or(StorageError::NotFound)?;
        let asset = asset.ok_or(StorageError::NotFound)?;
        let root = roots.first().ok_or(StorageError::NotFound)?;
        // Explicit history never decodes current_version. Preserve the old
        // selected tuple's field order after all native root predicates.
        let asset_id: Uuid = asset.try_get("id").map_err(turso_err)?;
        let kind: String = asset.try_get("kind").map_err(turso_err)?;
        let title: String = row.try_get("title").map_err(turso_err)?;
        let v: i32 = row.try_get("version").map_err(turso_err)?;
        let content: String = row.try_get("content").map_err(turso_err)?;
        let source_event_id: Uuid = root.1.try_get("source_event_id").map_err(turso_err)?;
        let restored_from: Option<i32> = row.try_get("restored_from").map_err(turso_err)?;
        let content_hash: String = row.try_get("content_hash").map_err(turso_err)?;
        self.require_learning_asset_visible(asset_id).await?;
        let semantic_status = self.semantic_version_status(asset_id, v).await?;
        let semantic_chunks = if semantic_status.is_some() {
            if !self.semantic_version_visible(asset_id, v).await? {
                return Err(StorageError::NotFound);
            }
            self.chunks_for_version(asset_id, v).await?
        } else {
            Vec::new()
        };
        let identity = self.identity_for_asset(asset_id).await?;
        let normalization_status = if kind != "memory" {
            "not_applicable"
        } else if semantic_status.as_deref().is_some_and(|s| s != "confirmed") {
            "unnormalized"
        } else if identity.is_some() {
            "explicit_identity"
        } else {
            "legacy_unidentified"
        };
        let mut value = json!({
            "asset_id": asset_id,
            "kind": kind,
            "title": title,
            "version": v,
            "content": content,
            "source_event_id": source_event_id,
            "restored_from": restored_from,
            "content_hash": content_hash,
            "identity": identity,
            "normalization_status": normalization_status,
        });
        if let Some(status) = semantic_status {
            value["semantic_status"] = json!(status);
            value["semantic_values"] = json!(semantic_chunks.iter().map(|(_, l)| json!({"value":l["semantic_value"],"source_event_id":l["source_event_id"],"source_version":l["source_version"],"locator":l})).collect::<Vec<_>>());
        }
        Ok(value)
    }

    pub(super) async fn chunks_for_version_inner(
        &mut self,
        asset: Uuid,
        version: i32,
    ) -> StorageResult<Vec<(String, Value)>> {
        let rows: Vec<(String, String)> = native_turso::query_as(
            "SELECT content, locator FROM oc_chunks \
             WHERE tenant_id = ? AND workspace_id = ? AND asset_id = ? AND version = ? ORDER BY ordinal",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(asset)
        .bind(version)
        .fetch_all(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        Ok(rows
            .into_iter()
            .map(|(content, locator)| {
                (
                    content,
                    serde_json::from_str(&locator).unwrap_or(Value::Null),
                )
            })
            .collect())
    }

    pub(super) async fn asset_current_version_inner(
        &mut self,
        asset: Uuid,
    ) -> StorageResult<Option<i32>> {
        native_turso::query_scalar::< Option<i32>>(
            "SELECT current_version FROM oc_assets WHERE tenant_id = ? AND workspace_id = ? AND id = ?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(asset)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)
        .map(Option::flatten)
    }

    pub(super) async fn insert_version_inner(
        &mut self,
        asset: Uuid,
        version: i32,
        content: &str,
        content_hash: &str,
        source: Uuid,
        restored_from: Option<i32>,
        title: Option<&str>,
    ) -> StorageResult<()> {
        // Even an explicit title prepares the original asset-table reference,
        // but only a missing title reads its value. Keep the native value so
        // COALESCE, column affinity and INSERT constraints still decide it.
        let inherited: Option<AssociationKey> = native_turso::query_scalar(
            "SELECT title FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND id=? AND ? IS NULL",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(asset)
        .bind(title)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        native_turso::query(
            "INSERT INTO oc_versions(tenant_id, workspace_id, asset_id, version, content, content_hash, source_event_id, restored_from, title, created_by, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, COALESCE(?, ?), ?, ?)",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(asset)
        .bind(version)
        .bind(content)
        .bind(content_hash)
        .bind(source)
        .bind(restored_from)
        .bind(title)
        .bind(inherited)
        .bind(self.principal_id)
        .bind(now_ms())
        .execute(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        Ok(())
    }

    pub(super) async fn insert_chunk_inner(
        &mut self,
        chunk: Uuid,
        asset: Uuid,
        version: i32,
        ordinal: i32,
        content: &str,
        locator: &Value,
        search_terms: &str,
    ) -> StorageResult<()> {
        native_turso::query(
            "INSERT INTO oc_chunks(tenant_id, workspace_id, id, asset_id, version, ordinal, content, locator, search_terms) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(chunk)
        .bind(asset)
        .bind(version)
        .bind(ordinal)
        .bind(content)
        .bind(serde_json::to_string(locator).unwrap_or_default())
        .bind(search_terms)
        .execute(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        Ok(())
    }

    pub(super) async fn insert_summary_inner(
        &mut self,
        summary: Uuid,
        chunk: Uuid,
        text: &str,
        model_revision: &str,
        search_terms: &str,
    ) -> StorageResult<()> {
        native_turso::query(
            "INSERT INTO oc_summaries(tenant_id, workspace_id, id, chunk_id, text, model_revision, search_terms) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(summary)
        .bind(chunk)
        .bind(text)
        .bind(model_revision)
        .bind(search_terms)
        .execute(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        Ok(())
    }

    pub(super) async fn update_asset_version_inner(
        &mut self,
        asset: Uuid,
        version: i32,
        title: Option<&str>,
    ) -> StorageResult<()> {
        native_turso::query(
            "UPDATE oc_assets SET current_version = ?, title = COALESCE(?, title) WHERE tenant_id = ? AND workspace_id = ? AND id = ?",
        )
        .bind(version)
        .bind(title)
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(asset)
        .execute(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        Ok(())
    }

    pub(super) async fn cleanup_derived_chunks_inner(&mut self) -> StorageResult<u64> {
        let chunks = native_turso::query(
            "SELECT rowid AS cleanup_rowid,asset_id,version FROM oc_chunks WHERE tenant_id=? AND workspace_id=?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .fetch_all(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        let asset_keys = chunks
            .iter()
            .map(|row| {
                Ok(vec![
                    row.try_get::<AssociationKey>("asset_id")
                        .map_err(turso_err)?,
                ])
            })
            .collect::<StorageResult<Vec<_>>>()?;
        let version_keys = chunks
            .iter()
            .map(|row| {
                Ok(vec![
                    row.try_get::<AssociationKey>("asset_id")
                        .map_err(turso_err)?,
                    row.try_get::<AssociationKey>("version")
                        .map_err(turso_err)?,
                ])
            })
            .collect::<StorageResult<Vec<_>>>()?;
        let mut removed: HashSet<_> =
            read_model::associate_query(&mut self.tx, self.scope, DELETED_ASSETS, 1, &asset_keys)
                .await?
                .into_iter()
                .map(|(index, _)| index)
                .collect();
        let versions = read_model::associate_query(
            &mut self.tx,
            self.scope,
            HISTORICAL_ROOTS,
            2,
            &version_keys,
        )
        .await?;
        let source_keys = versions
            .iter()
            .map(|(_, row)| {
                Ok(vec![
                    row.try_get::<AssociationKey>("source_event_id")
                        .map_err(turso_err)?,
                ])
            })
            .collect::<StorageResult<Vec<_>>>()?;
        let events = read_model::associate_query(
            &mut self.tx,
            self.scope,
            RETRACTED_EVENTS,
            1,
            &source_keys,
        )
        .await?;
        removed.extend(events.into_iter().map(|(index, _)| versions[index].0));
        let rowids = chunks
            .iter()
            .enumerate()
            .filter(|(index, _)| removed.contains(index))
            .map(|(_, row)| row.try_get::<i64>("cleanup_rowid").map_err(turso_err))
            .collect::<StorageResult<Vec<_>>>()?;
        // These rowids are internal locators in this scoped write snapshot,
        // never domain IDs or persisted keys. Count actual mutations, not hits.
        let mut changed = 0;
        for index in 0..rowids.len().div_ceil(8).max(1) {
            let start = index * 8;
            let batch = &rowids[start..(start + 8).min(rowids.len())];
            let mut query = native_turso::query(
                "DELETE FROM oc_chunks WHERE tenant_id=? AND workspace_id=? AND rowid IN (?,?,?,?,?,?,?,?)",
            )
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id);
            for slot in 0..8 {
                query = query.bind(batch.get(slot).copied());
            }
            changed += query
                .execute(&mut *self.tx)
                .await
                .map_err(turso_err)?
                .rows_affected();
        }
        Ok(changed)
    }
}
