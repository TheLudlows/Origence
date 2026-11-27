//! Private archives use AnySeek for committed turns and SQLite for live authority.
use crate::{
    context_retrieval::{ContextQuery, ContextSessionTurn},
    error::{AppError, Result},
    service::Service,
    session::{ArchiveDomain, ArchiveLocator, ArchivedTurnBody, FeedbackInput, SessionTurnInput},
    storage::{
        DomainTx, Permission, StorageError, StorageResult,
        anyseek::private_archive::{ArchiveQuery, NativeArchiveTurn},
        hash,
        sqlite::{ArchiveEntry, SqliteTx, now_ms},
    },
    types::AuthContext,
};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;

pub(crate) struct ArchiveCandidate {
    archive: ArchiveContent,
    score: f64,
    routes: Vec<String>,
}

enum ArchiveContent {
    Native(NativeArchiveTurn),
    Legacy(ArchiveEntry, ArchivedTurnBody),
}

#[derive(PartialEq, Eq, Hash, PartialOrd, Ord)]
enum ArchiveHit {
    Native(Uuid),
    Legacy(String),
}

impl ArchiveCandidate {
    pub(crate) async fn deliver(self, tx: &mut SqliteTx) -> StorageResult<ContextSessionTurn> {
        let mut turn = match self.archive {
            ArchiveContent::Native(record) => {
                require_native_record(tx, &record).await?;
                ContextSessionTurn {
                    session_id: record.session,
                    turn_id: record.turn,
                    source_event_id: record.source,
                    question: record.body.question,
                    answer: record.body.answer,
                    evidence: record.evidence,
                    evidence_use: record.body.evidence_use,
                    created_at: record.created_at,
                    score: 0.0,
                    retrieval_routes: Vec::new(),
                }
            }
            ArchiveContent::Legacy(entry, body) => tx.archive_delivery(&entry, body).await?,
        };
        turn.score = self.score;
        turn.retrieval_routes = self.routes;
        Ok(turn)
    }
}

fn session_id(id: Uuid) -> Result<()> {
    if id.is_nil() {
        return Err(AppError::Invalid("session_id must be non-nil".into()));
    }
    Ok(())
}

impl Service {
    /// Resolve the authenticated logical key's server-assigned private archive.
    pub async fn private_session(&self, a: &AuthContext) -> Result<Value> {
        let mut tx = self.write(a, Permission::Read).await?;
        let session_id = tx.private_session_id().await?;
        let key_id = tx.private_key_id().await?;
        tx.commit().await?;
        Ok(json!({"session_id":session_id,"key_id":key_id,"workspace_id":a.workspace_id}))
    }
    /// Explicit admin authorization rotates credentials without merging archives.
    pub async fn rotate_key(&self, a: &AuthContext, id: Uuid) -> Result<Value> {
        let mut tx = self.write(a, Permission::Delete).await?;
        let key = tx.rotate_scoped_key(id).await?;
        tx.commit().await?;
        Ok(json!(key))
    }

