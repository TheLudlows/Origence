//! Live visibility for one accepted message publication, never a success cache.
use super::read_model::{AssociationKey, READ_KEYS, support_members};
use super::*;
use crate::messages::{AcceptedMessageSpec, IngestReceipt, PublicationStatus};
use std::collections::HashSet;

const RECEIPT_METADATA_FORMAT: &str = "message-receipt:v1";

#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptMetadata {
    format: String,
    chunks: Vec<ReceiptChunk>,
}

#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptChunk {
    // Keep native json_quote output as a token, not a Rust interpretation of ID.
    id_json: String,
    #[serde(deserialize_with = "receipt_dimension")]
    dimension: Option<i64>,
}

fn receipt_dimension<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    serde::Deserialize::deserialize(deserializer)
}

fn invalid_plan() -> StorageError {
    StorageError::Unavailable("invalid message publication plan".into())
}

impl ReceiptMetadata {
    fn empty() -> Self {
        Self {
            format: RECEIPT_METADATA_FORMAT.into(),
            chunks: Vec::new(),
        }
    }

    fn decode(text: Option<&str>, count: usize) -> StorageResult<Self> {
        // Missing/null/empty chunks retain their original zero-ID behavior.
        if count == 0 {
            return Ok(Self::empty());
        }
        let metadata: Self =
            serde_json::from_str(text.ok_or_else(invalid_plan)?).map_err(|_| invalid_plan())?;
        if metadata.format != RECEIPT_METADATA_FORMAT || metadata.chunks.len() != count {
            return Err(invalid_plan());
        }
        Ok(metadata)
    }

