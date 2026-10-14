//! Immutable, relational-only archives. No native artifacts or background jobs.
use super::read_model::{self, AssociationKey};
use super::*;
use crate::session::{
    ArchivedEvidence, EvidenceKind, FeedbackInput, FeedbackTarget, SessionTurnInput, UsedEvidence,
};
use std::collections::HashSet;

pub(super) const SCHEMA: &str = include_str!("../session-schema.sql");

pub(super) async fn available(conn: &mut Connection) -> StorageResult<bool> {
    let tables: bool = native_turso::query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name IN ('oc_session_turns','oc_session_turn_evidence','oc_feedback'))")
        .fetch_one(&mut *conn).await.map_err(turso_err)?;
    // Evaluate both original relations, including their prepare errors. This
    // is the existing global presence probe, not a complete schema validator.
    let sources: bool = native_turso::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM oc_events WHERE kind='session_turn')",
    )
    .fetch_one(conn)
    .await
    .map_err(turso_err)?;
    Ok(tables || sources)
}

fn decode<T: serde::de::DeserializeOwned>(s: &str) -> StorageResult<T> {
    serde_json::from_str(s).map_err(|_| StorageError::Unavailable("invalid session record".into()))
}
fn encode<T: serde::Serialize>(value: &T) -> StorageResult<String> {
    serde_json::to_string(value)
        .map_err(|_| StorageError::Unavailable("invalid session record".into()))
}

// Complete fixed templates share the native eight-request binder. Each reads
// one scoped relation; the largest statement has 50 bind parameters.
const ARCHIVE_ASSETS: &str = "SELECT id,(id=?) AS m0,(id=?) AS m1,(id=?) AS m2,(id=?) AS m3,(id=?) AS m4,(id=?) AS m5,(id=?) AS m6,(id=?) AS m7 FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND (NOT deleted) AND ((id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?))";

const ARCHIVE_VERSIONS: &str = "SELECT source_event_id,(asset_id=? AND version=?) AS m0,(asset_id=? AND version=?) AS m1,(asset_id=? AND version=?) AS m2,(asset_id=? AND version=?) AS m3,(asset_id=? AND version=?) AS m4,(asset_id=? AND version=?) AS m5,(asset_id=? AND version=?) AS m6,(asset_id=? AND version=?) AS m7 FROM oc_versions WHERE tenant_id=? AND workspace_id=? AND (1) AND ((asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?))";

const ARCHIVE_ROOTS: &str = "SELECT id,(id=?) AS m0,(id=?) AS m1,(id=?) AS m2,(id=?) AS m3,(id=?) AS m4,(id=?) AS m5,(id=?) AS m6,(id=?) AS m7 FROM oc_events WHERE tenant_id=? AND workspace_id=? AND (state='active') AND ((id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?))";

const ARCHIVE_CHUNKS: &str = "SELECT locator,(id=? AND asset_id=? AND version=?) AS m0,(id=? AND asset_id=? AND version=?) AS m1,(id=? AND asset_id=? AND version=?) AS m2,(id=? AND asset_id=? AND version=?) AS m3,(id=? AND asset_id=? AND version=?) AS m4,(id=? AND asset_id=? AND version=?) AS m5,(id=? AND asset_id=? AND version=?) AS m6,(id=? AND asset_id=? AND version=?) AS m7 FROM oc_chunks WHERE tenant_id=? AND workspace_id=? AND (1) AND ((id=? AND asset_id=? AND version=?) OR (id=? AND asset_id=? AND version=?) OR (id=? AND asset_id=? AND version=?) OR (id=? AND asset_id=? AND version=?) OR (id=? AND asset_id=? AND version=?) OR (id=? AND asset_id=? AND version=?) OR (id=? AND asset_id=? AND version=?) OR (id=? AND asset_id=? AND version=?))";

