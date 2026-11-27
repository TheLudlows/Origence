//! Private archives reconcile AnySeek durability with SQLite delivery authority.
use crate::{
    context_retrieval::ContextQuery,
    error::{AppError, Result},
    service::Service,
    session::{ArchiveLocator, ArchivedTurnBody, FeedbackInput, SessionTurnInput},
    storage::{
        DomainTx, Permission, StorageError,
        anyseek::{operation_epoch, private_archive::ArchiveQuery},
        hash,
        sqlite::ArchiveEntry,
    },
    types::AuthContext,
};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;

pub(crate) struct ArchiveCandidate {
    pub entry: ArchiveEntry,
    pub body: ArchivedTurnBody,
    pub score: f64,
    pub routes: Vec<String>,
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
        let body = ArchivedTurnBody::from_input(&input);
        let (mut tx, cached) = self
            .command(a, Permission::Write, "session.turn.archive", key, &request)
            .await?;
        let entry = tx
            .stage_archive(
                session,
                &input,
                &hash(request.to_string()),
                &body,
                operation_epoch()?,
            )
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
        Ok(self
            .engine
            .vector()
            .private_archive()
            .archive_write(locator, body, terms, embedding)
            .await?)
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
        let entry = self.visible_archive_entry(a, session, turn).await?;
        let body = self.archived_body(&entry).await?;
        let mut tx = self.read(a, Permission::Write).await?;
        let value = tx.archive_view(&entry, body).await?;
        tx.commit().await?;
        Ok(value)
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
        let (_, next, _, entries) = tx.archive_page(session, after, limit).await?;
        tx.commit().await?;
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
        tx.withdraw_archives(session, turn).await?;
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
        tx.commit().await?;
        if entries.is_empty() {
            return Ok((0, Vec::new()));
        }
        if scanned > 5000 {
            return Err(AppError::Unavailable(
                "private archive collection exceeds query budget".into(),
            ));
        }
        let groups: Vec<_> = entries.iter().map(|entry| entry.locator.group()).collect();
        let eligible: HashMap<_, _> = groups.iter().cloned().zip(entries).collect();
        let backend = self.engine.vector().private_archive();
        if !backend.private_collection(domain, false).await? {
            return Err(AppError::Unavailable(
                "private archive collection unavailable".into(),
            ));
        }
        let terms = archive_terms(&input.query);
        let mut queries = Vec::new();
        if prepared.effective_mode != "vector" {
            queries.push(("keyword", ArchiveQuery::Keyword(&terms)));
        }
        if let Some(vector) = &prepared.embedding {
            queries.push(("vector", ArchiveQuery::Vector(vector)));
        }
        let mut ranked = HashMap::<String, (f64, Vec<String>)>::new();
        for (route, query) in &queries {
            let mut hits = Vec::new();
            for batch in groups.chunks(64) {
                // Check that the rankable projection of every eligible archive
                // is present, including records that would not match the query.
                if *route == queries[0].0 {
                    let (count, _) = backend
                        .archive_search(domain, ArchiveQuery::Keyword(""), batch, 1)
                        .await?;
                    if count != batch.len() {
                        return Err(AppError::Unavailable(
                            "private archive search projection unavailable".into(),
                        ));
                    }
                }
                let (_, found) = backend.archive_search(domain, *query, batch, limit).await?;
                hits.extend(found);
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
        ranked.sort_by(|a, b| {
            b.1.0
                .total_cmp(&a.1.0)
                .then_with(|| eligible[&b.0].sequence.cmp(&eligible[&a.0].sequence))
        });
        let mut found = Vec::new();
        for (group, (score, routes)) in ranked.into_iter().take(limit) {
            let entry = &eligible[&group];
            found.push(ArchiveCandidate {
                entry: entry.clone(),
                body: self.archived_body(entry).await?,
                score,
                routes,
            });
        }
        Ok((groups.len(), found))
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
        let mut tx = self.read(a, Permission::Write).await?;
        let value = tx.session_feedback(session, turn).await?;
        tx.commit().await?;
        Ok(value)
    }
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
