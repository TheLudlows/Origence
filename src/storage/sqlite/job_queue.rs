//! Durable queue registration, claims, fencing, retries and settlement.
use super::*;

/// A job left in `processing` longer than this is treated as abandoned by a
/// crashed worker and requeued by the next claim (A2.5).
const PROCESSING_TIMEOUT_MS: i64 = 5 * 60 * 1000;

impl JobQueue for TursoStore {
    async fn claim_next(&self) -> StorageResult<Option<ClaimedJob>> {
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(turso_err)?;
        let now = now_ms();
        // Requeue processing jobs abandoned by a crashed worker (A2.5).
        native_turso::query(
            "UPDATE oc_jobs SET state = 'pending', updated_at = ? \
             WHERE state = 'processing' AND NOT cancel_requested AND updated_at < ?",
        )
        .bind(now)
        .bind(now - PROCESSING_TIMEOUT_MS)
        .execute(&mut *tx)
        .await
        .map_err(turso_err)?;
        let row: Option<(
            Uuid,
            Uuid,
            Uuid,
            String,
            String,
            i64,
            i64,
            Uuid,
            Option<Uuid>,
            Option<Uuid>,
        )> = native_turso::query_as(
            "SELECT id, tenant_id, workspace_id, operation, payload, generation, run_token, \
                    created_by, asset_id, source_event_id \
             FROM oc_jobs \
             WHERE state IN ('pending', 'retry_wait') AND NOT cancel_requested \
               AND (next_retry_at IS NULL OR next_retry_at <= ?) \
             ORDER BY created_at LIMIT 1",
        )
        .bind(now)
        .fetch_optional(&mut *tx)
        .await
        .map_err(turso_err)?;
        let Some((
            job_id,
            tenant_id,
            workspace_id,
            kind,
            payload,
            generation,
            run_token,
            created_by,
            asset,
            source,
        )) = row
        else {
            tx.commit().await.map_err(turso_err)?;
            return Ok(None);
        };
        // The state guard makes the claim atomic even if two workers race.
        let claimed = native_turso::query(
            "UPDATE oc_jobs SET state = 'processing', run_token = run_token + 1, \
             attempt = attempt + 1, updated_at = ? \
             WHERE tenant_id = ? AND workspace_id = ? AND id = ? AND state IN ('pending', 'retry_wait')",
        )
        .bind(now)
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(job_id)
        .execute(&mut *tx)
        .await
        .map_err(turso_err)?;
        tx.commit().await.map_err(turso_err)?;
        if claimed.rows_affected() == 0 {
            return Ok(None);
        }
        // Recovery payloads may include a complete publication plan. Decode
        // outside both the storage transaction and the async executor thread;
        // malformed JSON still reaches the Worker's ordinary failure path.
        let payload = tokio::task::spawn_blocking(move || {
            serde_json::from_str(&payload).unwrap_or(Value::Null)
        })
        .await
        .map_err(|_| StorageError::Unavailable("job payload decoding task failed".into()))?;
        Ok(Some(ClaimedJob {
            scope: Scope {
                tenant_id,
                workspace_id,
            },
            job_id,
            kind,
            payload,
            generation,
            run_token: run_token + 1,
            created_by,
            asset,
            source,
        }))
    }

    async fn settle(&self, claim: ClaimedJob, finish: JobFinish) -> StorageResult<()> {
        let now = now_ms();
        let (state, outcome, attempt, next_retry_at) = match finish {
            JobFinish::Completed => ("completed", Some("ok"), None, None),
            JobFinish::Failed => ("failed", Some("failed"), None, None),
            JobFinish::Cancelled => ("cancelled", Some("cancelled"), None, None),
            JobFinish::Retryable { attempt } => (
                "retry_wait",
                None,
                Some(attempt),
                Some(now + retry_delay(attempt)),
            ),
        };
        let result = native_turso::query(
            "UPDATE oc_jobs SET state = ?, outcome = ?, attempt = COALESCE(?, attempt), \
             next_retry_at = ?, updated_at = ? \
             WHERE tenant_id = ? AND workspace_id = ? AND id = ? AND generation = ? AND run_token = ? AND state = 'processing'",
        )
        .bind(state)
        .bind(outcome)
        .bind(attempt)
        .bind(next_retry_at)
        .bind(now)
        .bind(claim.scope.tenant_id)
        .bind(claim.scope.workspace_id)
        .bind(claim.job_id)
        .bind(claim.generation)
        .bind(claim.run_token)
        .execute(&self.pool)
        .await
        .map_err(turso_err)?;
        if result.rows_affected() == 0 {
            return Err(StorageError::Conflict("stale job claim".to_string()));
        }
        Ok(())
    }
}