    fn ids_json(&self) -> String {
        format!(
            "[{}]",
            self.chunks
                .iter()
                .map(|chunk| chunk.id_json.as_str())
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}

/// Project metadata without transferring the saved embedding arrays. Ordinary
/// writer IDs are UUIDs; malformed ID values are not given a new byte limit.
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

impl BatchPublicationRow {
    fn from_metadata(row: &native_turso::Row, distinct_chunk_ids: i64) -> native_turso::Result<Self> {
        Ok(Self {
            operation: row.try_get("operation")?,
            state: row.try_get("state")?,
            generation: row.try_get("generation")?,
            asset_id: row.try_get("asset_id")?,
            source_event_id: row.try_get("source_event_id")?,
            accepted_spec: row.try_get("accepted_spec")?,
            expected_version: row.try_get("expected_version")?,
            publication_generation: row.try_get("publication_generation")?,
            publication_profile: row.try_get("publication_profile")?,
            memory_count: row.try_get("memory_count")?,
            publication_asset: row.try_get("publication_asset")?,
            publication_version: row.try_get("publication_version")?,
            chunk_count: row.try_get("chunk_count")?,
            distinct_chunk_ids,
        })
    }
}

impl TursoTx {
    /// Scope and live access are established by the caller's transaction. Receipt
    /// references are verified against the original job and source, not trusted as
    /// authority. Historical/source/Learning reads keep their own visibility rules.
    pub async fn message_batch_status(
        &mut self,
        receipt: &IngestReceipt,
    ) -> StorageResult<PublicationStatus> {
        self.check_permission(Permission::Write).await?;
        let row = native_turso::query(
            "SELECT operation,state,generation,asset_id,source_event_id, \
             json_extract(payload,'$.accepted_spec') AS accepted_spec, \
             json_extract(payload,'$.expected_version') AS expected_version, \
             json_extract(payload,'$.publication.generation') AS publication_generation, \
             json_extract(payload,'$.publication.profile') AS publication_profile, \
             json_array_length(payload,'$.publication.memories') AS memory_count, \
             json_extract(payload,'$.publication.memories[0].asset') AS publication_asset, \
             json_extract(payload,'$.publication.memories[0].version') AS publication_version, \
             json_array_length(payload,'$.publication.memories[0].chunks') AS chunk_count, \
             json_type(payload,'$.publication.memories[0].chunks') AS chunk_type, \
             CASE WHEN json_array_length(payload,'$.publication.memories[0].chunks')>0 THEN json_quote(json_extract(payload,'$.publication.receipt_metadata')) END AS receipt_metadata \
             FROM oc_jobs WHERE tenant_id=? AND workspace_id=? AND id=?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(receipt.job_id)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        let row = row.ok_or(StorageError::NotFound)?;
        let kind: Option<&str> = row.try_get("chunk_type").map_err(turso_err)?;
        if !matches!(kind, None | Some("null" | "array")) {
            return Err(StorageError::Unavailable(
                "invalid message publication plan".into(),
            ));
        }
        // The immutable lightweight plan is saved atomically with the heavy
        // publication. Validate all native ID tokens before early state returns;
        // dimensions remain relevant only in the completed/visible branch.
        let count: Option<i64> = row.try_get("chunk_count").map_err(turso_err)?;
        let count = usize::try_from(count.unwrap_or(0)).map_err(|_| invalid_plan())?;
        let metadata = ReceiptMetadata::decode(
            row.try_get::<Option<&str>>("receipt_metadata")
                .map_err(turso_err)?,
            count,
        )?;
        let distinct = self.publication_distinct_ids(&metadata).await?;
        let row = BatchPublicationRow::from_metadata(&row, distinct).map_err(turso_err)?;
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
        let source: Option<(String, String, Option<String>, Option<String>)> = native_turso::query_as(
            "SELECT kind,state,json_extract(content,'$.batch_id'),json_extract(content,'$.session_id') \
             FROM oc_events WHERE tenant_id=? AND workspace_id=? AND id=?",
        )
        .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(receipt.source_event_id)
        .fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        let Some((kind, source_state, batch_id, session_id)) = source else {
            return Ok(PublicationStatus::Invisible);
        };
        if kind != crate::messages::SOURCE_KIND
            || batch_id.as_deref() != Some(receipt.batch_id.as_str())
            || session_id.as_deref() != Some(receipt.session_id.as_str())
        {
            return Err(invalid());
        }
        let live_asset: Option<(Option<i32>, bool)> = native_turso::query_as(
            "SELECT current_version,deleted FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND id=?",
        ).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(receipt.asset_id)
            .fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
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
        let expected = self.publication_expected_chunks(&metadata).await?;
        let visible = self
            .publication_visible_count(receipt, &spec.embedding_profile, row.generation, &expected)
            .await?;
        let count: i64 = native_turso::query_scalar(
            "SELECT count(*) FROM oc_chunks WHERE tenant_id=? AND workspace_id=? AND asset_id=? AND version=?",
        ).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(receipt.asset_id).bind(receipt.target_version)
            .fetch_one(&mut *self.tx).await.map_err(turso_err)?;
        Ok(if visible == chunk_count && count == visible {
            PublicationStatus::Retrievable
        } else {
            PublicationStatus::Invisible
        })
    }
}

const PUBLICATION_SLOTS: usize = 8;

// One request remains one actual chunk even if several expected IDs normalize
// to its hex ID or several index/ledger witnesses exist. Keys stay native.
struct PublicationChunk {
    id: AssociationKey,
    hex_id: String,
}

type ExpectedChunks = HashMap<String, HashSet<i64>>;

impl TursoTx {
    async fn publication_distinct_ids(&mut self, metadata: &ReceiptMetadata) -> StorageResult<i64> {
        if metadata.chunks.is_empty() {
            return Ok(0);
        }
        // Validate each opaque native token before joining them into an array.
        // Empty or multiple values must not silently alter its membership.
        let tokens: Vec<_> = metadata.chunks.iter().map(|chunk| chunk.id_json.as_str()).collect();
        let invalid: i64 = native_turso::query_scalar(
            "SELECT count(*) FROM json_each(?) WHERE NOT json_valid(value)",
        ).bind(json!(tokens).to_string()).fetch_one(&mut *self.tx).await.map_err(turso_err)?;
        if invalid != 0 {
            return Err(invalid_plan());
        }
        native_turso::query_scalar("SELECT count(DISTINCT value) FROM json_each(?)")
            .bind(metadata.ids_json())
            .fetch_one(&mut *self.tx)
            .await
            .map_err(turso_err)
    }

    async fn publication_expected_chunks(
        &mut self,
        metadata: &ReceiptMetadata,
    ) -> StorageResult<ExpectedChunks> {
        let mut expected: ExpectedChunks = HashMap::new();
        if metadata.chunks.is_empty() {
            return Ok(expected);
        }
        let rows = native_turso::query("SELECT key,upper(replace(value,'-','')) AS id FROM json_each(?)")
            .bind(metadata.ids_json())
            .fetch_all(&mut *self.tx)
            .await
            .map_err(turso_err)?;
        for row in rows {
            let index: usize = row.try_get("key").map_err(turso_err)?;
            let id: Option<String> = row.try_get("id").map_err(turso_err)?;
            let chunk = metadata.chunks.get(index).ok_or_else(invalid_plan)?;
            if let (Some(id), Some(dimension)) = (id, chunk.dimension)
                && (1..=4096).contains(&dimension)
            {
                expected.entry(id).or_default().insert(dimension);
            }
        }
        Ok(expected)
    }

    /// Message receipt membership is part of the durable plan, never a cache of
    /// successful publication. Other operations retain their existing plan form.
    pub(super) async fn save_message_publication_inner(
        &mut self,
        job: Uuid,
        plan: &Value,
    ) -> StorageResult<bool> {
        // Bind the same in-memory plan used below; only lightweight saved
        // metadata is read back from the job. Stale generations are prepared
        // anew, while a current saved plan cannot acquire different membership.
        let serialized = plan.to_string();
        let saved: Option<(bool, Option<i64>, Option<String>)> = native_turso::query_as(
            "SELECT json_extract(payload,'$.publication.generation') IS json_extract(?1,'$.generation'),json_array_length(payload,'$.publication.memories[0].chunks'),CASE WHEN json_array_length(payload,'$.publication.memories[0].chunks')>0 THEN json_quote(json_extract(payload,'$.publication.receipt_metadata')) END FROM oc_jobs WHERE tenant_id=?2 AND workspace_id=?3 AND id=?4 AND operation='message_batch_ingest'",
        ).bind(&serialized).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(job)
            .fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        let Some((same_generation, saved_count, saved_metadata)) = saved else {
            return Ok(false);
        };
        let retained = if same_generation && saved_count.unwrap_or(0) > 0 {
            let count = usize::try_from(saved_count.unwrap_or(0)).map_err(|_| invalid_plan())?;
            Some(ReceiptMetadata::decode(saved_metadata.as_deref(), count)?)
        } else {
            None
        };
        // The extra native pass is charged to prepare/save, not repeated by
        // every status poll. Native malformed-JSON errors remain backend errors.
        let metadata = self.derive_receipt_metadata(&serialized).await?;
        if retained.as_ref().is_some_and(|saved| saved != &metadata) {
            return Err(invalid_plan());
        }
        let supplied = plan.get("receipt_metadata");
        if let Some(supplied) = supplied {
            let supplied: ReceiptMetadata =
                serde_json::from_value(supplied.clone()).map_err(|_| invalid_plan())?;
            if supplied != metadata {
                return Err(invalid_plan());
            }
        }
        let metadata_text = if retained.is_some() {
            // A retry reuses the original field, even if its caller omitted it.
            saved_metadata.ok_or_else(invalid_plan)?
        } else if let Some(supplied) = supplied {
            supplied.to_string()
        } else {
            serde_json::to_string(&metadata).map_err(|_| invalid_plan())?
        };
        native_turso::query(
            "UPDATE oc_jobs SET payload=json_set(payload,'$.publication',json_set(json(?),'$.receipt_metadata',json(?))),updated_at=? WHERE tenant_id=? AND workspace_id=? AND id=?",
        ).bind(serialized).bind(metadata_text).bind(now_ms())
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(job)
            .execute(&mut *self.tx).await.map_err(turso_err)?;
        Ok(true)
    }

    async fn derive_receipt_metadata(&mut self, plan: &str) -> StorageResult<ReceiptMetadata> {
        let (kind, count): (Option<String>, Option<i64>) = native_turso::query_as(
            "SELECT json_type(?1,'$.memories[0].chunks'),json_array_length(?1,'$.memories[0].chunks')",
        ).bind(plan).fetch_one(&mut *self.tx).await.map_err(turso_err)?;
        if !matches!(kind.as_deref(), None | Some("null" | "array")) {
            return Err(invalid_plan());
        }
        let mut metadata = ReceiptMetadata::empty();
        if count.unwrap_or(0) > 0 {
            let rows: Vec<(String, Option<i64>)> = native_turso::query_as(
                "SELECT json_quote(json_extract(value,'$.id')),json_array_length(value,'$.embedding') FROM json_each(?,'$.memories[0].chunks')",
            ).bind(plan).fetch_all(&mut *self.tx).await.map_err(turso_err)?;
            metadata.chunks = rows
                .into_iter()
                .map(|(id_json, dimension)| ReceiptChunk { id_json, dimension })
                .collect();
        }
        Ok(metadata)
    }

    async fn publication_visible_count(
        &mut self,
        receipt: &IngestReceipt,
        profile: &str,
        generation: i64,
        expected: &ExpectedChunks,
    ) -> StorageResult<i64> {
        // The earlier asset/source reads already proved currentness, deletion
        // and source activity. This native predicate establishes their exact
        // version association without decoding extra stored fields.
        let root: bool = native_turso::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM oc_versions WHERE tenant_id=? AND workspace_id=? AND asset_id=? AND version=? AND source_event_id=?)",
        ).bind(self.scope.tenant_id).bind(self.scope.workspace_id)
            .bind(receipt.asset_id).bind(receipt.target_version).bind(receipt.source_event_id)
            .fetch_one(&mut *self.tx).await.map_err(turso_err)?;
        let rows = native_turso::query(
            "SELECT id,hex(id) AS chunk_hex,json_extract(locator,'$.parser') IS 'memory-semantics:v1' AS semantic,CASE WHEN json_extract(locator,'$.parser') IS 'memory-semantics:v1' THEN json_type(locator,'$.support_sources') END AS support_type,CASE WHEN json_extract(locator,'$.parser') IS 'memory-semantics:v1' THEN json_quote(json_extract(locator,'$.support_sources')) END AS support_json FROM oc_chunks WHERE tenant_id=? AND workspace_id=? AND asset_id=? AND version=? AND ?",
        ).bind(self.scope.tenant_id).bind(self.scope.workspace_id)
            .bind(receipt.asset_id).bind(receipt.target_version).bind(root)
            .fetch_all(&mut *self.tx).await.map_err(turso_err)?;
        // This is per-chunk local S2 support only. In particular, do not add
        // whole-version, Learning, semantic-status or identity predicates.
        let support = self.publication_chunk_support(&rows).await?;
        let mut chunks = Vec::new();
        for (index, row) in rows.into_iter().enumerate() {
            if support[index] {
                chunks.push(PublicationChunk {
                    id: row.try_get("id").map_err(turso_err)?,
                    hex_id: row.try_get("chunk_hex").map_err(turso_err)?,
                });
            }
        }
        let mut requests = Vec::new();
        for (index, chunk) in chunks.iter().enumerate() {
            if let Some(dimensions) = expected.get(&chunk.hex_id) {
                for dimension in dimensions {
                    requests.push((index, chunk.id.clone(), *dimension));
                }
            }
        }
        let indexed = self
            .publication_index_matches(&requests, profile, generation)
            .await?;
        let vector = self
            .publication_ledger_matches(&chunks, receipt, generation, "vector")
            .await?;
        let full_text = self
            .publication_ledger_matches(&chunks, receipt, generation, "full_text")
            .await?;
        Ok((0..chunks.len())
            .filter(|index| {
                indexed.contains(index) && vector.contains(index) && full_text.contains(index)
            })
            .count() as i64)
    }

    async fn publication_chunk_support(
        &mut self,
        rows: &[native_turso::Row],
    ) -> StorageResult<Vec<bool>> {
        let mut visible = vec![true; rows.len()];
        let mut members = Vec::new();
        for (index, row) in rows.iter().enumerate() {
            if !row.try_get::<bool>("semantic").map_err(turso_err)? {
                continue;
            }
            let Some(kind) = row
                .try_get::<Option<&str>>("support_type")
                .map_err(turso_err)?
            else {
                continue;
            };
            let text: &str = row.try_get("support_json").map_err(turso_err)?;
            members.extend(
                support_members(kind, text)?
                    .into_iter()
                    .map(|value| (index, value)),
            );
        }
        let mut normalized = Vec::new();
        for batch in members.chunks(READ_KEYS) {
            let values: Vec<_> = batch.iter().map(|(_, value)| value.as_str()).collect();
            let rows = native_turso::query(
                "SELECT key,upper(replace(value,'-','')) AS source FROM json_each(?)",
            )
            .bind(format!("[{}]", values.join(",")))
            .fetch_all(&mut *self.tx)
            .await
            .map_err(turso_err)?;
            for row in rows {
                let offset: usize = row.try_get("key").map_err(turso_err)?;
                let index = batch.get(offset).map(|(index, _)| *index).ok_or_else(|| {
                    StorageError::Backend("invalid normalized support index".into())
                })?;
                if let Some(source) = row.try_get::<Option<String>>("source").map_err(turso_err)? {
                    normalized.push((index, source));
                } else {
                    visible[index] = false;
                }
            }
        }
        let mut sources: Vec<_> = normalized
            .iter()
            .map(|(_, source)| source.clone())
            .collect();
        sources.sort_unstable();
        sources.dedup();
        let mut active = HashSet::new();
        for batch in sources.chunks(READ_KEYS) {
            let sql = format!(
                "SELECT hex(id) AS source FROM oc_events WHERE tenant_id=? AND workspace_id=? AND state='active' AND hex(id) IN ({})",
                vec!["?"; batch.len()].join(","),
            );
            let mut query = native_turso::query_scalar::<String>(&sql)
                .bind(self.scope.tenant_id)
                .bind(self.scope.workspace_id);
            for source in batch {
                query = query.bind(source);
            }
            active.extend(query.fetch_all(&mut *self.tx).await.map_err(turso_err)?);
        }
        for (index, source) in normalized {
            if !active.contains(&source) {
                visible[index] = false;
            }
        }
        Ok(visible)
    }

    async fn publication_index_matches(
        &mut self,
        requests: &[(usize, AssociationKey, i64)],
        profile: &str,
        generation: i64,
    ) -> StorageResult<HashSet<usize>> {
        let mut matched = HashSet::new();
        // Prepare even for an empty earlier relation, as the old statement did.
        for batch_index in 0..requests.len().div_ceil(PUBLICATION_SLOTS).max(1) {
            let start = batch_index * PUBLICATION_SLOTS;
            let batch = &requests[start..(start + PUBLICATION_SLOTS).min(requests.len())];
            let mut query = native_turso::query(PUBLICATION_INDEX);
            for slot in 0..PUBLICATION_SLOTS {
                query = query
                    .bind(batch.get(slot).map(|(_, id, _)| id.clone()))
                    .bind(batch.get(slot).map(|(_, _, dimension)| *dimension));
            }
            query = query
                .bind(self.scope.tenant_id)
                .bind(self.scope.workspace_id)
                .bind(profile)
                .bind(generation);
            for slot in 0..PUBLICATION_SLOTS {
                query = query
                    .bind(batch.get(slot).map(|(_, id, _)| id.clone()))
                    .bind(batch.get(slot).map(|(_, _, dimension)| *dimension));
            }
            for row in query.fetch_all(&mut *self.tx).await.map_err(turso_err)? {
                for (slot, (index, _, _)) in batch.iter().enumerate() {
                    if row.try_get::<Option<bool>>(slot).map_err(turso_err)? == Some(true) {
                        matched.insert(*index);
                    }
                }
            }
        }
        Ok(matched)
    }

    async fn publication_ledger_matches(
        &mut self,
        chunks: &[PublicationChunk],
        receipt: &IngestReceipt,
        generation: i64,
        surface: &str,
    ) -> StorageResult<HashSet<usize>> {
        let mut matched = HashSet::new();
        for batch_index in 0..chunks.len().div_ceil(PUBLICATION_SLOTS).max(1) {
            let start = batch_index * PUBLICATION_SLOTS;
            let batch = &chunks[start..(start + PUBLICATION_SLOTS).min(chunks.len())];
            let mut query = native_turso::query(PUBLICATION_LEDGER);
            for slot in 0..PUBLICATION_SLOTS {
                query = query.bind(batch.get(slot).map(|chunk| chunk.id.clone()));
            }
            query = query
                .bind(self.scope.tenant_id)
                .bind(self.scope.workspace_id)
                .bind(receipt.source_event_id)
                .bind(receipt.target_version)
                .bind(generation)
                .bind(surface);
            for slot in 0..PUBLICATION_SLOTS {
                query = query.bind(batch.get(slot).map(|chunk| chunk.id.clone()));
            }
            for row in query.fetch_all(&mut *self.tx).await.map_err(turso_err)? {
                for slot in 0..batch.len() {
                    if row.try_get::<Option<bool>>(slot).map_err(turso_err)? == Some(true) {
                        matched.insert(start + slot);
                    }
                }
            }
        }
        Ok(matched)
    }
}

const PUBLICATION_INDEX: &str = "SELECT (artifact_id=? AND dimension=?) AS m0,(artifact_id=? AND dimension=?) AS m1,(artifact_id=? AND dimension=?) AS m2,(artifact_id=? AND dimension=?) AS m3,(artifact_id=? AND dimension=?) AS m4,(artifact_id=? AND dimension=?) AS m5,(artifact_id=? AND dimension=?) AS m6,(artifact_id=? AND dimension=?) AS m7 FROM oc_index_entries WHERE tenant_id=? AND workspace_id=? AND field='content' AND state='ready' AND model_id=? AND generation=? AND ((artifact_id=? AND dimension=?) OR (artifact_id=? AND dimension=?) OR (artifact_id=? AND dimension=?) OR (artifact_id=? AND dimension=?) OR (artifact_id=? AND dimension=?) OR (artifact_id=? AND dimension=?) OR (artifact_id=? AND dimension=?) OR (artifact_id=? AND dimension=?))";

const PUBLICATION_LEDGER: &str = "SELECT (artifact_id=?) AS m0,(artifact_id=?) AS m1,(artifact_id=?) AS m2,(artifact_id=?) AS m3,(artifact_id=?) AS m4,(artifact_id=?) AS m5,(artifact_id=?) AS m6,(artifact_id=?) AS m7 FROM oc_artifact_ledger WHERE tenant_id=? AND workspace_id=? AND source_id=? AND version=? AND generation=? AND surface=? AND artifact_type='chunk' AND state='committed' AND ((artifact_id=?) OR (artifact_id=?) OR (artifact_id=?) OR (artifact_id=?) OR (artifact_id=?) OR (artifact_id=?) OR (artifact_id=?) OR (artifact_id=?))";

#[cfg(test)]
mod tests {
    use super::*;