const ARCHIVE_CHUNK_OWNERS: &str = "SELECT artifact_id,(source_id=? AND version=? AND artifact_id=?) AS m0,(source_id=? AND version=? AND artifact_id=?) AS m1,(source_id=? AND version=? AND artifact_id=?) AS m2,(source_id=? AND version=? AND artifact_id=?) AS m3,(source_id=? AND version=? AND artifact_id=?) AS m4,(source_id=? AND version=? AND artifact_id=?) AS m5,(source_id=? AND version=? AND artifact_id=?) AS m6,(source_id=? AND version=? AND artifact_id=?) AS m7 FROM oc_artifact_owners WHERE tenant_id=? AND workspace_id=? AND (artifact_type='chunk' AND artifact_id=chunk_id) AND ((source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?))";

const ARCHIVE_ENTITY_OWNERS: &str = "SELECT artifact_id,(source_id=? AND version=? AND artifact_id=?) AS m0,(source_id=? AND version=? AND artifact_id=?) AS m1,(source_id=? AND version=? AND artifact_id=?) AS m2,(source_id=? AND version=? AND artifact_id=?) AS m3,(source_id=? AND version=? AND artifact_id=?) AS m4,(source_id=? AND version=? AND artifact_id=?) AS m5,(source_id=? AND version=? AND artifact_id=?) AS m6,(source_id=? AND version=? AND artifact_id=?) AS m7 FROM oc_artifact_owners WHERE tenant_id=? AND workspace_id=? AND (artifact_type='entity') AND ((source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?))";

const ARCHIVE_RELATION_OWNERS: &str = "SELECT artifact_id,(source_id=? AND version=? AND artifact_id=?) AS m0,(source_id=? AND version=? AND artifact_id=?) AS m1,(source_id=? AND version=? AND artifact_id=?) AS m2,(source_id=? AND version=? AND artifact_id=?) AS m3,(source_id=? AND version=? AND artifact_id=?) AS m4,(source_id=? AND version=? AND artifact_id=?) AS m5,(source_id=? AND version=? AND artifact_id=?) AS m6,(source_id=? AND version=? AND artifact_id=?) AS m7 FROM oc_artifact_owners WHERE tenant_id=? AND workspace_id=? AND (artifact_type='relation') AND ((source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?))";

const ARCHIVE_ENTITY_LEDGER: &str = "SELECT artifact_id,(source_id=? AND version=? AND artifact_id=?) AS m0,(source_id=? AND version=? AND artifact_id=?) AS m1,(source_id=? AND version=? AND artifact_id=?) AS m2,(source_id=? AND version=? AND artifact_id=?) AS m3,(source_id=? AND version=? AND artifact_id=?) AS m4,(source_id=? AND version=? AND artifact_id=?) AS m5,(source_id=? AND version=? AND artifact_id=?) AS m6,(source_id=? AND version=? AND artifact_id=?) AS m7 FROM oc_artifact_ledger WHERE tenant_id=? AND workspace_id=? AND (artifact_type='entity' AND surface='graph' AND state='committed') AND ((source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?))";

const ARCHIVE_RELATION_LEDGER: &str = "SELECT artifact_id,(source_id=? AND version=? AND artifact_id=?) AS m0,(source_id=? AND version=? AND artifact_id=?) AS m1,(source_id=? AND version=? AND artifact_id=?) AS m2,(source_id=? AND version=? AND artifact_id=?) AS m3,(source_id=? AND version=? AND artifact_id=?) AS m4,(source_id=? AND version=? AND artifact_id=?) AS m5,(source_id=? AND version=? AND artifact_id=?) AS m6,(source_id=? AND version=? AND artifact_id=?) AS m7 FROM oc_artifact_ledger WHERE tenant_id=? AND workspace_id=? AND (artifact_type='relation' AND surface='graph' AND state='committed') AND ((source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?) OR (source_id=? AND version=? AND artifact_id=?))";

fn association_key(value: impl native_turso::Encode) -> StorageResult<AssociationKey> {
    let value = native_turso::Encode::encode(value).map_err(turso_err)?;
    native_turso::Decode::decode(&value).map_err(turso_err)
}

