//! Live visibility for one accepted message publication, never a success cache.
use super::*;
use crate::messages::{AcceptedMessageSpec, IngestReceipt, PublicationStatus};

/// Project only small metadata. Publication JSON contains every embedding and
/// must never be deserialized on the async executor during a status poll.
#[derive(sqlx::FromRow)]
struct BatchPublicationRow {
    operation: String,
    state: String,
    generation: i64,
    asset_id: Option<Uuid>,
    source_event_id: Option<Uuid>,
    accepted_spec: Option<String>,
    expected_version: Option<i64>,
    publication_generation: Option<i64>,
    publication_profile: Option<String>,
    memory_count: Option<i64>,
    publication_asset: Option<String>,
    publication_version: Option<i64>,
    chunk_count: Option<i64>,
    distinct_chunk_ids: i64,
}

impl SqliteTx {
    /// Scope and live access are established by the caller's transaction. Receipt
    /// references are verified against the original job and source, not trusted as
    /// authority. Historical/source/Learning reads keep their own visibility rules.
    pub async fn message_batch_status(
        &mut self,
        receipt: &IngestReceipt,
    ) -> StorageResult<PublicationStatus> {
        self.check_permission(Permission::Write).await?;
        let row: Option<BatchPublicationRow> = sqlx::query_as(
            "SELECT operation,state,generation,asset_id,source_event_id, \
             json_extract(payload,'$.accepted_spec') AS accepted_spec, \
             json_extract(payload,'$.expected_version') AS expected_version, \
             json_extract(payload,'$.publication.generation') AS publication_generation, \
             json_extract(payload,'$.publication.profile') AS publication_profile, \
             json_array_length(payload,'$.publication.memories') AS memory_count, \
             json_extract(payload,'$.publication.memories[0].asset') AS publication_asset, \
             json_extract(payload,'$.publication.memories[0].version') AS publication_version, \
             json_array_length(payload,'$.publication.memories[0].chunks') AS chunk_count, \
             (SELECT count(DISTINCT json_extract(chunk.value,'$.id')) \
                 FROM json_each(payload,'$.publication.memories[0].chunks') chunk) AS distinct_chunk_ids \
             FROM oc_jobs WHERE tenant_id=? AND workspace_id=? AND id=?",
        )
        .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(receipt.job_id)
        .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        let row = row.ok_or(StorageError::NotFound)?;
        let invalid =
            || StorageError::Conflict("receipt does not match accepted message batch".into());
        let spec: AcceptedMessageSpec =
            serde_json::from_str(row.accepted_spec.as_deref().ok_or_else(invalid)?)
                .map_err(|_| invalid())?;
        if row.operation != "message_batch_ingest"
            || row.asset_id != Some(receipt.asset_id)
            || row.source_event_id != Some(receipt.source_event_id)
            || row.expected_version.unwrap_or(0).checked_add(1)
                != Some(i64::from(receipt.target_version))
            || spec != receipt.accepted_spec
        {
            return Err(invalid());
        }
        let source: Option<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT kind,state,json_extract(content,'$.batch_id'),json_extract(content,'$.session_id') \
             FROM oc_events WHERE tenant_id=? AND workspace_id=? AND id=?",
        )
        .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(receipt.source_event_id)
        .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        let Some((kind, source_state, batch_id, session_id)) = source else {
            return Ok(PublicationStatus::Invisible);
        };
        if kind != crate::messages::SOURCE_KIND
            || batch_id.as_deref() != Some(receipt.batch_id.as_str())
            || session_id.as_deref() != Some(receipt.session_id.as_str())
        {
            return Err(invalid());
        }
        let live_asset: Option<(Option<i32>, bool)> = sqlx::query_as(
            "SELECT current_version,deleted FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND id=?",
        ).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(receipt.asset_id)
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        let Some((current_version, deleted)) = live_asset else {
            return Ok(PublicationStatus::Invisible);
        };
        if deleted || source_state != "active" {
            return Ok(PublicationStatus::Invisible);
        }
        match row.state.as_str() {
            "pending" | "processing" | "retry_wait" => return Ok(PublicationStatus::Pending),
            "failed" => return Ok(PublicationStatus::Failed),
            "cancelled" | "superseded" => return Ok(PublicationStatus::CancelledOrSuperseded),
            "completed" => {}
            _ => return Err(StorageError::Conflict("invalid message job state".into())),
        }
        // An absent/deleted chunk must not make the universal index check pass
        // vacuously. Compare all exact IDs against the durable publication plan.
        let chunk_count = row.chunk_count.unwrap_or(0);
        if current_version != Some(receipt.target_version)
            || row.publication_generation != Some(row.generation)
            || row.publication_profile.as_deref() != Some(spec.embedding_profile.as_str())
            || row.memory_count != Some(1)
            || row.publication_asset.as_deref() != Some(receipt.asset_id.to_string().as_str())
            || row.publication_version != Some(i64::from(receipt.target_version))
            || chunk_count == 0
            || row.distinct_chunk_ids != chunk_count
        {
            return Ok(PublicationStatus::Invisible);
        }
        // JSON projection/iteration runs on SQLite's blocking connection thread;
        // no source text, embedding arrays or saved plan reach this async thread.
        let visible: i64 = sqlx::query_scalar(&format!(
            "WITH expected AS MATERIALIZED ( \
                 SELECT upper(replace(json_extract(chunk.value,'$.id'),'-','')) AS id \
                 FROM oc_jobs j,json_each(j.payload,'$.publication.memories[0].chunks') chunk \
                 WHERE j.tenant_id=? AND j.workspace_id=? AND j.id=?) \
             SELECT count(*) {} AND c.asset_id=? AND c.version=? AND e.id=? AND {} \
             AND EXISTS(SELECT 1 FROM expected WHERE expected.id=hex(c.id)) \
             AND EXISTS(SELECT 1 FROM oc_index_entries i WHERE i.tenant_id=c.tenant_id AND i.workspace_id=c.workspace_id \
                 AND i.artifact_id=c.id AND i.field='content' AND i.model_id=? AND i.state='ready' AND i.generation=? \
                 AND EXISTS(SELECT 1 FROM oc_artifact_ledger l WHERE l.tenant_id=c.tenant_id AND l.workspace_id=c.workspace_id \
                     AND l.artifact_id=c.id AND l.artifact_type='chunk' AND l.source_id=e.id AND l.version=c.version \
                     AND l.generation=i.generation AND l.surface='vector' AND l.state='committed'))",
            visibility::CURRENT_CHUNK_ROWS, visibility::CHUNK_SUPPORT_ACTIVE,
        ))
        .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(receipt.job_id)
        .bind(self.scope.tenant_id).bind(self.scope.workspace_id)
        .bind(receipt.asset_id).bind(receipt.target_version).bind(receipt.source_event_id)
        .bind(&spec.embedding_profile).bind(row.generation)
        .fetch_one(&mut *self.tx).await.map_err(sqlite_err)?;
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM oc_chunks WHERE tenant_id=? AND workspace_id=? AND asset_id=? AND version=?",
        ).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(receipt.asset_id).bind(receipt.target_version)
            .fetch_one(&mut *self.tx).await.map_err(sqlite_err)?;
        Ok(if visible == chunk_count && count == visible {
            PublicationStatus::Retrievable
        } else {
            PublicationStatus::Invisible
        })
    }
}