    async fn database() -> (tempfile::TempDir, Pool, TursoTx, IngestReceipt) {
        let dir = tempfile::tempdir().unwrap();
        let pool = Pool::open(dir.path().join("publication-batches.db"))
            .await
            .unwrap();
        for sql in [
            "CREATE TABLE oc_jobs(tenant_id BLOB,workspace_id BLOB,id BLOB,payload TEXT,operation TEXT,updated_at INTEGER)",
            "CREATE TABLE oc_versions(tenant_id BLOB,workspace_id BLOB,asset_id BLOB,version INTEGER,source_event_id BLOB)",
            "CREATE TABLE oc_chunks(tenant_id BLOB,workspace_id BLOB,id BLOB,asset_id BLOB,version INTEGER,locator TEXT)",
            "CREATE TABLE oc_events(tenant_id BLOB,workspace_id BLOB,id BLOB,state TEXT)",
            "CREATE TABLE oc_index_entries(tenant_id BLOB,workspace_id BLOB,artifact_id BLOB,field TEXT,model_id TEXT,dimension INTEGER,generation INTEGER,state TEXT)",
            "CREATE TABLE oc_artifact_ledger(tenant_id BLOB,workspace_id BLOB,source_id BLOB,version INTEGER,artifact_type TEXT,artifact_id BLOB,surface TEXT,generation INTEGER,state TEXT)",
        ] {
            native_turso::query(sql).execute(&pool).await.unwrap();
        }
        let tx = TursoTx {
            tx: pool.begin_with("BEGIN IMMEDIATE").await.unwrap(),
            scope: Scope {
                tenant_id: Uuid::new_v4(),
                workspace_id: Uuid::new_v4(),
            },
            principal_id: Uuid::new_v4(),
            required: Vec::new(),
            command: None,
        };
        let receipt = IngestReceipt {
            batch_id: "batch".into(),
            session_id: "session".into(),
            asset_id: Uuid::new_v4(),
            source_event_id: Uuid::new_v4(),
            job_id: Uuid::new_v4(),
            target_version: 1,
            accepted_spec: AcceptedMessageSpec::raw_only("fixture"),
        };
        (dir, pool, tx, receipt)
    }