impl TursoTx {
    pub(super) async fn enqueue_inner(&mut self, work: WorkItem) -> StorageResult<()> {
        native_turso::query(
            "INSERT INTO oc_jobs(tenant_id, workspace_id, id, created_by, operation, payload, \
             state, run_token, generation, attempt, asset_id, source_event_id, cancel_requested, \
             created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, 'pending', 0, 1, 0, ?, ?, 0, ?, ?)",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(work.job_id)
        .bind(self.principal_id)
        .bind(work.kind)
        .bind(serde_json::to_string(&work.payload).unwrap_or_default())
        .bind(work.asset)
        .bind(work.source)
        .bind(now_ms())
        .bind(now_ms())
        .execute(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        Ok(())
    }

    pub(super) async fn cancel_affected_jobs_inner(&mut self) -> StorageResult<()> {
        native_turso::query(
            "UPDATE oc_jobs SET state = 'cancelled', cancel_requested = 1, run_token = run_token + 1, updated_at = ? \
             WHERE tenant_id = ? AND workspace_id = ? AND operation <> 'cleanup' AND state IN ('pending', 'processing', 'failed', 'retry_wait') \
               AND (EXISTS(SELECT 1 FROM oc_assets a WHERE a.id = oc_jobs.asset_id AND a.tenant_id = oc_jobs.tenant_id AND a.workspace_id = oc_jobs.workspace_id AND a.deleted) \
                 OR EXISTS(SELECT 1 FROM oc_events e WHERE e.id = oc_jobs.source_event_id AND e.tenant_id = oc_jobs.tenant_id AND e.workspace_id = oc_jobs.workspace_id AND e.state = 'retracted'))",
        )
        .bind(now_ms())
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .execute(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        Ok(())
    }

    pub(super) async fn job_row_inner(&mut self, id: Uuid) -> StorageResult<Value> {
        let row: Option<(Uuid, String, String, Option<Uuid>, Option<Uuid>)> = native_turso::query_as(
            "SELECT created_by, state, operation, asset_id, source_event_id FROM oc_jobs \
             WHERE tenant_id = ? AND workspace_id = ? AND id = ?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(id)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        let Some((created_by, state, operation, asset, source)) = row else {
            return Err(StorageError::NotFound);
        };
        Ok(json!({
            "created_by": created_by,
            "state": state,
            "operation": operation,
            "asset_id": asset,
            "source_event_id": source,
        }))
    }

    pub(super) async fn set_job_action_inner(
        &mut self,
        id: Uuid,
        action: &str,
    ) -> StorageResult<i64> {
        native_turso::query_scalar(
            "UPDATE oc_jobs SET state = ?, generation = generation + 1, run_token = run_token + 1, \
                    cancel_requested = ?, error_code = NULL, next_retry_at = NULL, attempt = 0, updated_at = ? \
             WHERE tenant_id = ? AND workspace_id = ? AND id = ? RETURNING generation",
        )
        .bind(if action == "cancel" {
            "cancelled"
        } else {
            "pending"
        })
        .bind(action == "cancel")
        .bind(now_ms())
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(id)
        .fetch_one(&mut *self.tx)
        .await
        .map_err(turso_err)
    }

    pub(super) async fn job_view_inner(&mut self, id: Uuid) -> StorageResult<Value> {
        let row: Option<(Uuid, String, String, i64, Option<String>, Option<String>, Option<String>, bool, i64)> = native_turso::query_as(
            "SELECT id, operation, state, generation, outcome, result, error_code, cancel_requested, updated_at \
             FROM oc_jobs WHERE tenant_id = ? AND workspace_id = ? AND id = ?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(id)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        let Some((
            job_id,
            operation,
            state,
            generation,
            outcome,
            result,
            error_code,
            cancel_requested,
            updated_at,
        )) = row
        else {
            return Err(StorageError::NotFound);
        };
        Ok(json!({
            "id": job_id,
            "operation": operation,
            "state": state,
            "generation": generation,
            "outcome": outcome,
            "result": result.and_then(|s| serde_json::from_str::<Value>(&s).ok()),
            "error_code": error_code,
            "cancel_requested": cancel_requested,
            "updated_at": updated_at,
        }))
    }

    pub(super) async fn active_job_inner(
        &mut self,
        job: Uuid,
        generation: i64,
        run_token: i64,
    ) -> StorageResult<bool> {
        native_turso::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM oc_jobs WHERE tenant_id = ? AND workspace_id = ? AND id = ? AND generation = ? AND run_token = ? AND state = 'processing' AND NOT cancel_requested)",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(job)
        .bind(generation)
        .bind(run_token)
        .fetch_one(&mut *self.tx)
        .await
        .map_err(turso_err)
    }

    pub(super) async fn settle_completed_inner(
        &mut self,
        job: Uuid,
        outcome: &str,
        result: &Value,
    ) -> StorageResult<()> {
        native_turso::query(
            "UPDATE oc_jobs SET state = 'completed', outcome = ?, result = ?, error_code = NULL, updated_at = ? \
             WHERE tenant_id = ? AND workspace_id = ? AND id = ?",
        )
        .bind(outcome)
        .bind(serde_json::to_string(result).unwrap_or_default())
        .bind(now_ms())
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(job)
        .execute(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        Ok(())
    }

    pub(super) async fn settle_failed_inner(
        &mut self,
        job: Uuid,
        generation: i64,
        run_token: i64,
        state: &str,
        error_code: &str,
    ) -> StorageResult<()> {
        native_turso::query(
            "UPDATE oc_jobs SET state = ?, error_code = ?, updated_at = ? \
             WHERE tenant_id = ? AND workspace_id = ? AND id = ? AND generation = ? AND run_token = ? AND state = 'processing'",
        )
        .bind(state)
        .bind(error_code)
        .bind(now_ms())
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(job)
        .bind(generation)
        .bind(run_token)
        .execute(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        Ok(())
    }
}
