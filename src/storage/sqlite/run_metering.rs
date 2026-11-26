//! Immutable metering reports are projected into the same transaction as the Run event.
use super::*;
use crate::run::RunEvent;

#[repr(i64)]
enum Metric {
    Duration,
    Tokens,
    EstimatedCost,
    BilledCost,
}

impl SqliteTx {
    pub(super) async fn record_run_metering(
        &mut self,
        run: Uuid,
        created: i64,
        event: &RunEvent,
    ) -> StorageResult<()> {
        let RunEvent::MeteringReported {
            duration_ms,
            agent_tokens,
            estimated_cost,
            billed_cost,
        } = event
        else {
            return Ok(());
        };
        let values = [
            (
                Metric::Duration,
                duration_ms.map(|value| (i64::from(value), "")),
            ),
            (
                Metric::Tokens,
                agent_tokens.map(|value| (i64::from(value), "")),
            ),
            (
                Metric::EstimatedCost,
                estimated_cost
                    .as_ref()
                    .map(|cost| (cost.micros as i64, cost.currency.as_str())),
            ),
            (
                Metric::BilledCost,
                billed_cost
                    .as_ref()
                    .map(|cost| (cost.micros as i64, cost.currency.as_str())),
            ),
        ];
        let existing: Vec<i64> = sqlite_db::query_scalar(
            "SELECT metric FROM oc_run_measurements WHERE tenant_id=? AND workspace_id=? AND run_id=?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(run)
        .fetch_all(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        let records: Vec<_> = values
            .into_iter()
            .filter_map(|(metric, value)| value.map(|value| (metric as i64, value)))
            .collect();
        if records.iter().any(|(metric, _)| existing.contains(metric)) {
            return Err(StorageError::Conflict("Run metric already reported".into()));
        }
        let owner = self.private_key_id().await?;
        for (metric, (value, currency)) in records {
            sqlite_db::query(
                "INSERT INTO oc_run_measurements(tenant_id,workspace_id,run_id,owner_key_id,created_at,metric,value,currency)
                 VALUES(?,?,?,?,?,?,?,?)",
            )
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .bind(run)
            .bind(owner)
            .bind(created)
            .bind(metric)
            .bind(value)
            .bind(currency)
            .execute(&mut *self.tx)
            .await
            .map_err(sqlite_err)?;
        }
        Ok(())
    }

    pub(super) async fn run_metering(
        &mut self,
        owner: Option<Uuid>,
        start: i64,
        end: i64,
    ) -> StorageResult<Value> {
        // Both alternatives use the same numbered bindings; a key-scoped read
        // can use the owner/time index without scanning other owners.
        let scope = if owner.is_some() {
            "tenant_id=?1 AND workspace_id=?2 AND created_at>=?3 AND created_at<?4 AND owner_key_id=?5"
        } else {
            "tenant_id=?1 AND workspace_id=?2 AND created_at>=?3 AND created_at<?4 AND ?5 IS NULL"
        };
        // Sum high and low integer parts independently before recombining. This
        // preserves exact monetary micros and avoids a floating point SUM.
        let groups: Vec<(i64, String, i64, i64, i64)> = sqlite_db::query_as(&format!(
            "SELECT metric,currency,COUNT(*),SUM(value/1000000),SUM(value%1000000)
             FROM oc_run_measurements WHERE {scope} GROUP BY metric,currency ORDER BY metric,currency"
        ))
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(start)
        .bind(end)
        .bind(owner)
        .fetch_all(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        let mut duration_runs = 0;
        let mut duration_p95_ms: Option<i64> = None;
        let mut token_runs = 0;
        let mut agent_tokens = None;
        let mut costs = Vec::new();
        for (metric, currency, runs, whole, remainder) in groups {
            let total = || (i128::from(whole) * 1_000_000 + i128::from(remainder)).to_string();
            match metric {
                value if value == Metric::Duration as i64 => {
                    duration_runs = runs;
                    // Nearest rank ceil(0.95 * n), expressed as a zero-based offset.
                    let rank = runs - runs / 20 - 1;
                    duration_p95_ms = Some(
                        sqlite_db::query_scalar(&format!(
                            "SELECT value FROM oc_run_measurements WHERE {scope} AND metric=?6
                         ORDER BY value LIMIT 1 OFFSET ?7"
                        ))
                        .bind(self.scope.tenant_id)
                        .bind(self.scope.workspace_id)
                        .bind(start)
                        .bind(end)
                        .bind(owner)
                        .bind(Metric::Duration as i64)
                        .bind(rank)
                        .fetch_one(&mut *self.tx)
                        .await
                        .map_err(sqlite_err)?,
                    );
                }
                value if value == Metric::Tokens as i64 => {
                    token_runs = runs;
                    agent_tokens = Some(total());
                }
                _ => {
                    let basis = if metric == Metric::EstimatedCost as i64 {
                        "estimated"
                    } else {
                        "billed"
                    };
                    costs.push(
                        json!({"basis":basis,"currency":currency,"micros":total(),"runs":runs}),
                    );
                }
            }
        }
        Ok(json!({
            "duration_p95_ms":duration_p95_ms, "duration_runs":duration_runs,
            "agent_tokens":agent_tokens, "token_runs":token_runs, "costs":costs
        }))
    }
}