    #[tokio::test]
    async fn publication_metadata_preserves_ids_and_dimensions_at_batch_boundaries() {
        let (_dir, pool, mut tx, _) = database().await;
        for count in [0usize, 7, 8, 9, 31, 32, 33, 127, 128, 129] {
            let chunks: Vec<_> = (0..count)
                .map(|index| {
                    let dimension = if index == 0 { 4096 } else { 1 };
                    json!({"id":format!("id-{index}"),"embedding":vec![0; dimension]})
                })
                .collect();
            let plan = json!({"memories":[{"chunks":chunks}]}).to_string();
            let metadata = tx.derive_receipt_metadata(&plan).await.unwrap();
            assert_eq!(metadata.chunks.len(), count);
            assert_eq!(
                tx.publication_distinct_ids(&metadata).await.unwrap(),
                count as i64
            );
            let expected = tx.publication_expected_chunks(&metadata).await.unwrap();
            assert_eq!(expected.len(), count);
            for index in 0..count {
                assert_eq!(
                    expected[&format!("ID{index}")],
                    HashSet::from([if index == 0 { 4096 } else { 1 }])
                );
            }
        }
        // Duplicates, NULL IDs and numeric/text identities retain native
        // DISTINCT. Normalization preserves every dimension witness.
        let chunks = r#"[{"id":"a-b","embedding":[0]},{"id":"AB","embedding":[0,0]},{"id":1},{"id":1.0},{"id":true},{"id":"1"},{"id":null},{},{"id":"a-b"}]"#;
        let plan = format!("{{\"memories\":[{{\"chunks\":{chunks}}}]}}");
        let metadata = tx.derive_receipt_metadata(&plan).await.unwrap();
        assert_eq!(metadata.chunks.len(), 9);
        assert_eq!(tx.publication_distinct_ids(&metadata).await.unwrap(), 4);
        assert_eq!(
            tx.publication_expected_chunks(&metadata).await.unwrap()["AB"],
            HashSet::from([1, 2])
        );
        for plan in [r#"{}"#, r#"{"memories":[{}]}"#, r#"{"memories":[{"chunks":null}]}"#, r#"{"memories":[{"chunks":[]}]}"#] {
            assert_eq!(tx.derive_receipt_metadata(plan).await.unwrap(), ReceiptMetadata::empty());
        }
        tx.tx.rollback().await.unwrap();
        pool.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn publication_metadata_requires_single_native_id_tokens_and_dimensions() {
        let (_dir, pool, mut tx, _) = database().await;
        for token in ["", " ", "1,2", "null,\"injected\"", "\"unterminated", "not-json"] {
            let metadata = ReceiptMetadata {
                format: RECEIPT_METADATA_FORMAT.into(),
                chunks: vec![ReceiptChunk { id_json: token.into(), dimension: Some(1) }],
            };
            assert_invalid_plan(tx.publication_distinct_ids(&metadata).await.unwrap_err());
        }
        for token in ["null", "1", "1.0", "true", "{}", "[]", "\"a,b\"", " 1 "] {
            let metadata = ReceiptMetadata {
                format: RECEIPT_METADATA_FORMAT.into(),
                chunks: vec![ReceiptChunk { id_json: token.into(), dimension: None }],
            };
            assert_eq!(tx.publication_distinct_ids(&metadata).await.unwrap(), i64::from(token != "null"));
        }
        let missing_dimension = json!({"format":RECEIPT_METADATA_FORMAT,"chunks":[{"id_json":"null"}]}).to_string();
        assert_invalid_plan(ReceiptMetadata::decode(Some(&missing_dimension), 1).unwrap_err());
        let null_dimension = json!({"format":RECEIPT_METADATA_FORMAT,"chunks":[{"id_json":"null","dimension":null}]}).to_string();
        assert_eq!(ReceiptMetadata::decode(Some(&null_dimension), 1).unwrap().chunks[0].dimension, None);
        for unrelated in [None, Some("null"), Some("7"), Some("{}"), Some("[]"), Some("1e999")] {
            assert_eq!(ReceiptMetadata::decode(unrelated, 0).unwrap(), ReceiptMetadata::empty());
        }
        tx.tx.rollback().await.unwrap();
        pool.shutdown().await.unwrap();
    }

    async fn saved_plan(tx: &mut TursoTx, job: Uuid) -> Value {
        let value: String = native_turso::query_scalar(
            "SELECT json_extract(payload,'$.publication') FROM oc_jobs WHERE tenant_id=? AND workspace_id=? AND id=?",
        ).bind(tx.scope.tenant_id).bind(tx.scope.workspace_id).bind(job)
            .fetch_one(&mut *tx.tx).await.unwrap();
        serde_json::from_str(&value).unwrap()
    }

    fn assert_invalid_plan(error: StorageError) {
        assert!(matches!(error, StorageError::Unavailable(message) if message == "invalid message publication plan"));
    }

    #[tokio::test]
    async fn publication_save_validates_and_reuses_same_generation_metadata() {
        let (_dir, pool, mut tx, receipt) = database().await;
        native_turso::query("INSERT INTO oc_jobs VALUES(?,?,?,'{}','message_batch_ingest',0)")
            .bind(tx.scope.tenant_id).bind(tx.scope.workspace_id).bind(receipt.job_id)
            .execute(&mut *tx.tx).await.unwrap();
        let plan = json!({"generation":3,"memories":[{"chunks":[
            {"id":"a-b","embedding":[0]}, {"id":"AB","embedding":[0,0]},
            {"id":null}, {"id":1}, {"id":"1"}, {"id":true},
            {"id":{"a":1}}, {"id":"{\"a\":1}"},
            "{\"id\":\"encoded\",\"embedding\":[0,0,0]}"
        ]}]});
        let mut forged_fresh = plan.clone();
        forged_fresh["receipt_metadata"] = json!({"format":RECEIPT_METADATA_FORMAT,"chunks":[]});
        assert_invalid_plan(tx.save_publication(receipt.job_id, &forged_fresh).await.unwrap_err());
        tx.save_publication(receipt.job_id, &plan).await.unwrap();
        let prepared = saved_plan(&mut tx, receipt.job_id).await;
        let metadata = &prepared["receipt_metadata"];
        assert_eq!(metadata["format"], RECEIPT_METADATA_FORMAT);
        assert_eq!(metadata["chunks"].as_array().unwrap().len(), 9);
        assert_eq!(metadata["chunks"][0], json!({"id_json":"\"a-b\"","dimension":1}));
        assert_eq!(metadata["chunks"][2], json!({"id_json":"null","dimension":null}));
        assert_eq!(metadata["chunks"][8], json!({"id_json":"\"encoded\"","dimension":3}));
        tx.save_publication(receipt.job_id, &prepared).await.unwrap();
        assert_eq!(saved_plan(&mut tx, receipt.job_id).await, prepared);
        // An omitted caller field can only reuse the existing witness after
        // native validation; it must never replace the membership or dimension.
        tx.save_publication(receipt.job_id, &plan).await.unwrap();
        assert_eq!(saved_plan(&mut tx, receipt.job_id).await, prepared);
        let mut different_id = plan.clone();
        different_id["memories"][0]["chunks"][0]["id"] = json!("replacement");
        let mut different_dimension = plan.clone();
        different_dimension["memories"][0]["chunks"][0]["embedding"] = json!([0, 0]);
        let mut forged = prepared.clone();
        forged["receipt_metadata"]["chunks"][0]["id_json"] = json!("\"replacement\"");
        for invalid in [different_id, different_dimension, forged] {
            assert_invalid_plan(tx.save_publication(receipt.job_id, &invalid).await.unwrap_err());
            assert_eq!(saved_plan(&mut tx, receipt.job_id).await, prepared);
        }
        // Missing current-generation metadata is corruption, not a backfill.
        native_turso::query("UPDATE oc_jobs SET payload=json_remove(payload,'$.publication.receipt_metadata')")
            .execute(&mut *tx.tx).await.unwrap();
        assert_invalid_plan(tx.save_publication(receipt.job_id, &plan).await.unwrap_err());
        assert_eq!(saved_plan(&mut tx, receipt.job_id).await, plan);
        // The worker does not reuse stale generations: ordinary reprepare may
        // replace the old plan without reviving its absent witness.
        let mut next_generation = plan.clone();
        next_generation["generation"] = json!(4);
        next_generation["memories"][0]["chunks"][0]["id"] = json!("next");
        tx.save_publication(receipt.job_id, &next_generation).await.unwrap();
        let next = saved_plan(&mut tx, receipt.job_id).await;
        assert_eq!(next["generation"], 4);
        assert_eq!(next["receipt_metadata"]["chunks"][0]["id_json"], "\"next\"");
        tx.tx.rollback().await.unwrap();
        pool.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn publication_witnesses_keep_raw_keys_multiplicity_and_local_support() {
        let (_dir, pool, mut tx, receipt) = database().await;
        let scope = tx.scope;
        native_turso::query("INSERT INTO oc_versions VALUES(?,?,?,?,?)")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(receipt.asset_id)
            .bind(receipt.target_version)
            .bind(receipt.source_event_id)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        let support = Uuid::new_v4();
        native_turso::query("INSERT INTO oc_events VALUES(?,?,?,'active')")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .bind(support)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        let mut expected = HashMap::new();
        // Short BLOBs are legal native keys in the old count predicate. They
        // must not acquire a UUID decoder while joining these scoped witnesses.
        for index in 0..129u16 {
            let id = index.to_be_bytes().to_vec();
            expected.insert(format!("{index:04X}"), HashSet::from([1, 2]));
            let locator = if index == 0 {
                json!({"parser":"memory-semantics:v1","support_sources":[support],"semantic_status":7}).to_string()
            } else {
                "{}".into()
            };
            native_turso::query("INSERT INTO oc_chunks VALUES(?,?,?,?,?,?)")
                .bind(scope.tenant_id)
                .bind(scope.workspace_id)
                .bind(&id)
                .bind(receipt.asset_id)
                .bind(receipt.target_version)
                .bind(locator)
                .execute(&mut *tx.tx)
                .await
                .unwrap();
            for _ in 0..2 {
                native_turso::query(
                    "INSERT INTO oc_index_entries VALUES(?,?,?,'content','fixture',2,3,'ready')",
                )
                .bind(scope.tenant_id)
                .bind(scope.workspace_id)
                .bind(&id)
                .execute(&mut *tx.tx)
                .await
                .unwrap();
            }
            for surface in ["vector", "full_text"] {
                native_turso::query(
                    "INSERT INTO oc_artifact_ledger VALUES(?,?,?,?,'chunk',?,?,3,'committed')",
                )
                .bind(scope.tenant_id)
                .bind(scope.workspace_id)
                .bind(receipt.source_event_id)
                .bind(receipt.target_version)
                .bind(&id)
                .bind(surface)
                .execute(&mut *tx.tx)
                .await
                .unwrap();
            }
        }
        assert_eq!(
            tx.publication_visible_count(&receipt, "fixture", 3, &expected)
                .await
                .unwrap(),
            129
        );
        native_turso::query("UPDATE oc_events SET state='retracted' WHERE id=?")
            .bind(support)
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        // Only the unsupported chunk disappears, not the whole version.
        assert_eq!(
            tx.publication_visible_count(&receipt, "fixture", 3, &expected)
                .await
                .unwrap(),
            128
        );
        native_turso::query("UPDATE oc_artifact_ledger SET generation=4 WHERE artifact_id=? AND surface='full_text'")
            .bind(vec![0u8, 1]).execute(&mut *tx.tx).await.unwrap();
        assert_eq!(
            tx.publication_visible_count(&receipt, "fixture", 3, &expected)
                .await
                .unwrap(),
            127
        );
        assert_eq!(
            tx.publication_visible_count(&receipt, "fixture", 4, &expected)
                .await
                .unwrap(),
            0
        );
        // A foreign root hides all chunks before malformed locator extraction.
        native_turso::query("UPDATE oc_versions SET source_event_id=?")
            .bind(Uuid::new_v4())
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        native_turso::query("UPDATE oc_chunks SET locator='{'")
            .execute(&mut *tx.tx)
            .await
            .unwrap();
        assert_eq!(
            tx.publication_visible_count(&receipt, "fixture", 3, &expected)
                .await
                .unwrap(),
            0
        );
        tx.tx.rollback().await.unwrap();
        pool.shutdown().await.unwrap();
    }
}