    pub async fn archive_turn(
        &self,
        a: &AuthContext,
        key: &str,
        session: Uuid,
        input: SessionTurnInput,
    ) -> Result<Value> {
        session_id(session)?;
        input.validate().map_err(|e| AppError::Invalid(e.into()))?;
        let request = json!({"session_id":session,"input":input});
        let digest = hash(request.to_string());
        let turn = input.turn_id;
        let mut tx = self.read(a, Permission::Write).await?;
        let domain = tx.require_native_archive_live(session, turn).await?;
        let initialized = tx.native_archive_initialized(session).await?;
        let historical = match tx.archive_entry(session, turn).await {
            Ok(_) => true,
            Err(StorageError::NotFound) => false,
            Err(error) => return Err(error.into()),
        };
        tx.commit().await?;
        if historical {
            return self
                .archive_legacy_turn(a, key, session, &input, &request, &digest)
                .await;
        }

        let backend = self.engine.vector().private_archive();
        let existing = if self.require_native_collection(domain, initialized).await? {
            backend.native_archive_read(domain, session, turn).await?
        } else {
            backend.native_archive_collection(domain, true).await?;
            None
        };
        let (mut tx, cached) = self
            .command(a, Permission::Write, "session.turn.archive", key, &request)
            .await?;
        verify_native_lookup(&mut tx, session, turn, existing.as_ref()).await?;
        let (record, needs_write) = match existing {
            Some(record) if record.request_hash == digest => (record, false),
            Some(_) => return Err(AppError::Conflict("turn is immutable".into())),
            None => (
                NativeArchiveTurn {
                    session,
                    turn,
                    source: Uuid::new_v4(),
                    created_by: a.id,
                    created_at: now_ms(),
                    evidence: tx
                        .prepare_native_archive_evidence(&input.used_evidence)
                        .await?,
                    sequence: tx.allocate_native_archive_sequence(session).await?,
                    request_hash: digest,
                    body: ArchivedTurnBody::from_input(&input),
                },
                true,
            ),
        };
        // Record only the ordinary API key binding. Native CAS decides whether
        // the turn exists and which original identifiers it has.
        if cached.is_none() {
            tx.finish_command(json!({"session_id":session,"turn_id":turn}))
                .await?;
        }
        tx.commit().await?;
        let record = if needs_write {
            let (terms, vector) = self.archive_projection(&record.body).await?;
            backend
                .native_archive_write(domain, &record, terms, vector)
                .await?
        } else {
            record
        };
        let mut tx = self.read(a, Permission::Write).await?;
        require_native_record(&mut tx, &record).await?;
        tx.commit().await?;
        Ok(
            json!({"session_id":session,"turn_id":turn,"source_event_id":record.source,"state":"archived"}),
        )
    }

    async fn archive_legacy_turn(
        &self,
        a: &AuthContext,
        key: &str,
        session: Uuid,
        input: &SessionTurnInput,
        request: &Value,
        digest: &str,
    ) -> Result<Value> {
        let body = ArchivedTurnBody::from_input(input);
        let (mut tx, cached) = self
            .command(a, Permission::Write, "session.turn.archive", key, request)
            .await?;
        let entry = tx
            .retry_legacy_archive(session, input.turn_id, digest, &body)
            .await?;
        if cached.is_none() {
            tx.finish_command(json!({"session_id":session,"turn_id":input.turn_id,"source_event_id":entry.locator.source})).await?;
        }
        tx.commit().await?;

        let backend = self.engine.vector().private_archive();
        let seq = if let Some((_, seq)) = backend
            .archive_read(&entry.locator, entry.state == "pending")
            .await?
        {
            seq
        } else {
            self.write_archive_locator(&entry.locator, &body).await?
        };
        let mut tx = self.write(a, Permission::Write).await?;
        tx.confirm_archive(&entry, seq).await?;
        tx.commit().await?;
        Ok(entry.receipt())
    }

    pub(crate) async fn write_archive_locator(
        &self,
        locator: &ArchiveLocator,
        body: &ArchivedTurnBody,
    ) -> Result<u64> {
        let (terms, embedding) = self.archive_projection(body).await?;
        Ok(self
            .engine
            .vector()
            .private_archive()
            .archive_write(locator, body, terms, embedding)
            .await?)
    }

    async fn archive_projection(
        &self,
        body: &ArchivedTurnBody,
    ) -> Result<(String, Option<Vec<f32>>)> {
        let text = format!("{}\n{}", body.question, body.answer);
        let terms = tokio::task::spawn_blocking(move || archive_terms(&text))
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        let embedding = if self.models.profile.is_some() {
            // Bound provider input independently of the authoritative record.
            // Include both fields even at the archive's original byte limit.
            let excerpt = format!("{}\n{}", prefix(&body.question), prefix(&body.answer));
            Some(self.models.embed(&excerpt).await?)
        } else {
            None
        };
        Ok((terms, embedding))
    }

