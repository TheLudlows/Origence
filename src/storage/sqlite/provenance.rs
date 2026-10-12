//! Source events/files, withdrawal and artifact ownership.
use super::*;

impl SqliteTx {
    pub(super) async fn create_event_inner(
        &mut self,
        kind: &str,
        content: &str,
        file: Option<Uuid>,
    ) -> StorageResult<Uuid> {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO oc_events(tenant_id, workspace_id, id, content, kind, file_id, state, created_by, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, 'active', ?, ?)",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(id)
        .bind(content)
        .bind(kind)
        .bind(file)
        .bind(self.principal_id)
        .bind(now_ms())
        .execute(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        Ok(id)
    }

    pub(super) async fn insert_file_inner(
        &mut self,
        name: &str,
        media_type: &str,
        hash: &str,
        size: i64,
    ) -> StorageResult<Uuid> {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO oc_files(tenant_id, workspace_id, id, name, media_type, hash, size, deleted, created_by, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(id)
        .bind(name)
        .bind(media_type)
        .bind(hash)
        .bind(size)
        .bind(self.principal_id)
        .bind(now_ms())
        .execute(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        Ok(id)
    }

    pub(super) async fn file_exists_inner(&mut self, id: Uuid) -> StorageResult<bool> {
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM oc_files WHERE tenant_id = ? AND workspace_id = ? AND id = ? AND NOT deleted)",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(id)
        .fetch_one(&mut *self.tx)
        .await
        .map_err(sqlite_err)
    }

    pub(super) async fn file_hash_inner(&mut self, id: Uuid) -> StorageResult<Option<String>> {
        sqlx::query_scalar(
            "SELECT hash FROM oc_files WHERE tenant_id = ? AND workspace_id = ? AND id = ? AND NOT deleted",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(id)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(sqlite_err)
    }

    pub(super) async fn file_visible_inner(&mut self, id: Uuid) -> StorageResult<bool> {
        self.file_exists(id).await
    }

    pub(super) async fn retract_event_inner(&mut self, id: Uuid) -> StorageResult<u64> {
        sqlx::query(
            "UPDATE oc_events SET state = 'retracted' WHERE tenant_id = ? AND workspace_id = ? AND id = ?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(id)
        .execute(&mut *self.tx)
        .await
        .map_err(sqlite_err)
        .map(|r| r.rows_affected())
    }

    pub(super) async fn delete_file_inner(&mut self, id: Uuid) -> StorageResult<u64> {
        let count = sqlx::query(
            "UPDATE oc_files SET deleted = 1 WHERE tenant_id = ? AND workspace_id = ? AND id = ?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(id)
        .execute(&mut *self.tx)
        .await
        .map_err(sqlite_err)?
        .rows_affected();
        sqlx::query(
            "UPDATE oc_events SET state = 'retracted' WHERE tenant_id = ? AND workspace_id = ? AND file_id = ?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(id)
        .execute(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        Ok(count)
    }

    pub(super) async fn valid_targets_inner(
        &mut self,
        asset: Option<Uuid>,
        source: Option<Uuid>,
    ) -> StorageResult<()> {
        if let Some(id) = asset {
            let valid: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM oc_assets WHERE tenant_id = ? AND workspace_id = ? AND id = ? AND NOT deleted)",
            )
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .bind(id)
            .fetch_one(&mut *self.tx)
            .await
            .map_err(sqlite_err)?;
            if !valid {
                return Err(StorageError::Conflict(
                    "asset is deleted or missing".to_string(),
                ));
            }
        }
        if let Some(id) = source {
            let valid: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM oc_events WHERE tenant_id = ? AND workspace_id = ? AND id = ? AND state = 'active')",
            )
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .bind(id)
            .fetch_one(&mut *self.tx)
            .await
            .map_err(sqlite_err)?;
            if !valid {
                return Err(StorageError::Conflict(
                    "source is retracted or missing".to_string(),
                ));
            }
        }
        Ok(())
    }

    pub(super) async fn event_view_inner(&mut self, id: Uuid) -> StorageResult<Value> {
        let row: Option<(Uuid, String, String, Option<Uuid>, i64)> = sqlx::query_as(
            "SELECT e.id,e.kind,e.content,e.file_id,e.created_at FROM oc_events e \
             LEFT JOIN oc_files f ON f.tenant_id=e.tenant_id AND f.workspace_id=e.workspace_id AND f.id=e.file_id \
             WHERE e.tenant_id=? AND e.workspace_id=? AND e.id=? AND e.state='active' \
             AND (e.file_id IS NULL OR f.deleted=0)",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(id)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        let Some((event_id, kind, content, file_id, created_at)) = row else {
            return Err(StorageError::NotFound);
        };
        self.require_learning_source_visible(id).await?;
        if kind == "session_turn" {
            self.require_session_source_visible(id).await?;
        }
        Ok(
            json!({"event_id":event_id,"kind":kind,"content":content,"file_id":file_id,"created_at":created_at}),
        )
    }

    pub(super) async fn event_content_inner(
        &mut self,
        source: Uuid,
        require_active: bool,
    ) -> StorageResult<Option<String>> {
        let sql = if require_active {
            "SELECT content FROM oc_events WHERE tenant_id = ? AND workspace_id = ? AND id = ? AND state = 'active'"
        } else {
            "SELECT content FROM oc_events WHERE tenant_id = ? AND workspace_id = ? AND id = ?"
        };
        sqlx::query_scalar(sql)
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .bind(source)
            .fetch_optional(&mut *self.tx)
            .await
            .map_err(sqlite_err)
    }

    pub(super) async fn event_file_inner(&mut self, source: Uuid) -> StorageResult<Value> {
        let row: Option<(String, Option<Uuid>, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT e.content, e.file_id, f.hash, f.media_type \
             FROM oc_events e \
             LEFT JOIN oc_files f ON f.id = e.file_id AND f.tenant_id = e.tenant_id AND f.workspace_id = e.workspace_id AND NOT f.deleted \
             WHERE e.id = ? AND e.tenant_id = ? AND e.workspace_id = ?",
        )
        .bind(source)
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        let Some((content, file_id, hash, media_type)) = row else {
            return Err(StorageError::NotFound);
        };
        Ok(json!({
            "content": content,
            "file_id": file_id,
            "hash": hash,
            "media_type": media_type,
        }))
    }

    pub(super) async fn register_owner_inner(
        &mut self,
        source: SourceVersion,
        artifact_type: &str,
        artifact_id: Uuid,
        chunk_id: Option<Uuid>,
    ) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO oc_artifact_owners(tenant_id, workspace_id, source_id, version, \
             artifact_type, artifact_id, chunk_id, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT DO NOTHING",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(source.source_id)
        .bind(source.version)
        .bind(artifact_type)
        .bind(artifact_id)
        .bind(chunk_id)
        .bind(now_ms())
        .execute(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        Ok(())
    }

    pub(super) async fn detach_owners_inner(&mut self, source: SourceVersion) -> StorageResult<u64> {
        let res = sqlx::query(
            "DELETE FROM oc_artifact_owners \
             WHERE tenant_id = ? AND workspace_id = ? AND source_id = ? AND version = ?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(source.source_id)
        .bind(source.version)
        .execute(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        Ok(res.rows_affected())
    }

    pub(super) async fn owned_artifacts_inner(&mut self) -> StorageResult<Value> {
        // Return the current owner set grouped by artifact. M4 diffs this
        // against its graph rows to find orphans (graph-core G4); M3 itself does
        // not store entity/relation bodies.
        let owned: Vec<(String, Uuid)> = sqlx::query_as(
            "SELECT artifact_type, artifact_id FROM oc_artifact_owners \
             WHERE tenant_id = ? AND workspace_id = ? \
               AND artifact_type IN ('entity','relation')",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .fetch_all(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        let mut relations = Vec::new();
        let mut entities = Vec::new();
        for (kind, id) in owned {
            match kind.as_str() {
                "relation" => relations.push(id),
                "entity" => entities.push(id),
                _ => {}
            }
        }
        Ok(json!({ "relations": relations, "entities": entities }))
    }
}
