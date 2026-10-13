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
        // Re-check source validity and generation in the same statement (A2.6
        // step 3): only an active source's matching generation commits.
        native_turso::query(
            "UPDATE oc_artifact_ledger SET state = 'committed', updated_at = ? \
             WHERE tenant_id = ? AND workspace_id = ? AND source_id = ? AND version = ? \
               AND artifact_id = ? AND surface = ? AND generation = ? \
               AND state IN ('pending','retry_wait') \
               AND EXISTS (SELECT 1 FROM oc_events e \
                           WHERE e.tenant_id = oc_artifact_ledger.tenant_id \
                             AND e.workspace_id = oc_artifact_ledger.workspace_id \
                             AND e.id = oc_artifact_ledger.source_id \
                             AND e.state = 'active')",
        )
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