    async fn require_native_collection(
        &self,
        domain: ArchiveDomain,
        initialized: bool,
    ) -> Result<bool> {
        if !initialized {
            return Ok(false);
        }
        if !self
            .engine
            .vector()
            .private_archive()
            .native_archive_collection(domain, false)
            .await?
        {
            return Err(AppError::Unavailable(
                "private archive collection missing".into(),
            ));
        }
        Ok(true)
    }

    pub(crate) async fn load_native_archive(
        &self,
        a: &AuthContext,
        session: Uuid,
        turn: Uuid,
    ) -> Result<Option<NativeArchiveTurn>> {
        session_id(session)?;
        let mut tx = self.read(a, Permission::Write).await?;
        let domain = tx.require_native_archive_live(session, turn).await?;
        let initialized = tx.native_archive_initialized(session).await?;
        tx.commit().await?;
        if !self.require_native_collection(domain, initialized).await? {
            return Ok(None);
        }
        let record = self
            .engine
            .vector()
            .private_archive()
            .native_archive_read(domain, session, turn)
            .await?;
        let mut tx = self.read(a, Permission::Write).await?;
        verify_native_lookup(&mut tx, session, turn, record.as_ref()).await?;
        tx.commit().await?;
        Ok(record)
    }

    async fn visible_archive_entry(
        &self,
        a: &AuthContext,
        session: Uuid,
        turn: Uuid,
    ) -> Result<ArchiveEntry> {
        session_id(session)?;
        let mut tx = self.read(a, Permission::Write).await?;
        let mut entry = tx.archive_entry(session, turn).await?;
        if entry.state == "archived" {
            tx.require_turn_visible(session, turn).await?;
        }
        tx.commit().await?;
        if entry.state == "pending" {
            let (_, seq) = self
                .engine
                .vector()
                .private_archive()
                .archive_read(&entry.locator, true)
                .await?
                .ok_or(AppError::NotFound)?;
            let mut tx = self.write(a, Permission::Write).await?;
            tx.confirm_archive(&entry, seq).await?;
            entry = tx.archive_entry(session, turn).await?;
            tx.commit().await?;
        }
        Ok(entry)
    }

    pub(crate) async fn archived_body(&self, entry: &ArchiveEntry) -> Result<ArchivedTurnBody> {
        self.engine
            .vector()
            .private_archive()
            .archive_read(&entry.locator, false)
            .await
            .map_err(|error| match error {
                StorageError::Unavailable(_) => AppError::Unavailable(
                    "private archive record missing or failed integrity verification".into(),
                ),
                other => other.into(),
            })?
            .map(|(body, _)| body)
            .ok_or_else(|| AppError::Unavailable("private archive record unavailable".into()))
    }

    pub async fn session_turn(&self, a: &AuthContext, session: Uuid, turn: Uuid) -> Result<Value> {
        if let Some(record) = self.load_native_archive(a, session, turn).await? {
            let mut tx = self.read(a, Permission::Write).await?;
            let value = native_archive_view(&mut tx, record).await?;
            tx.commit().await?;
            return Ok(value);
        }
        let entry = self.visible_archive_entry(a, session, turn).await?;
        let body = self.archived_body(&entry).await?;
        let mut tx = self.read(a, Permission::Write).await?;
        let value = tx.archive_view(&entry, body).await?;
        tx.commit().await?;
        Ok(value)
    }

