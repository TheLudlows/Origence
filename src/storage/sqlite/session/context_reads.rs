//! Context-only, call-local raw session metadata. C's archive validators remain
//! ordered and authoritative; prefetch hints never authorize an archive.
use super::*;

const TURN_ROWS: &str = "SELECT source_event_id,(session_id=? AND turn_id=?) AS m0,(session_id=? AND turn_id=?) AS m1,(session_id=? AND turn_id=?) AS m2,(session_id=? AND turn_id=?) AS m3,(session_id=? AND turn_id=?) AS m4,(session_id=? AND turn_id=?) AS m5,(session_id=? AND turn_id=?) AS m6,(session_id=? AND turn_id=?) AS m7 FROM oc_session_turns WHERE tenant_id=? AND workspace_id=? AND (1) AND ((session_id=? AND turn_id=?) OR (session_id=? AND turn_id=?) OR (session_id=? AND turn_id=?) OR (session_id=? AND turn_id=?) OR (session_id=? AND turn_id=?) OR (session_id=? AND turn_id=?) OR (session_id=? AND turn_id=?) OR (session_id=? AND turn_id=?))";
const TURN_EVENTS: &str = "SELECT id,(id=?) AS m0,(id=?) AS m1,(id=?) AS m2,(id=?) AS m3,(id=?) AS m4,(id=?) AS m5,(id=?) AS m6,(id=?) AS m7 FROM oc_events WHERE tenant_id=? AND workspace_id=? AND (state='active') AND ((id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?))";
const TURN_EVIDENCE: &str = "SELECT evidence,ordinal,(session_id=? AND turn_id=? AND (? IS NULL OR ordinal>?)) AS m0,(session_id=? AND turn_id=? AND (? IS NULL OR ordinal>?)) AS m1,(session_id=? AND turn_id=? AND (? IS NULL OR ordinal>?)) AS m2,(session_id=? AND turn_id=? AND (? IS NULL OR ordinal>?)) AS m3,(session_id=? AND turn_id=? AND (? IS NULL OR ordinal>?)) AS m4,(session_id=? AND turn_id=? AND (? IS NULL OR ordinal>?)) AS m5,(session_id=? AND turn_id=? AND (? IS NULL OR ordinal>?)) AS m6,(session_id=? AND turn_id=? AND (? IS NULL OR ordinal>?)) AS m7 FROM oc_session_turn_evidence WHERE tenant_id=? AND workspace_id=? AND (1) AND ((session_id=? AND turn_id=? AND (? IS NULL OR ordinal>?)) OR (session_id=? AND turn_id=? AND (? IS NULL OR ordinal>?)) OR (session_id=? AND turn_id=? AND (? IS NULL OR ordinal>?)) OR (session_id=? AND turn_id=? AND (? IS NULL OR ordinal>?)) OR (session_id=? AND turn_id=? AND (? IS NULL OR ordinal>?)) OR (session_id=? AND turn_id=? AND (? IS NULL OR ordinal>?)) OR (session_id=? AND turn_id=? AND (? IS NULL OR ordinal>?)) OR (session_id=? AND turn_id=? AND (? IS NULL OR ordinal>?))) ORDER BY ordinal LIMIT 128";

const TURN_DATES: &str = "SELECT created_at,(session_id=? AND turn_id=?) AS m0,(session_id=? AND turn_id=?) AS m1,(session_id=? AND turn_id=?) AS m2,(session_id=? AND turn_id=?) AS m3,(session_id=? AND turn_id=?) AS m4,(session_id=? AND turn_id=?) AS m5,(session_id=? AND turn_id=?) AS m6,(session_id=? AND turn_id=?) AS m7 FROM oc_session_turns WHERE tenant_id=? AND workspace_id=? AND (1) AND ((session_id=? AND turn_id=?) OR (session_id=? AND turn_id=?) OR (session_id=? AND turn_id=?) OR (session_id=? AND turn_id=?) OR (session_id=? AND turn_id=?) OR (session_id=? AND turn_id=?) OR (session_id=? AND turn_id=?) OR (session_id=? AND turn_id=?))";
const TURN_BODIES: &str = "SELECT content,(id=?) AS m0,(id=?) AS m1,(id=?) AS m2,(id=?) AS m3,(id=?) AS m4,(id=?) AS m5,(id=?) AS m6,(id=?) AS m7 FROM oc_events WHERE tenant_id=? AND workspace_id=? AND (state='active' AND kind='session_turn') AND ((id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?))";