#[derive(Default)]
struct ArchiveRow {
    root: Option<native_turso::Row>,
    chunk: Option<native_turso::Row>,
    chunk_owned: bool,
    graph_owned: bool,
}

/// One read-call snapshot; never retained across a write or another transaction.
struct ArchiveReadBatch {
    references: Vec<UsedEvidence>,
    rows: Vec<ArchiveRow>,
    roots_loaded: bool,
    chunks_loaded: bool,
    owners_loaded: bool,
    graph_loaded: bool,
    contents_loaded: bool,
    contents: HashMap<Uuid, native_turso::Row>,
    support: HashMap<(Uuid, i32), bool>,
}

impl TursoTx {
    async fn require_sessions(&mut self) -> StorageResult<()> {
        if !available(&mut self.tx).await? {
            return Err(StorageError::Unavailable(
                "session archives require a new database".into(),
            ));
        }
        Ok(())
    }

    /// Each relation is loaded only after the current reference reaches its
    /// original read boundary. In particular, missing chunks and invalid local
    /// support must not prepare owner/ledger statements. Raw rows defer decoding
    /// until consumption; native JSON guards remain in reference order.
    fn archive_metadata(references: &[UsedEvidence]) -> ArchiveReadBatch {
        ArchiveReadBatch {
            references: references.to_vec(),
            rows: references.iter().map(|_| ArchiveRow::default()).collect(),
            roots_loaded: false,
            chunks_loaded: false,
            owners_loaded: false,
            graph_loaded: false,
            contents_loaded: false,
            contents: HashMap::new(),
            support: HashMap::new(),
        }
    }

    async fn archive_roots(&mut self, batch: &mut ArchiveReadBatch) -> StorageResult<()> {
        if batch.roots_loaded || batch.references.is_empty() {
            return Ok(());
        }
        let keys = batch
            .references
            .iter()
            .map(|reference| Ok(vec![association_key(reference.asset_id)?]))
            .collect::<StorageResult<Vec<_>>>()?;
        let assets: HashSet<_> =
            read_model::associate_query(&mut self.tx, self.scope, ARCHIVE_ASSETS, 1, &keys)
                .await?
                .into_iter()
                .map(|(index, _)| index)
                .collect();
        let keys = batch
            .references
            .iter()
            .map(|reference| {
                Ok(vec![
                    association_key(reference.asset_id)?,
                    association_key(reference.asset_version)?,
                ])
            })
            .collect::<StorageResult<Vec<_>>>()?;
        let versions: Vec<_> =
            read_model::associate_query(&mut self.tx, self.scope, ARCHIVE_VERSIONS, 2, &keys)
                .await?
                .into_iter()
                .filter(|(index, _)| assets.contains(index))
                .collect();
        let keys = versions
            .iter()
            .map(|(_, row)| Ok(vec![row.try_get("source_event_id").map_err(turso_err)?]))
            .collect::<StorageResult<Vec<_>>>()?;
        // No current-version or source-kind predicate belongs to this layer.
        let active: HashSet<_> =
            read_model::associate_query(&mut self.tx, self.scope, ARCHIVE_ROOTS, 1, &keys)
                .await?
                .into_iter()
                .map(|(index, _)| index)
                .collect();
        for (index, (request, row)) in versions.into_iter().enumerate() {
            if active.contains(&index) && batch.rows[request].root.is_none() {
                batch.rows[request].root = Some(row);
            }
        }
        batch.roots_loaded = true;
        Ok(())
    }

