//! Assets, immutable versions, chunks and publication projections.
use super::visibility::ACTIVE_ASSET_VERSION;
use super::*;

impl SqliteTx {
    pub(super) async fn insert_knowledge_asset_inner(
        &mut self,
        title: &str,
    ) -> StorageResult<Uuid> {
        let id = Uuid::new_v4();
        sqlx::query(
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
        .map_err(sqlite_err)?;
        Ok(id)
    }

    pub(super) async fn restore_source_inner(
        &mut self,
        asset: Uuid,
        version: i32,
    ) -> StorageResult<Value> {
        let row: Option<(Option<i32>, Uuid, String)> = sqlx::query_as(
            "SELECT a.current_version, v.source_event_id, v.title \
             FROM oc_assets a \
             JOIN oc_versions v ON v.asset_id = a.id AND v.tenant_id = a.tenant_id AND v.workspace_id = a.workspace_id \
             JOIN oc_events e ON e.id = v.source_event_id AND e.tenant_id = v.tenant_id AND e.workspace_id = v.workspace_id \
             WHERE a.id = ? AND a.tenant_id = ? AND a.workspace_id = ? AND v.version = ? AND NOT a.deleted AND e.state = 'active'",
        )
        .bind(asset)
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(version)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        let Some((current_version, source_event_id, title)) = row else {
            return Err(StorageError::NotFound);
        };
        Ok(json!({
            "current_version": current_version,
            "source_event_id": source_event_id,
            "title": title,
        }))
    }

    pub(super) async fn delete_asset_inner(&mut self, id: Uuid) -> StorageResult<u64> {
        sqlx::query(
            "UPDATE oc_assets SET deleted = 1, current_version = NULL WHERE tenant_id = ? AND workspace_id = ? AND id = ?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(id)
        .execute(&mut *self.tx)
        .await
        .map_err(sqlite_err)
        .map(|r| r.rows_affected())
    }

    pub(super) async fn asset_view_inner(
        &mut self,
        id: Uuid,
        version: Option<i32>,
    ) -> StorageResult<Value> {
        let row: Option<(Uuid, String, String, i32, String, Uuid, Option<i32>, String)> = sqlx::query_as(
            &format!("SELECT a.id, a.kind, v.title, v.version, v.content, v.source_event_id, v.restored_from, v.content_hash {ACTIVE_ASSET_VERSION}"),
        )
        .bind(version)
        .bind(id)
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        let Some((asset_id, kind, title, v, content, source_event_id, restored_from, content_hash)) =
            row
        else {
            return Err(StorageError::NotFound);
        };
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
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT content, locator FROM oc_chunks \
             WHERE tenant_id = ? AND workspace_id = ? AND asset_id = ? AND version = ? ORDER BY ordinal",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(asset)
        .bind(version)
        .fetch_all(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
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
        sqlx::query_scalar::<_, Option<i32>>(
            "SELECT current_version FROM oc_assets WHERE tenant_id = ? AND workspace_id = ? AND id = ?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(asset)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(sqlite_err)
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
        sqlx::query(
            "INSERT INTO oc_versions(tenant_id, workspace_id, asset_id, version, content, content_hash, source_event_id, restored_from, title, created_by, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, COALESCE(?, (SELECT title FROM oc_assets WHERE tenant_id = ? AND workspace_id = ? AND id = ?)), ?, ?)",
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
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(asset)
        .bind(self.principal_id)
        .bind(now_ms())
        .execute(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
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
        sqlx::query(
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
        .map_err(sqlite_err)?;
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
        sqlx::query(
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
        .map_err(sqlite_err)?;
        Ok(())
    }

    pub(super) async fn update_asset_version_inner(
        &mut self,
        asset: Uuid,
        version: i32,
        title: Option<&str>,
    ) -> StorageResult<()> {
        sqlx::query(
            "UPDATE oc_assets SET current_version = ?, title = COALESCE(?, title) WHERE tenant_id = ? AND workspace_id = ? AND id = ?",
        )
        .bind(version)
        .bind(title)
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(asset)
        .execute(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        Ok(())
    }

    pub(super) async fn cleanup_derived_chunks_inner(&mut self) -> StorageResult<u64> {
        sqlx::query(
            "DELETE FROM oc_chunks WHERE tenant_id = ? AND workspace_id = ? \
               AND (EXISTS(SELECT 1 FROM oc_assets a WHERE a.id = oc_chunks.asset_id AND a.tenant_id = oc_chunks.tenant_id AND a.workspace_id = oc_chunks.workspace_id AND a.deleted) \
                 OR EXISTS(SELECT 1 FROM oc_versions v JOIN oc_events e ON e.id = v.source_event_id AND e.tenant_id = v.tenant_id AND e.workspace_id = v.workspace_id WHERE v.asset_id = oc_chunks.asset_id AND v.version = oc_chunks.version AND v.tenant_id = oc_chunks.tenant_id AND v.workspace_id = oc_chunks.workspace_id AND e.state = 'retracted'))",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .execute(&mut *self.tx)
        .await
        .map_err(sqlite_err)
        .map(|r| r.rows_affected())
    }
}
