//! Artifact publication ledger transitions and reconciliation.
use super::*;

impl TursoStore {
    pub(super) async fn reconcile_ledger_inner(&self) -> StorageResult<Value> {
        let now = now_ms();
        let orphaned = native_turso::query(
            "UPDATE oc_artifact_ledger SET state = 'orphan', updated_at = ? \
             WHERE state = 'pending' AND updated_at < ?",
        )
        .bind(now)
        .bind(now - LEDGER_PENDING_TIMEOUT_MS)
        .execute(&self.pool)
        .await
        .map_err(turso_err)?
        .rows_affected();
        let requeued = native_turso::query(
            "UPDATE oc_artifact_ledger SET state = 'pending', updated_at = ? \
             WHERE state = 'retry_wait' AND next_retry_at IS NOT NULL AND next_retry_at <= ?",
        )
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(turso_err)?
        .rows_affected();
        Ok(json!({ "requeued": requeued, "orphaned": orphaned }))
    }
}

impl TursoTx {
    pub(super) async fn register_pending_inner(&mut self, entry: LedgerEntry) -> StorageResult<()> {
        if entry.key.scope != self.scope {
            return Err(StorageError::Forbidden);
        }
        native_turso::query(
            "INSERT INTO oc_artifact_ledger(tenant_id, workspace_id, source_id, version, \
             artifact_type, artifact_id, surface, generation, idempotency_key, state, \
             created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?) \
             ON CONFLICT DO NOTHING",
        )
        .bind(entry.key.scope.tenant_id)
        .bind(entry.key.scope.workspace_id)
        .bind(entry.key.source.source_id)
        .bind(entry.key.source.version)
        .bind(entry.artifact_type)
        .bind(entry.key.artifact_id)
        .bind(entry.key.surface.as_str())
        .bind(entry.key.generation)
        .bind(entry.idempotency_key)
        .bind(now_ms())
        .bind(now_ms())
        .execute(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        Ok(())
    }

    pub(super) async fn confirm_committed_inner(&mut self, key: LedgerKey) -> StorageResult<()> {
        if key.scope != self.scope {
            return Err(StorageError::Forbidden);
        }
        // Both reads/writes use the caller's transaction. Missing or withdrawn
        // sources are a no-op, but still prepare the exact ledger UPDATE.
        let active: bool = native_turso::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM oc_events WHERE tenant_id=? AND workspace_id=? AND id=? AND state='active')",
        )
        .bind(key.scope.tenant_id)
        .bind(key.scope.workspace_id)
        .bind(key.source.source_id)
        .fetch_one(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        native_turso::query(
            "UPDATE oc_artifact_ledger SET state = 'committed', updated_at = ? \
             WHERE tenant_id = ? AND workspace_id = ? AND source_id = ? AND version = ? \
               AND artifact_id = ? AND surface = ? AND generation = ? \
               AND state IN ('pending','retry_wait') AND ?",
        )
        .bind(now_ms())
        .bind(key.scope.tenant_id)
        .bind(key.scope.workspace_id)
        .bind(key.source.source_id)
        .bind(key.source.version)
        .bind(key.artifact_id)
        .bind(key.surface.as_str())
        .bind(key.generation)
        .bind(active)
        .execute(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        Ok(())
    }

    pub(super) async fn fail_retryable_inner(
        &mut self,
        key: LedgerKey,
        error: &str,
    ) -> StorageResult<()> {
        if key.scope != self.scope {
            return Err(StorageError::Forbidden);
        }
        let now = now_ms();
        let attempt: i32 = native_turso::query_scalar(
            "SELECT attempt FROM oc_artifact_ledger \
             WHERE tenant_id = ? AND workspace_id = ? AND source_id = ? AND version = ? \
               AND artifact_id = ? AND surface = ? AND generation = ?",
        )
        .bind(key.scope.tenant_id)
        .bind(key.scope.workspace_id)
        .bind(key.source.source_id)
        .bind(key.source.version)
        .bind(key.artifact_id)
        .bind(key.surface.as_str())
        .bind(key.generation)
        .fetch_one(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        let next = attempt + 1;
        if next >= MAX_LEDGER_ATTEMPTS {
            native_turso::query(
                "UPDATE oc_artifact_ledger SET state = 'orphan', attempt = ?, last_error = ?, \
                 next_retry_at = NULL, updated_at = ? \
                 WHERE tenant_id = ? AND workspace_id = ? AND source_id = ? AND version = ? \
                   AND artifact_id = ? AND surface = ? AND generation = ? \
                   AND state IN ('pending','retry_wait')",
            )
            .bind(next)
            .bind(error)
            .bind(now)
            .bind(key.scope.tenant_id)
            .bind(key.scope.workspace_id)
            .bind(key.source.source_id)
            .bind(key.source.version)
            .bind(key.artifact_id)
            .bind(key.surface.as_str())
            .bind(key.generation)
            .execute(&mut *self.tx)
            .await
            .map_err(turso_err)?;
        } else {
            native_turso::query(
                "UPDATE oc_artifact_ledger SET state = 'retry_wait', attempt = ?, last_error = ?, \
                 next_retry_at = ?, updated_at = ? \
                 WHERE tenant_id = ? AND workspace_id = ? AND source_id = ? AND version = ? \
                   AND artifact_id = ? AND surface = ? AND generation = ? \
                   AND state IN ('pending','retry_wait')",
            )
            .bind(next)
            .bind(error)
            .bind(now + retry_delay(next))
            .bind(now)
            .bind(key.scope.tenant_id)
            .bind(key.scope.workspace_id)
            .bind(key.source.source_id)
            .bind(key.source.version)
            .bind(key.artifact_id)
            .bind(key.surface.as_str())
            .bind(key.generation)
            .execute(&mut *self.tx)
            .await
            .map_err(turso_err)?;
        }
        Ok(())
    }

    pub(super) async fn fail_permanent_inner(
        &mut self,
        key: LedgerKey,
        error: &str,
    ) -> StorageResult<()> {
        if key.scope != self.scope {
            return Err(StorageError::Forbidden);
        }
        native_turso::query(
            "UPDATE oc_artifact_ledger SET state = 'orphan', last_error = ?, updated_at = ? \
             WHERE tenant_id = ? AND workspace_id = ? AND source_id = ? AND version = ? \
               AND artifact_id = ? AND surface = ? AND generation = ? \
               AND state IN ('pending','retry_wait')",
        )
        .bind(error)
        .bind(now_ms())
        .bind(key.scope.tenant_id)
        .bind(key.scope.workspace_id)
        .bind(key.source.source_id)
        .bind(key.source.version)
        .bind(key.artifact_id)
        .bind(key.surface.as_str())
        .bind(key.generation)
        .execute(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::ledger::Surface;

    #[tokio::test]
    async fn confirm_committed_keeps_missing_source_noop_and_exact_retry_fields() {
        let dir = tempfile::tempdir().unwrap();
        let pool = Pool::open(dir.path().join("ledger-confirm.db"))
            .await
            .unwrap();
        for sql in [
            "CREATE TABLE oc_events(tenant_id BLOB,workspace_id BLOB,id BLOB,state TEXT)",
            "CREATE TABLE oc_artifact_ledger(tenant_id BLOB,workspace_id BLOB,source_id BLOB,version INTEGER,artifact_id BLOB,surface TEXT,generation INTEGER,state TEXT,updated_at INTEGER,attempt INTEGER,last_error TEXT,next_retry_at INTEGER)",
        ] {
            native_turso::query(sql).execute(&pool).await.unwrap();
        }
        let scope = Scope {
            tenant_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
        };
        let mut tx = TursoTx {
            tx: pool.begin_with("BEGIN IMMEDIATE").await.unwrap(),
            scope,
            principal_id: Uuid::new_v4(),
            required: Vec::new(),
            command: None,
        };
        let key = LedgerKey {
            scope,
            source: SourceVersion {
                source_id: Uuid::new_v4(),
                version: 2,
            },
            artifact_id: Uuid::new_v4(),
            surface: Surface::Vector,
            generation: 3,
        };
        native_turso::query(
            "INSERT INTO oc_artifact_ledger VALUES(?,?,?,?,?,?,?,'retry_wait',1,4,'keep',123)",
        )
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(key.source.source_id)
        .bind(key.source.version)
        .bind(key.artifact_id)
        .bind(key.surface.as_str())
        .bind(key.generation)
        .execute(&mut *tx.tx)
        .await
        .unwrap();
        // Missing and withdrawn sources remain successful no-ops.
        tx.confirm_committed_inner(key).await.unwrap();
        native_turso::query("INSERT INTO oc_events VALUES(?,?,?,'retracted')")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(key.source.source_id)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        tx.confirm_committed_inner(key).await.unwrap();
        let before: (String, i64, i64, String, i64) = native_turso::query_as(
            "SELECT state,updated_at,attempt,last_error,next_retry_at FROM oc_artifact_ledger",
        )
        .fetch_one(&mut *tx.tx)
        .await
        .unwrap();
        assert_eq!(before, ("retry_wait".into(), 1, 4, "keep".into(), 123));
        native_turso::query("UPDATE oc_events SET state='active'")
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        let mut wrong = key;
        wrong.generation += 1;
        tx.confirm_committed_inner(wrong).await.unwrap();
        let unchanged: String = native_turso::query_scalar("SELECT state FROM oc_artifact_ledger")
            .fetch_one(&mut *tx.tx)
            .await
            .unwrap();
        assert_eq!(unchanged, "retry_wait");
        tx.confirm_committed_inner(key).await.unwrap();
        let after: (String, i64, String, i64) = native_turso::query_as(
            "SELECT state,attempt,last_error,next_retry_at FROM oc_artifact_ledger",
        )
        .fetch_one(&mut *tx.tx)
        .await
        .unwrap();
        assert_eq!(after, ("committed".into(), 4, "keep".into(), 123));
        native_turso::query("UPDATE oc_artifact_ledger SET state='orphan'")
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        tx.confirm_committed_inner(key).await.unwrap();
        let state: String = native_turso::query_scalar("SELECT state FROM oc_artifact_ledger")
            .fetch_one(&mut *tx.tx)
            .await
            .unwrap();
        assert_eq!(state, "orphan");
        wrong.scope.workspace_id = Uuid::new_v4();
        assert!(matches!(
            tx.confirm_committed_inner(wrong).await,
            Err(StorageError::Forbidden)
        ));
        tx.tx.rollback().await.unwrap();
        // An absent source must not bypass preparing the ledger statement.
        native_turso::query("DROP TABLE oc_artifact_ledger")
            .execute(&pool)
            .await
            .unwrap();
        let mut tx = TursoTx {
            tx: pool.begin_with("BEGIN IMMEDIATE").await.unwrap(),
            scope,
            principal_id: Uuid::new_v4(),
            required: Vec::new(),
            command: None,
        };
        assert!(matches!(
            tx.confirm_committed_inner(key).await,
            Err(StorageError::Backend(_))
        ));
        drop(tx);
        pool.shutdown().await.unwrap();
    }
}