    async fn archive_chunks(&mut self, batch: &mut ArchiveReadBatch) -> StorageResult<()> {
        if batch.chunks_loaded {
            return Ok(());
        }
        let selected: Vec<_> = batch
            .rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| row.root.is_some().then_some(index))
            .collect();
        if !selected.is_empty() {
            let keys = selected
                .iter()
                .map(|index| {
                    let reference = &batch.references[*index];
                    Ok(vec![
                        association_key(reference.chunk_id)?,
                        association_key(reference.asset_id)?,
                        association_key(reference.asset_version)?,
                    ])
                })
                .collect::<StorageResult<Vec<_>>>()?;
            for (index, row) in
                read_model::associate_query(&mut self.tx, self.scope, ARCHIVE_CHUNKS, 3, &keys)
                    .await?
            {
                batch.rows[selected[index]].chunk.get_or_insert(row);
            }
        }
        batch.chunks_loaded = true;
        Ok(())
    }

    async fn archive_owners(&mut self, batch: &mut ArchiveReadBatch) -> StorageResult<()> {
        if batch.owners_loaded {
            return Ok(());
        }
        let selected: Vec<_> = batch
            .rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| (row.root.is_some() && row.chunk.is_some()).then_some(index))
            .collect();
        if !selected.is_empty() {
            let keys = selected
                .iter()
                .map(|index| {
                    let reference = &batch.references[*index];
                    let root = batch.rows[*index]
                        .root
                        .as_ref()
                        .ok_or(StorageError::NotFound)?;
                    Ok(vec![
                        root.try_get("source_event_id").map_err(turso_err)?,
                        association_key(reference.asset_version)?,
                        association_key(reference.chunk_id)?,
                    ])
                })
                .collect::<StorageResult<Vec<_>>>()?;
            // artifact_id is already equal to the requested chunk UUID, so the
            // native artifact_id=chunk_id gate retains both original owner keys.
            for (index, _) in read_model::associate_query(
                &mut self.tx,
                self.scope,
                ARCHIVE_CHUNK_OWNERS,
                3,
                &keys,
            )
            .await?
            {
                batch.rows[selected[index]].chunk_owned = true;
            }
        }
        batch.owners_loaded = true;
        Ok(())
    }

    async fn archive_graph(&mut self, batch: &mut ArchiveReadBatch) -> StorageResult<()> {
        if batch.graph_loaded {
            return Ok(());
        }
        for (kind, owner_sql, ledger_sql) in [
            (
                EvidenceKind::Entity,
                ARCHIVE_ENTITY_OWNERS,
                ARCHIVE_ENTITY_LEDGER,
            ),
            (
                EvidenceKind::Relation,
                ARCHIVE_RELATION_OWNERS,
                ARCHIVE_RELATION_LEDGER,
            ),
        ] {
            let selected: Vec<_> = batch
                .references
                .iter()
                .enumerate()
                .filter_map(|(index, reference)| {
                    (reference.kind == kind && batch.rows[index].chunk_owned).then_some(index)
                })
                .collect();
            if selected.is_empty() {
                continue;
            }
            let keys = selected
                .iter()
                .map(|index| {
                    let reference = &batch.references[*index];
                    let root = batch.rows[*index]
                        .root
                        .as_ref()
                        .ok_or(StorageError::NotFound)?;
                    Ok(vec![
                        root.try_get("source_event_id").map_err(turso_err)?,
                        association_key(reference.asset_version)?,
                        association_key(reference.id)?,
                    ])
                })
                .collect::<StorageResult<Vec<_>>>()?;
            let owned: HashSet<_> =
                read_model::associate_query(&mut self.tx, self.scope, owner_sql, 3, &keys)
                    .await?
                    .into_iter()
                    .map(|(index, _)| index)
                    .collect();
            // The old graph predicate constrained neither generation nor the
            // graph owner's chunk_id; intersect its complete native key only.
            for (index, _) in
                read_model::associate_query(&mut self.tx, self.scope, ledger_sql, 3, &keys).await?
            {
                batch.rows[selected[index]].graph_owned = owned.contains(&index);
            }
        }
        batch.graph_loaded = true;
        Ok(())
    }

    async fn archive_contents(&mut self, batch: &mut ArchiveReadBatch) -> StorageResult<()> {
        if batch.contents_loaded {
            return Ok(());
        }
        let mut sources = Vec::new();
        for row in &batch.rows {
            if let Some(root) = &row.root
                && let Ok(source) = root.try_get::<Uuid>("source_event_id")
            {
                sources.push(source);
            }
            // These are prefetch hints, never visibility decisions. Strict
            // locator/provenance decoding still occurs at the original boundary.
            if let Some(chunk) = &row.chunk
                && let Ok(text) = chunk.try_get::<String>("locator")
                && let Ok(locator) = decode::<Value>(&text)
                && locator["parser"] == crate::memory_semantics::PARSER
                && let Ok(source) =
                    serde_json::from_value::<Uuid>(locator["source_event_id"].clone())
            {
                sources.push(source);
            }
        }
        if !sources.is_empty() {
            for row in read_model::rows_by_ids(
                &mut self.tx,
                self.scope,
                "oc_events",
                "id,content,state='active' AS active",
                "id",
                &sources,
            )
            .await?
            {
                if row.try_get::<Option<bool>>("active").map_err(turso_err)? == Some(true) {
                    batch
                        .contents
                        .insert(row.try_get("id").map_err(turso_err)?, row);
                }
            }
        }
        batch.contents_loaded = true;
        Ok(())
    }

    async fn archive_from_batch(
        &mut self,
        batch: &mut ArchiveReadBatch,
        index: usize,
        reference: &UsedEvidence,
        evidence_ref_id: Uuid,
    ) -> StorageResult<ArchivedEvidence> {
        reference
            .validate()
            .map_err(StorageError::InvalidArgument)?;
        self.archive_roots(batch).await?;
        let root: Uuid = batch.rows[index]
            .root
            .as_ref()
            .ok_or(StorageError::NotFound)?
            .try_get("source_event_id")
            .map_err(turso_err)?;
        let version = (reference.asset_id, reference.asset_version);
        let supported = if let Some(supported) = batch.support.get(&version) {
            *supported
        } else {
            let supported = self
                .semantic_version_visible_local(version.0, version.1)
                .await?;
            batch.support.insert(version, supported);
            supported
        };
        if !supported {
            return Err(StorageError::NotFound);
        }
        self.archive_chunks(batch).await?;
        let locator: String = batch.rows[index]
            .chunk
            .as_ref()
            .ok_or(StorageError::NotFound)?
            .try_get("locator")
            .map_err(turso_err)?;
        let locator: Value = decode(&locator)?;
        self.archive_owners(batch).await?;
        if !batch.rows[index].chunk_owned {
            return Err(StorageError::NotFound);
        }
        if reference.kind != EvidenceKind::Chunk {
            self.archive_graph(batch).await?;
            if !batch.rows[index].graph_owned {
                return Err(StorageError::NotFound);
            }
        }
        let (source_event_id, source_version) =
            if locator["parser"] == crate::memory_semantics::PARSER {
                let source: Uuid = serde_json::from_value(locator["source_event_id"].clone())
                    .map_err(|_| StorageError::Unavailable("invalid semantic provenance".into()))?;
                let version: i32 = serde_json::from_value(locator["source_version"].clone())
                    .map_err(|_| StorageError::Unavailable("invalid semantic provenance".into()))?;
                if version <= 0 {
                    return Err(StorageError::Unavailable(
                        "invalid semantic provenance".into(),
                    ));
                }
                (source, version)
            } else {
                (root, reference.asset_version)
            };
        self.archive_contents(batch).await?;
        for source in [source_event_id, root] {
            // event_content(..., true) decoded String even though only its
            // existence was used. Keep that error, in member-then-root order.
            batch
                .contents
                .get(&source)
                .ok_or(StorageError::NotFound)?
                .try_get::<String>("content")
                .map_err(turso_err)?;
        }
        Ok(ArchivedEvidence {
            evidence_ref_id,
            reference: reference.clone(),
            source_event_id,
            source_version,
            version_source_event_id: root,
            locator,
        })
    }

    /// Exact historical provenance, without the public writer or Learning guard.
    pub(super) async fn archive_evidence_local(
        &mut self,
        reference: &UsedEvidence,
        evidence_ref_id: Uuid,
    ) -> StorageResult<ArchivedEvidence> {
        reference
            .validate()
            .map_err(StorageError::InvalidArgument)?;
        let mut batch = Self::archive_metadata(std::slice::from_ref(reference));
        self.archive_from_batch(&mut batch, 0, reference, evidence_ref_id)
            .await
    }

    async fn turn_evidence(
        &mut self,
        session: Uuid,
        turn: Uuid,
    ) -> StorageResult<Vec<ArchivedEvidence>> {
        let rows: Vec<String> = native_turso::query_scalar("SELECT evidence FROM oc_session_turn_evidence WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=? ORDER BY ordinal LIMIT 65")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
            .fetch_all(&mut *self.tx).await.map_err(turso_err)?;
        if rows.len() > crate::session::MAX_USED_EVIDENCE {
            return Err(StorageError::NotFound);
        }
        rows.iter().map(|s| decode(s)).collect()
    }

    async fn archive_evidence(
        &mut self,
        reference: &UsedEvidence,
        evidence_ref_id: Uuid,
    ) -> StorageResult<ArchivedEvidence> {
        let evidence = self
            .archive_evidence_local(reference, evidence_ref_id)
            .await?;
        self.require_archived_evidence_visible(&evidence).await?;
        Ok(evidence)
    }

    /// Validate exact immutable provenance without following Learning edges.
    /// The iterative Learning guard is the only caller which may use this
    /// helper without the public writer-only archive permission check.
    pub(super) async fn require_turn_visible_local(
        &mut self,
        session: Uuid,
        turn: Uuid,
    ) -> StorageResult<(Uuid, Vec<ArchivedEvidence>)> {
        self.require_sessions().await?;
        let turn_row = native_turso::query("SELECT source_event_id FROM oc_session_turns WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
            .fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        let keys = turn_row
            .as_ref()
            .map(|row| -> StorageResult<Vec<AssociationKey>> {
                Ok(vec![row.try_get("source_event_id").map_err(turso_err)?])
            })
            .transpose()?
            .into_iter()
            .collect::<Vec<_>>();
        let active =
            read_model::associate_query(&mut self.tx, self.scope, ARCHIVE_ROOTS, 1, &keys).await?;
        if active.is_empty() {
            return Err(StorageError::NotFound);
        }
        let source = turn_row
            .ok_or(StorageError::NotFound)?
            .try_get("source_event_id")
            .map_err(turso_err)?;
        let archived = self.turn_evidence(session, turn).await?;
        let references: Vec<_> = archived
            .iter()
            .map(|evidence| evidence.reference.clone())
            .collect();
        let mut batch = Self::archive_metadata(&references);
        for (index, evidence) in archived.iter().enumerate() {
            let current = self
                .archive_from_batch(
                    &mut batch,
                    index,
                    &evidence.reference,
                    evidence.evidence_ref_id,
                )
                .await?;
            if current != *evidence {
                return Err(StorageError::NotFound);
            }
        }
        Ok((source, archived))
    }

    pub async fn require_turn_visible(&mut self, session: Uuid, turn: Uuid) -> StorageResult<Uuid> {
        self.check_permission(Permission::Write).await?;
        let (source, evidence) = self.require_turn_visible_local(session, turn).await?;
        self.require_archived_evidence_set_visible(&evidence)
            .await?;
        Ok(source)
    }

    /// Also gate the existing raw-source endpoint, so it cannot bypass withdrawal.
    pub(super) async fn require_session_source_visible(
        &mut self,
        source: Uuid,
    ) -> StorageResult<()> {
        if !available(&mut self.tx).await? {
            return Err(StorageError::NotFound);
        }
        let turn: Option<(Uuid, Uuid)> = native_turso::query_as("SELECT session_id,turn_id FROM oc_session_turns WHERE tenant_id=? AND workspace_id=? AND source_event_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(source)
            .fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        let (session, turn) = turn.ok_or(StorageError::NotFound)?;
        self.require_turn_visible(session, turn).await?;
        Ok(())
    }

    pub async fn archive_turn(
        &mut self,
        session: Uuid,
        input: &SessionTurnInput,
        request_hash: &str,
    ) -> StorageResult<Value> {
        self.check_permission(Permission::Write).await?;
        self.require_sessions().await?;
        input.validate().map_err(StorageError::InvalidArgument)?;
        if session.is_nil() {
            return Err(StorageError::InvalidArgument("session_id must be non-nil"));
        }
        let existing: Option<(Uuid, String)> = native_turso::query_as("SELECT source_event_id,request_hash FROM oc_session_turns WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(input.turn_id)
            .fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        let source = if let Some((source, hash)) = existing {
            if hash != request_hash {
                return Err(StorageError::Conflict("turn is immutable".into()));
            }
            self.require_turn_visible(session, input.turn_id).await?;
            source
        } else {
            let mut evidence = Vec::new();
            if input.used_evidence.len() == 1 {
                evidence.push(
                    self.archive_evidence(&input.used_evidence[0], Uuid::new_v4())
                        .await?,
                );
            } else {
                let mut batch = Self::archive_metadata(&input.used_evidence);
                for (index, reference) in input.used_evidence.iter().enumerate() {
                    let item = self
                        .archive_from_batch(&mut batch, index, reference, Uuid::new_v4())
                        .await?;
                    self.require_archived_evidence_visible(&item).await?;
                    evidence.push(item);
                }
            }
            // Enforce the closure bound over the whole turn, not separately
            // for each reference before writing an unreadable archive.
            self.require_archived_evidence_set_visible(&evidence)
                .await?;
            let source = self
                .create_event(
                    "session_turn",
                    &json!({"question":input.question,"answer":input.answer}).to_string(),
                    None,
                )
                .await?;
            native_turso::query("INSERT INTO oc_session_turns(tenant_id,workspace_id,session_id,turn_id,source_event_id,request_hash,created_by,created_at) VALUES(?,?,?,?,?,?,?,?)")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(input.turn_id)
                .bind(source).bind(request_hash).bind(self.principal_id).bind(now_ms()).execute(&mut *self.tx).await.map_err(turso_err)?;
            for (ordinal, item) in evidence.iter().enumerate() {
                native_turso::query("INSERT INTO oc_session_turn_evidence(tenant_id,workspace_id,session_id,turn_id,evidence_ref_id,ordinal,evidence) VALUES(?,?,?,?,?,?,?)")
                    .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(input.turn_id)
                    .bind(item.evidence_ref_id).bind(ordinal as i32).bind(encode(item)?).execute(&mut *self.tx).await.map_err(turso_err)?;
            }
            self.audit("session.turn_archived", input.turn_id, json!({"session_id":session,"source_event_id":source,"evidence_count":evidence.len()})).await?;
            source
        };
        Ok(
            json!({"session_id":session,"turn_id":input.turn_id,"source_event_id":source,"state":"archived"}),
        )
    }

    pub async fn session_turn(&mut self, session: Uuid, turn: Uuid) -> StorageResult<Value> {
        let source = self.require_turn_visible(session, turn).await?;
        let body = self
            .event_content(source, true)
            .await?
            .ok_or(StorageError::NotFound)?;
        let body: Value = decode(&body)?;
        let (created_by, created_at): (Uuid, i64) = native_turso::query_as("SELECT created_by,created_at FROM oc_session_turns WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
            .fetch_one(&mut *self.tx).await.map_err(turso_err)?;
        let evidence = self.turn_evidence(session, turn).await?;
        let feedback = self.feedback_rows(session, turn).await?;
        Ok(
            json!({"session_id":session,"turn_id":turn,"source_event_id":source,"question":body["question"],"answer":body["answer"],"used_evidence":evidence,"feedback":feedback,"created_by":created_by,"created_at":created_at}),
        )
    }

    async fn feedback_rows(&mut self, session: Uuid, turn: Uuid) -> StorageResult<Vec<Value>> {
        let rows: Vec<(Uuid, String, i64, i64)> = native_turso::query_as("SELECT principal_id,feedback,created_at,feedback_sequence FROM oc_feedback WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=? ORDER BY feedback_sequence")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
            .fetch_all(&mut *self.tx).await.map_err(turso_err)?;
        rows.into_iter()
            .map(|(principal, body, created_at, sequence)| {
                let mut value: Value = decode(&body)?;
                value["created_by"] = json!(principal);
                value["created_at"] = json!(created_at);
                value["principal_id"] = json!(principal);
                value["feedback_sequence"] = json!(sequence);
                Ok(value)
            })
            .collect()
    }

    pub async fn session_feedback(&mut self, session: Uuid, turn: Uuid) -> StorageResult<Value> {
        self.require_turn_visible(session, turn).await?;
        Ok(
            json!({"session_id":session,"turn_id":turn,"feedback":self.feedback_rows(session, turn).await?}),
        )
    }

    pub async fn record_feedback(
        &mut self,
        session: Uuid,
        turn: Uuid,
        input: &FeedbackInput,
        request_hash: &str,
    ) -> StorageResult<Value> {
        input.validate().map_err(StorageError::InvalidArgument)?;
        if session.is_nil() || turn.is_nil() {
            return Err(StorageError::InvalidArgument(
                "session and turn identifiers must be non-nil",
            ));
        }
        self.require_turn_visible(session, turn).await?;
        if let FeedbackTarget::Evidence { evidence_ref_id } = &input.target {
            let found: bool = native_turso::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_session_turn_evidence WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=? AND evidence_ref_id=?)")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn).bind(evidence_ref_id)
                .fetch_one(&mut *self.tx).await.map_err(turso_err)?;
            if !found {
                return Err(StorageError::NotFound);
            }
        }
        let existing: Option<(String, Uuid, Uuid, i64)> = native_turso::query_as("SELECT request_hash,session_id,turn_id,feedback_sequence FROM oc_feedback WHERE tenant_id=? AND workspace_id=? AND principal_id=? AND feedback_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(self.principal_id).bind(input.feedback_id)
            .fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        let sequence = if let Some((hash, stored_session, stored_turn, sequence)) = existing {
            if hash != request_hash || stored_session != session || stored_turn != turn {
                return Err(StorageError::Conflict("feedback is immutable".into()));
            }
            sequence
        } else {
            let count: i64 = native_turso::query_scalar("SELECT count(*) FROM oc_feedback WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=?")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
                .fetch_one(&mut *self.tx).await.map_err(turso_err)?;
            if count >= 256 {
                return Err(StorageError::Conflict("turn feedback limit reached".into()));
            }
            let sequence: i64 = native_turso::query_scalar("SELECT COALESCE(MAX(feedback_sequence),0)+1 FROM oc_feedback WHERE tenant_id=? AND workspace_id=?")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id)
                .fetch_one(&mut *self.tx).await.map_err(turso_err)?;
            native_turso::query("INSERT INTO oc_feedback(tenant_id,workspace_id,principal_id,feedback_id,session_id,turn_id,request_hash,feedback,created_at,feedback_sequence) VALUES(?,?,?,?,?,?,?,?,?,?)")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(self.principal_id).bind(input.feedback_id)
                .bind(session).bind(turn).bind(request_hash).bind(encode(input)?).bind(now_ms()).bind(sequence).execute(&mut *self.tx).await.map_err(turso_err)?;
            self.audit(
                "session.feedback_recorded",
                input.feedback_id,
                json!({"session_id":session,"turn_id":turn,"feedback_sequence":sequence}),
            )
            .await?;
            sequence
        };
        Ok(
            json!({"session_id":session,"turn_id":turn,"feedback_id":input.feedback_id,"principal_id":self.principal_id,"feedback_sequence":sequence,"state":"recorded"}),
        )
    }
}
