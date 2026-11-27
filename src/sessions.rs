//! Private archives use AnySeek for committed turns and SQLite for live authority.
use crate::{
    context_retrieval::{ContextQuery, ContextSessionTurn},
    error::{AppError, Result},
    service::Service,
    session::{ArchiveDomain, ArchivedTurnBody, FeedbackInput, SessionTurnInput},
    storage::{
        DomainTx, Permission, StorageError, StorageResult,
        anyseek::private_archive::{ArchiveQuery, NativeArchiveTurn},
        hash,
        sqlite::{SqliteTx, now_ms},
    },
    types::AuthContext,
};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;

pub(crate) struct ArchiveCandidate {
    archive: NativeArchiveTurn,
    score: f64,
    routes: Vec<String>,
}

impl ArchiveCandidate {
    pub(crate) async fn deliver(self, tx: &mut SqliteTx) -> StorageResult<ContextSessionTurn> {
        let record = self.archive;
        require_native_record(tx, &record).await?;
        let turn = ContextSessionTurn {
            session_id: record.session,
            turn_id: record.turn,
            source_event_id: record.source,
            question: record.body.question,
            answer: record.body.answer,
            evidence: record.evidence,
            evidence_use: record.body.evidence_use,
            created_at: record.created_at,
            score: self.score,
            retrieval_routes: self.routes,
        };
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
        tx.commit().await?;

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

    pub async fn session_turn(&self, a: &AuthContext, session: Uuid, turn: Uuid) -> Result<Value> {
        let record = self
            .load_native_archive(a, session, turn)
            .await?
            .ok_or(AppError::NotFound)?;
        let mut tx = self.read(a, Permission::Write).await?;
        let value = native_archive_view(&mut tx, record).await?;
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
        let domain = tx.archive_domain(session).await?;
        let initialized = tx.native_archive_initialized(session).await?;
        tx.commit().await?;
        let (next, native) = if self.require_native_collection(domain, initialized).await? {
            self.engine
                .vector()
                .private_archive()
                .native_archive_page(domain, session, after, limit)
                .await?
        } else {
            (None, Vec::new())
        };
        let mut tx = self.read(a, Permission::Write).await?;
        tx.archive_domain(session).await?;
        let mut turns = Vec::new();
        for record in native {
            match native_archive_view(&mut tx, record).await {
                Ok(view) => turns.push(view),
                Err(StorageError::NotFound) => {}
                Err(error) => return Err(error.into()),
            }
        }
        tx.commit().await?;
        Ok(json!({"session_id":session,"turns":turns,"next_after":next}))
    }

    pub async fn withdraw_session_archive(
        &self,
        a: &AuthContext,
        session: Uuid,
        turn: Option<Uuid>,
    ) -> Result<Value> {
        let mut tx = self.write(a, Permission::Write).await?;
        if let Some(turn) = turn {
            if !tx.native_archive_request_exists(session, turn).await? {
                return Err(AppError::NotFound);
            }
            tx.withdraw_native_archive(session, turn).await?;
        } else {
            tx.withdraw_session_native(session).await?;
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
        let domain = tx.archive_domain(session).await?;
        let initialized = tx.native_archive_initialized(session).await?;
        tx.commit().await?;
        if !self.require_native_collection(domain, initialized).await? {
            return Ok((0, Vec::new()));
        }
        let backend = self.engine.vector().private_archive();
        let (count, _) = backend
            .native_archive_search(domain, ArchiveQuery::Keyword(""), 1)
            .await?;
        let terms = archive_terms(&input.query);
        let mut queries = Vec::new();
        if prepared.effective_mode != "vector" {
            queries.push(("keyword", ArchiveQuery::Keyword(&terms)));
        }
        if let Some(vector) = &prepared.embedding {
            queries.push(("vector", ArchiveQuery::Vector(vector)));
        }
        let mut ranked = HashMap::<Uuid, (f64, Vec<String>)>::new();
        for (route, query) in &queries {
            let (_, mut hits) = backend.native_archive_search(domain, *query, limit).await?;
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
        ranked.sort_by(|a, b| b.1.0.total_cmp(&a.1.0).then_with(|| a.0.cmp(&b.0)));
        let mut found = Vec::new();
        for (turn, (score, routes)) in ranked.into_iter().take(limit) {
            if let Some(archive) = backend
                .native_archive_read_candidate(domain, session, turn)
                .await?
            {
                found.push(ArchiveCandidate {
                    archive,
                    score,
                    routes,
                });
            }
        }
        Ok((count, found))
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
        let archive = self
            .load_native_archive(a, session, turn)
            .await?
            .ok_or(AppError::NotFound)?;
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
        tx.attach_native_archive_proof(&archive).await?;
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
        let record = self
            .load_native_archive(a, session, turn)
            .await?
            .ok_or(AppError::NotFound)?;
        let mut tx = self.read(a, Permission::Write).await?;
        require_native_record(&mut tx, &record).await?;
        let value = tx.session_feedback_view(session, turn).await?;
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