#[derive(Default)]
struct TurnRow {
    turn: Option<native_turso::Row>,
    event: Option<native_turso::Row>,
    evidence: Vec<native_turso::Row>,
    body: Option<native_turso::Row>,
    date: Option<native_turso::Row>,
}

pub(in crate::storage::sqlite) struct TurnReadBatch {
    keys: Vec<(Uuid, Uuid)>,
    rows: Vec<TurnRow>,
    evidence_loaded: bool,
    bodies_loaded: bool,
    archives: ArchiveReadBatch,
    archive_indices: Vec<Vec<usize>>,
}

impl TursoTx {
    pub(in crate::storage::sqlite) async fn context_turn_metadata(
        &mut self,
        keys: &[(Uuid, Uuid)],
    ) -> StorageResult<TurnReadBatch> {
        if !keys.is_empty() {
            self.require_sessions().await?;
        }
        let mut batch = TurnReadBatch {
            keys: keys.to_vec(),
            rows: keys.iter().map(|_| TurnRow::default()).collect(),
            evidence_loaded: false,
            bodies_loaded: false,
            archives: Self::archive_metadata(&[]),
            archive_indices: keys.iter().map(|_| Vec::new()).collect(),
        };
        let requests = keys
            .iter()
            .map(|(session, turn)| {
                Ok(vec![association_key(*session)?, association_key(*turn)?])
            })
            .collect::<StorageResult<Vec<_>>>()?;
        for (index, row) in
            read_model::associate_query(&mut self.tx, self.scope, TURN_ROWS, 2, &requests)
                .await?
        {
            batch.rows[index].turn.get_or_insert(row);
        }
        let requests = batch
            .rows
            .iter()
            .map(|row| {
                Ok(vec![match &row.turn {
                    Some(row) => row.try_get("source_event_id").map_err(turso_err)?,
                    None => association_key(None::<Uuid>)?,
                }])
            })
            .collect::<StorageResult<Vec<_>>>()?;
        for (index, row) in
            read_model::associate_query(&mut self.tx, self.scope, TURN_EVENTS, 1, &requests)
                .await?
        {
            batch.rows[index].event.get_or_insert(row);
        }
        Ok(batch)
    }

    async fn context_turn_evidence_rows(&mut self, batch: &mut TurnReadBatch) -> StorageResult<()> {
        if batch.evidence_loaded {
            return Ok(());
        }
        let mut cursors: Vec<Option<AssociationKey>> = batch.keys.iter().map(|_| None).collect();
        let mut pending: Vec<bool> = batch.rows.iter().map(|row| row.event.is_some()).collect();
        while pending.iter().any(|pending| *pending) {
            let requests = batch
                .keys
                .iter()
                .enumerate()
                .map(|(index, (session, turn))| {
                    let cursor = cursors[index]
                        .clone()
                        .unwrap_or(association_key(None::<i64>)?);
                    Ok(vec![
                        association_key(pending[index].then_some(*session))?,
                        association_key(pending[index].then_some(*turn))?,
                        cursor.clone(),
                        cursor,
                    ])
                })
                .collect::<StorageResult<Vec<_>>>()?;
            let rows = read_model::associate_query(
                &mut self.tx,
                self.scope,
                TURN_EVIDENCE,
                4,
                &requests,
            )
            .await?;
            // A full global page may contain no row for one request. Only an
            // empty eight-key page proves all its remaining requests exhausted.
            let mut groups = HashSet::new();
            for (index, row) in rows {
                groups.insert(index / read_model::ASSOCIATION_KEYS);
                if batch.rows[index].evidence.len() < 65 {
                    cursors[index] = Some(row.try_get("ordinal").map_err(turso_err)?);
                    batch.rows[index].evidence.push(row);
                }
                if batch.rows[index].evidence.len() == 65 {
                    pending[index] = false;
                }
            }
            for (index, pending) in pending.iter_mut().enumerate() {
                if !groups.contains(&(index / read_model::ASSOCIATION_KEYS)) {
                    *pending = false;
                }
            }
        }
        // Best-effort hints populate a shared lazy archive readset. Strict
        // String/JSON decoding is repeated in original per-turn order below.
        let mut references = Vec::new();
        for (index, row) in batch.rows.iter().enumerate() {
            if row.evidence.len() > crate::session::MAX_USED_EVIDENCE {
                continue;
            }
            for evidence in &row.evidence {
                if let Ok(text) = evidence.try_get::<String>("evidence")
                    && let Ok(archive) = decode::<ArchivedEvidence>(&text)
                {
                    batch.archive_indices[index].push(references.len());
                    references.push(archive.reference);
                }
            }
        }
        batch.archives = Self::archive_metadata(&references);
        batch.evidence_loaded = true;
        Ok(())
    }