    pub(crate) async fn load_native_archive_source(
        &self,
        a: &AuthContext,
        source: Uuid,
    ) -> Result<Option<NativeArchiveTurn>> {
        let mut tx = self.read(a, Permission::Write).await?;
        let Some(session) = tx.bound_private_session().await? else {
            return Ok(None);
        };
        let domain = match tx.archive_domain(session).await {
            Ok(domain) => domain,
            Err(StorageError::NotFound) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let initialized = tx.native_archive_initialized(session).await?;
        tx.commit().await?;
        if !self.require_native_collection(domain, initialized).await? {
            return Ok(None);
        }
        let Some(record) = self
            .engine
            .vector()
            .private_archive()
            .native_archive_source(domain, session, source)
            .await?
        else {
            return Ok(None);
        };
        let mut tx = self.read(a, Permission::Write).await?;
        require_native_record(&mut tx, &record).await?;
        tx.commit().await?;
        Ok(Some(record))
    }

    pub(crate) async fn archive_source(
        &self,
        a: &AuthContext,
        source: Uuid,
        session: Uuid,
        turn: Uuid,
    ) -> Result<Value> {
        let entry = self.visible_archive_entry(a, session, turn).await?;
        let body = self.archived_body(&entry).await?;
        let content = json!(body).to_string();
        let mut tx = self.read(a, Permission::Write).await?;
        tx.archive_delivery(&entry, body).await?;
        let mut value = tx.event_view(source).await?;
        value["content"] = json!(content);
        tx.commit().await?;
        Ok(value)
    }

    /// Page by immutable registration sequence. Withdrawn turns leave gaps.
    pub async fn session_turns(
        &self,
        a: &AuthContext,
        session: Uuid,
        after: i64,
        limit: usize,
    ) -> Result<Value> {
        if !(1..=32).contains(&limit) || after < 0 {
            return Err(AppError::Invalid("invalid private archive page".into()));
        }
        let mut tx = self.read(a, Permission::Write).await?;
        let (domain, legacy_next, scanned, entries) =
            tx.archive_page(session, after, limit).await?;
        let initialized = tx.native_archive_initialized(session).await?;
        tx.commit().await?;
        let (native_next, native) =
            if scanned < limit && self.require_native_collection(domain, initialized).await? {
                self.engine
                    .vector()
                    .private_archive()
                    .native_archive_page(domain, session, after, limit - scanned)
                    .await?
            } else {
                (None, Vec::new())
            };
        let mut bodies = Vec::new();
        for entry in entries {
            let body = self.archived_body(&entry).await?;
            bodies.push((entry, body));
        }
        let mut tx = self.read(a, Permission::Write).await?;
        tx.archive_domain(session).await?;
        let mut turns = Vec::new();
        for (entry, body) in bodies {
            match tx.archive_view(&entry, body).await {
                Ok(view) => turns.push(view),
                Err(StorageError::NotFound) => {}
                Err(error) => return Err(error.into()),
            }
        }
        for record in native {
            match native_archive_view(&mut tx, record).await {
                Ok(view) => turns.push(view),
                Err(StorageError::NotFound) => {}
                Err(error) => return Err(error.into()),
            }
        }
        tx.commit().await?;
        Ok(json!({"session_id":session,"turns":turns,"next_after":native_next.or(legacy_next)}))
    }

    pub async fn withdraw_session_archive(
        &self,
        a: &AuthContext,
        session: Uuid,
        turn: Option<Uuid>,
    ) -> Result<Value> {
        let mut tx = self.write(a, Permission::Write).await?;
        match tx.withdraw_archives(session, turn).await {
            Ok(()) => {
                if turn.is_none() {
                    tx.withdraw_session_native(session).await?;
                }
            }
            Err(StorageError::NotFound) if turn.is_some() => {
                let turn = turn.expect("checked");
                if !tx.native_archive_request_exists(session, turn).await? {
                    return Err(AppError::NotFound);
                }
                tx.withdraw_native_archive(session, turn).await?;
            }
            Err(error) => return Err(error.into()),
        }
        tx.audit(
            "session.archive_withdrawn",
            turn.unwrap_or(session),
            json!({"session_id":session,"turn_id":turn}),
        )
        .await?;
        tx.commit().await?;
        Ok(json!({"session_id":session,"turn_id":turn,"blocked":true}))
    }

    pub(crate) async fn context_session_candidates(
        &self,
        auth: &AuthContext,
        input: &crate::evidence::EvidenceInput,
        prepared: &ContextQuery,
        limit: usize,
    ) -> Result<(usize, Vec<ArchiveCandidate>)> {
        let mut tx = self.read(auth, Permission::Write).await?;
        let bound = tx.bound_private_session().await?;
        if input.session_id.is_some() && input.session_id != bound {
            return Err(AppError::NotFound);
        }
        let Some(session) = bound.filter(|id| Some(*id) != input.exclude_session_id) else {
            return Ok((0, Vec::new()));
        };
        // AnySeek's default 10,000-record collection stores two members per turn.
        let (domain, _, scanned, entries) = tx.archive_page(session, 0, 5001).await?;
        let initialized = tx.native_archive_initialized(session).await?;
        tx.commit().await?;
        if entries.is_empty() && !initialized {
            return Ok((0, Vec::new()));
        }
        if scanned > 5000 {
            return Err(AppError::Unavailable(
                "private archive collection exceeds query budget".into(),
            ));
        }
        let groups: Vec<_> = entries.iter().map(|entry| entry.locator.group()).collect();
        let mut eligible: HashMap<_, _> = groups.iter().cloned().zip(entries).collect();
        let backend = self.engine.vector().private_archive();
        if !groups.is_empty() && !backend.private_collection(domain, false).await? {
            return Err(AppError::Unavailable(
                "private archive collection unavailable".into(),
            ));
        }
        let native = self.require_native_collection(domain, initialized).await?;
        let native_count = if native {
            backend
                .native_archive_search(domain, ArchiveQuery::Keyword(""), 1)
                .await?
                .0
        } else {
            0
        };
        let terms = archive_terms(&input.query);
        let mut queries = Vec::new();
        if prepared.effective_mode != "vector" {
            queries.push(("keyword", ArchiveQuery::Keyword(&terms)));
        }
        if let Some(vector) = &prepared.embedding {
            queries.push(("vector", ArchiveQuery::Vector(vector)));
        }
        // A missing legacy projection is an error even for a nonmatching turn.
        for batch in groups.chunks(64) {
            let (count, _) = backend
                .archive_search(domain, ArchiveQuery::Keyword(""), batch, 1)
                .await?;
            if count != batch.len() {
                return Err(AppError::Unavailable(
                    "private archive search projection unavailable".into(),
                ));
            }
        }
        let mut ranked = HashMap::<ArchiveHit, (f64, Vec<String>)>::new();
        for (route, query) in &queries {
            let mut hits = Vec::new();
            if native {
                let (_, found) = backend.native_archive_search(domain, *query, limit).await?;
                hits.extend(
                    found
                        .into_iter()
                        .map(|(turn, score)| (ArchiveHit::Native(turn), score)),
                );
            }
            for batch in groups.chunks(64) {
                let (_, found) = backend.archive_search(domain, *query, batch, limit).await?;
                hits.extend(
                    found
                        .into_iter()
                        .map(|(group, score)| (ArchiveHit::Legacy(group), score)),
                );
            }
            hits.sort_by(|left, right| {
                right
                    .1
                    .total_cmp(&left.1)
                    .then_with(|| left.0.cmp(&right.0))
            });
            for (index, (group, score)) in hits.into_iter().enumerate() {
                let item = ranked.entry(group).or_default();
                if !item.1.iter().any(|previous| previous == route) {
                    item.0 += if queries.len() == 2 {
                        1.0 / (61 + index) as f64
                    } else {
                        score
                    };
                    item.1.push((*route).into());
                }
            }
        }
        let mut ranked: Vec<_> = ranked.into_iter().collect();
        let sequence = |hit: &ArchiveHit| match hit {
            ArchiveHit::Legacy(group) => eligible.get(group).map(|entry| entry.sequence),
            ArchiveHit::Native(_) => None,
        };
        ranked.sort_by(|a, b| {
            b.1.0
                .total_cmp(&a.1.0)
                .then_with(|| sequence(&b.0).cmp(&sequence(&a.0)))
                .then_with(|| a.0.cmp(&b.0))
        });
        let mut found = Vec::new();
        for (hit, (score, routes)) in ranked.into_iter().take(limit) {
            let archive = match hit {
                ArchiveHit::Native(turn) => {
                    let Some(record) = backend
                        .native_archive_read_candidate(domain, session, turn)
                        .await?
                    else {
                        continue;
                    };
                    ArchiveContent::Native(record)
                }
                ArchiveHit::Legacy(group) => {
                    let entry = eligible.remove(&group).ok_or_else(|| {
                        AppError::Unavailable("private archive directory changed".into())
                    })?;
                    let body = self.archived_body(&entry).await?;
                    ArchiveContent::Legacy(entry, body)
                }
            };
            found.push(ArchiveCandidate {
                archive,
                score,
                routes,
            });
        }
        Ok((groups.len() + native_count, found))
    }

    pub async fn record_feedback(
        &self,
        a: &AuthContext,
        key: &str,
        session: Uuid,
        turn: Uuid,
        input: FeedbackInput,
    ) -> Result<Value> {
        session_id(session)?;
        if turn.is_nil() {
            return Err(AppError::Invalid("turn_id must be non-nil".into()));
        }
        input.validate().map_err(|e| AppError::Invalid(e.into()))?;
        let archive = self.load_native_archive(a, session, turn).await?;
        let request = json!({"session_id":session,"turn_id":turn,"input":input});
        let (mut tx, cached) = self
            .command(
                a,
                Permission::Write,
                "session.feedback.record",
                key,
                &request,
            )
            .await?;
        if let Some(receipt) = cached {
            tx.require_turn_visible(session, turn).await?;
            tx.commit().await?;
            return Ok(receipt);
        }
        if let Some(record) = &archive {
            tx.attach_native_archive_proof(record).await?;
        }
        let receipt = tx
            .record_feedback(session, turn, &input, &hash(request.to_string()))
            .await?;
        Self::finish(tx, receipt).await
    }

    pub async fn session_feedback(
        &self,
        a: &AuthContext,
        session: Uuid,
        turn: Uuid,
    ) -> Result<Value> {
        session_id(session)?;
        let record = self.load_native_archive(a, session, turn).await?;
        let mut tx = self.read(a, Permission::Write).await?;
        let value = if record.is_some() {
            tx.require_native_archive_live(session, turn).await?;
            json!({"session_id":session,"turn_id":turn,"feedback":tx.feedback_rows(session, turn).await?})
        } else {
            tx.session_feedback(session, turn).await?
        };
        tx.commit().await?;
        Ok(value)
    }
}

async fn native_archive_view(tx: &mut SqliteTx, record: NativeArchiveTurn) -> StorageResult<Value> {
    require_native_record(tx, &record).await?;
    let feedback = tx.feedback_rows(record.session, record.turn).await?;
    let mut value = json!({
        "session_id":record.session,"turn_id":record.turn,"source_event_id":record.source,
        "question":record.body.question,"answer":record.body.answer,"used_evidence":record.evidence,
        "feedback":feedback,"created_by":record.created_by,"created_at":record.created_at,"sequence":record.sequence,
    });
    if !record.body.evidence_use.is_reported() {
        value["evidence_use"] = json!(record.body.evidence_use);
    }
    Ok(value)
}

async fn require_native_record(tx: &mut SqliteTx, record: &NativeArchiveTurn) -> StorageResult<()> {
    verify_native_lookup(tx, record.session, record.turn, Some(record)).await
}

async fn verify_native_lookup(
    tx: &mut SqliteTx,
    session: Uuid,
    turn: Uuid,
    record: Option<&NativeArchiveTurn>,
) -> StorageResult<()> {
    tx.require_native_archive_live(session, turn).await?;
    if let Some((source, digest)) = tx.native_archive_proof_digest(session, turn).await? {
        match record {
            Some(record) if record.source == source && record.request_hash == digest => {}
            Some(_) => {
                return Err(StorageError::Unavailable(
                    "private archive proof mismatch".into(),
                ));
            }
            None => {
                return Err(StorageError::Unavailable(
                    "private archive record referenced by a proof is missing".into(),
                ));
            }
        }
    }
    Ok(())
}

fn prefix(text: &str) -> &str {
    &text[..text.floor_char_boundary(text.len().min(4096))]
}

fn archive_terms(text: &str) -> String {
    let tokens = crate::parsing::lexical(text);
    tokens
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| {
            if token.len() > 128 {
                format!("z{}", hash(token))
            } else {
                token.to_owned()
            }
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(" ")
}