    async fn context_turn_bodies(&mut self, batch: &mut TurnReadBatch) -> StorageResult<()> {
        if batch.bodies_loaded {
            return Ok(());
        }
        let requests = batch
            .keys
            .iter()
            .map(|(session, turn)| {
                Ok(vec![association_key(*session)?, association_key(*turn)?])
            })
            .collect::<StorageResult<Vec<_>>>()?;
        for (index, row) in
            read_model::associate_query(&mut self.tx, self.scope, TURN_DATES, 2, &requests)
                .await?
        {
            batch.rows[index].date.get_or_insert(row);
        }
        let requests = batch
            .rows
            .iter()
            .map(|row| {
                Ok(vec![match &row.turn {
                    Some(row) => row.try_get("source_event_id").map_err(turso_err)?,
                    None => association_key(None::<Uuid>)?,
                }])
            })
            .collect::<StorageResult<Vec<_>>>()?;
        for (index, row) in
            read_model::associate_query(&mut self.tx, self.scope, TURN_BODIES, 1, &requests)
                .await?
        {
            batch.rows[index].body.get_or_insert(row);
        }
        batch.bodies_loaded = true;
        Ok(())
    }

    pub(in crate::storage::sqlite) async fn context_turn_from_metadata(
        &mut self,
        batch: &mut TurnReadBatch,
        index: usize,
    ) -> StorageResult<Option<crate::context_retrieval::ContextSessionTurn>> {
        if batch.rows[index].event.is_none() {
            return Ok(None);
        }
        let source: Uuid = batch.rows[index]
            .turn
            .as_ref()
            .ok_or(StorageError::NotFound)?
            .try_get("source_event_id")
            .map_err(turso_err)?;
        self.context_turn_evidence_rows(batch).await?;
        let texts: Vec<String> = batch.rows[index]
            .evidence
            .iter()
            .map(|row| row.try_get("evidence").map_err(turso_err))
            .collect::<StorageResult<_>>()?;
        if texts.len() > crate::session::MAX_USED_EVIDENCE {
            return Ok(None);
        }
        let evidence: Vec<ArchivedEvidence> = texts
            .iter()
            .map(|text| decode(text))
            .collect::<StorageResult<_>>()?;
        if evidence.len() != batch.archive_indices[index].len() {
            return Err(StorageError::Unavailable(
                "inconsistent session metadata".into(),
            ));
        }
        for (archive, position) in evidence.iter().zip(&batch.archive_indices[index]) {
            let current = match self
                .archive_from_batch(
                    &mut batch.archives,
                    *position,
                    &archive.reference,
                    archive.evidence_ref_id,
                )
                .await
            {
                Ok(value) => value,
                Err(StorageError::NotFound) => return Ok(None),
                Err(error) => return Err(error),
            };
            if current != *archive {
                return Ok(None);
            }
        }
        match self.require_archived_evidence_set_visible(&evidence).await {
            Ok(()) => {}
            Err(StorageError::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        }
        self.context_turn_bodies(batch).await?;
        let Some(row) = &batch.rows[index].body else {
            return Ok(None);
        };
        let body: String = row.try_get("content").map_err(turso_err)?;
        let created_at: i64 = batch.rows[index]
            .date
            .as_ref()
            .ok_or(StorageError::NotFound)?
            .try_get("created_at")
            .map_err(turso_err)?;
        #[derive(serde::Deserialize)]
        struct Body {
            question: String,
            answer: String,
        }
        let body: Body = decode(&body)?;
        let (session_id, turn_id) = batch.keys[index];
        Ok(Some(crate::context_retrieval::ContextSessionTurn {
            session_id,
            turn_id,
            source_event_id: source,
            question: body.question,
            answer: body.answer,
            evidence,
            created_at,
            score: 0.0,
        }))
    }
}
