//! Immutable, relational-only archives. No native artifacts or background jobs.
use super::*;
use crate::session::{ArchivedEvidence, EvidenceKind, FeedbackInput, FeedbackTarget, SessionTurnInput, UsedEvidence};

pub(super) const SCHEMA: &str = include_str!("../session-schema.sql");

pub(super) async fn available(conn: &mut SqliteConnection) -> StorageResult<bool> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name IN ('oc_session_turns','oc_session_turn_evidence','oc_feedback')) OR EXISTS(SELECT 1 FROM oc_events WHERE kind='session_turn')")
        .fetch_one(conn).await.map_err(sqlite_err)
}

pub(super) async fn check_schema(conn: &mut SqliteConnection) -> StorageResult<()> {
    let normalize = |s: &str| s.split_whitespace().collect::<String>().to_ascii_lowercase();
    for statement in SCHEMA.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let name = statement.split_whitespace().nth(2).ok_or_else(|| StorageError::Unavailable("invalid session schema".into()))?;
        let actual: Option<String> = sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name=?")
            .bind(name).fetch_optional(&mut *conn).await.map_err(sqlite_err)?;
        if actual.as_deref().map(normalize) != Some(normalize(statement)) {
            return Err(StorageError::Unavailable("incompatible session schema".into()));
        }
    }
    Ok(())
}

fn decode<T: serde::de::DeserializeOwned>(s: &str) -> StorageResult<T> {
    serde_json::from_str(s).map_err(|_| StorageError::Unavailable("invalid session record".into()))
}
fn encode<T: serde::Serialize>(value: &T) -> StorageResult<String> {
    serde_json::to_string(value).map_err(|_| StorageError::Unavailable("invalid session record".into()))
}

impl SqliteTx {
    async fn require_sessions(&mut self) -> StorageResult<()> {
        if !available(&mut self.tx).await? {
            return Err(StorageError::Unavailable("session archives require a new database".into()));
        }
        Ok(())
    }

    /// Exact historical version, with complete S2 support. Never use current-only retrieval.
    async fn archive_evidence(&mut self, reference: &UsedEvidence, evidence_ref_id: Uuid) -> StorageResult<ArchivedEvidence> {
        reference.validate().map_err(StorageError::InvalidArgument)?;
        let version = self.asset_view(reference.asset_id, Some(reference.asset_version)).await?;
        let root: Uuid = serde_json::from_value(version["source_event_id"].clone())
            .map_err(|_| StorageError::Unavailable("invalid version provenance".into()))?;
        let locator: Option<String> = sqlx::query_scalar("SELECT locator FROM oc_chunks WHERE tenant_id=? AND workspace_id=? AND id=? AND asset_id=? AND version=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(reference.chunk_id)
            .bind(reference.asset_id).bind(reference.asset_version).fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        let locator: Value = decode(&locator.ok_or(StorageError::NotFound)?)?;
        let chunk_owned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_artifact_owners WHERE tenant_id=? AND workspace_id=? AND source_id=? AND version=? AND artifact_type='chunk' AND artifact_id=? AND chunk_id=?)")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(root).bind(reference.asset_version)
            .bind(reference.chunk_id).bind(reference.chunk_id).fetch_one(&mut *self.tx).await.map_err(sqlite_err)?;
        if !chunk_owned { return Err(StorageError::NotFound); }
        if reference.kind != EvidenceKind::Chunk {
            let kind = match reference.kind { EvidenceKind::Entity => "entity", EvidenceKind::Relation => "relation", EvidenceKind::Chunk => unreachable!() };
            // Existing graph ownership is source/version-level (chunk_id is NULL).
            // Require the caller's chosen chunk to belong to that same exact support.
            let owned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_artifact_owners o JOIN oc_artifact_ledger l ON l.tenant_id=o.tenant_id AND l.workspace_id=o.workspace_id AND l.source_id=o.source_id AND l.version=o.version AND l.artifact_type=o.artifact_type AND l.artifact_id=o.artifact_id WHERE o.tenant_id=? AND o.workspace_id=? AND o.source_id=? AND o.version=? AND o.artifact_type=? AND o.artifact_id=? AND l.surface='graph' AND l.state='committed')")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(root).bind(reference.asset_version)
                .bind(kind).bind(reference.id).fetch_one(&mut *self.tx).await.map_err(sqlite_err)?;
            if !owned { return Err(StorageError::NotFound); }
        }
        let (source_event_id, source_version) = if locator["parser"] == crate::memory_semantics::PARSER {
            let source: Uuid = serde_json::from_value(locator["source_event_id"].clone())
                .map_err(|_| StorageError::Unavailable("invalid semantic provenance".into()))?;
            let version: i32 = serde_json::from_value(locator["source_version"].clone())
                .map_err(|_| StorageError::Unavailable("invalid semantic provenance".into()))?;
            if version <= 0 { return Err(StorageError::Unavailable("invalid semantic provenance".into())); }
            (source, version)
        } else { (root, reference.asset_version) };
        if self.event_content(source_event_id, true).await?.is_none() || self.event_content(root, true).await?.is_none() {
            return Err(StorageError::NotFound);
        }
        Ok(ArchivedEvidence { evidence_ref_id, reference: reference.clone(), source_event_id, source_version, version_source_event_id: root, locator })
    }

    async fn turn_evidence(&mut self, session: Uuid, turn: Uuid) -> StorageResult<Vec<ArchivedEvidence>> {
        let rows: Vec<String> = sqlx::query_scalar("SELECT evidence FROM oc_session_turn_evidence WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=? ORDER BY ordinal")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
            .fetch_all(&mut *self.tx).await.map_err(sqlite_err)?;
        rows.iter().map(|s| decode(s)).collect()
    }

    pub async fn require_turn_visible(&mut self, session: Uuid, turn: Uuid) -> StorageResult<Uuid> {
        self.check_permission(Permission::Write).await?;
        self.require_sessions().await?;
        let source: Option<Uuid> = sqlx::query_scalar("SELECT t.source_event_id FROM oc_session_turns t JOIN oc_events e ON e.tenant_id=t.tenant_id AND e.workspace_id=t.workspace_id AND e.id=t.source_event_id WHERE t.tenant_id=? AND t.workspace_id=? AND t.session_id=? AND t.turn_id=? AND e.state='active'")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        let source = source.ok_or(StorageError::NotFound)?;
        for evidence in self.turn_evidence(session, turn).await? {
            let current = self.archive_evidence(&evidence.reference, evidence.evidence_ref_id).await?;
            if current != evidence { return Err(StorageError::NotFound); }
        }
        Ok(source)
    }

    /// Also gate the existing raw-source endpoint, so it cannot bypass withdrawal.
    pub(super) async fn require_session_source_visible(&mut self, source: Uuid) -> StorageResult<()> {
        if !available(&mut self.tx).await? { return Err(StorageError::NotFound); }
        let turn: Option<(Uuid, Uuid)> = sqlx::query_as("SELECT session_id,turn_id FROM oc_session_turns WHERE tenant_id=? AND workspace_id=? AND source_event_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(source)
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        let (session, turn) = turn.ok_or(StorageError::NotFound)?;
        self.require_turn_visible(session, turn).await?;
        Ok(())
    }

    pub async fn archive_turn(&mut self, session: Uuid, input: &SessionTurnInput, request_hash: &str) -> StorageResult<Value> {
        self.check_permission(Permission::Write).await?;
        self.require_sessions().await?;
        input.validate().map_err(StorageError::InvalidArgument)?;
        if session.is_nil() { return Err(StorageError::InvalidArgument("session_id must be non-nil")); }
        let existing: Option<(Uuid, String)> = sqlx::query_as("SELECT source_event_id,request_hash FROM oc_session_turns WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(input.turn_id)
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        let source = if let Some((source, hash)) = existing {
            if hash != request_hash { return Err(StorageError::Conflict("turn is immutable".into())); }
            self.require_turn_visible(session, input.turn_id).await?;
            source
        } else {
            let mut evidence = Vec::new();
            for reference in &input.used_evidence { evidence.push(self.archive_evidence(reference, Uuid::new_v4()).await?); }
            let source = self.create_event("session_turn", &json!({"question":input.question,"answer":input.answer}).to_string(), None).await?;
            sqlx::query("INSERT INTO oc_session_turns(tenant_id,workspace_id,session_id,turn_id,source_event_id,request_hash,created_by,created_at) VALUES(?,?,?,?,?,?,?,?)")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(input.turn_id)
                .bind(source).bind(request_hash).bind(self.principal_id).bind(now_ms()).execute(&mut *self.tx).await.map_err(sqlite_err)?;
            for (ordinal, item) in evidence.iter().enumerate() {
                sqlx::query("INSERT INTO oc_session_turn_evidence(tenant_id,workspace_id,session_id,turn_id,evidence_ref_id,ordinal,evidence) VALUES(?,?,?,?,?,?,?)")
                    .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(input.turn_id)
                    .bind(item.evidence_ref_id).bind(ordinal as i32).bind(encode(item)?).execute(&mut *self.tx).await.map_err(sqlite_err)?;
            }
            self.audit("session.turn_archived", input.turn_id, json!({"session_id":session,"source_event_id":source,"evidence_count":evidence.len()})).await?;
            source
        };
        Ok(json!({"session_id":session,"turn_id":input.turn_id,"source_event_id":source,"state":"archived"}))
    }

    pub async fn session_turn(&mut self, session: Uuid, turn: Uuid) -> StorageResult<Value> {
        let source = self.require_turn_visible(session, turn).await?;
        let body = self.event_content(source, true).await?.ok_or(StorageError::NotFound)?;
        let body: Value = decode(&body)?;
        let (created_by, created_at): (Uuid, i64) = sqlx::query_as("SELECT created_by,created_at FROM oc_session_turns WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
            .fetch_one(&mut *self.tx).await.map_err(sqlite_err)?;
        let evidence = self.turn_evidence(session, turn).await?;
        let feedback = self.feedback_rows(session, turn).await?;
        Ok(json!({"session_id":session,"turn_id":turn,"source_event_id":source,"question":body["question"],"answer":body["answer"],"used_evidence":evidence,"feedback":feedback,"created_by":created_by,"created_at":created_at}))
    }

    async fn feedback_rows(&mut self, session: Uuid, turn: Uuid) -> StorageResult<Vec<Value>> {
        let rows: Vec<(Uuid, String, i64)> = sqlx::query_as("SELECT principal_id,feedback,created_at FROM oc_feedback WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=? ORDER BY created_at,principal_id,feedback_id")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
            .fetch_all(&mut *self.tx).await.map_err(sqlite_err)?;
        rows.into_iter().map(|(principal, body, created_at)| { let mut value: Value = decode(&body)?; value["created_by"]=json!(principal); value["created_at"]=json!(created_at); Ok(value) }).collect()
    }

    pub async fn session_feedback(&mut self, session: Uuid, turn: Uuid) -> StorageResult<Value> {
        self.require_turn_visible(session, turn).await?;
        Ok(json!({"session_id":session,"turn_id":turn,"feedback":self.feedback_rows(session, turn).await?}))
    }

    pub async fn record_feedback(&mut self, session: Uuid, turn: Uuid, input: &FeedbackInput, request_hash: &str) -> StorageResult<Value> {
        input.validate().map_err(StorageError::InvalidArgument)?;
        if session.is_nil() || turn.is_nil() { return Err(StorageError::InvalidArgument("session and turn identifiers must be non-nil")); }
        self.require_turn_visible(session, turn).await?;
        if let FeedbackTarget::Evidence { evidence_ref_id } = &input.target {
            let found: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_session_turn_evidence WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=? AND evidence_ref_id=?)")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn).bind(evidence_ref_id)
                .fetch_one(&mut *self.tx).await.map_err(sqlite_err)?;
            if !found { return Err(StorageError::NotFound); }
        }
        let existing: Option<String> = sqlx::query_scalar("SELECT request_hash FROM oc_feedback WHERE tenant_id=? AND workspace_id=? AND principal_id=? AND feedback_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(self.principal_id).bind(input.feedback_id)
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        if let Some(hash) = existing {
            if hash != request_hash { return Err(StorageError::Conflict("feedback is immutable".into())); }
        } else {
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM oc_feedback WHERE tenant_id=? AND workspace_id=? AND session_id=? AND turn_id=?")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn)
                .fetch_one(&mut *self.tx).await.map_err(sqlite_err)?;
            if count >= 256 { return Err(StorageError::Conflict("turn feedback limit reached".into())); }
            sqlx::query("INSERT INTO oc_feedback(tenant_id,workspace_id,principal_id,feedback_id,session_id,turn_id,request_hash,feedback,created_at) VALUES(?,?,?,?,?,?,?,?,?)")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(self.principal_id).bind(input.feedback_id)
                .bind(session).bind(turn).bind(request_hash).bind(encode(input)?).bind(now_ms()).execute(&mut *self.tx).await.map_err(sqlite_err)?;
            self.audit("session.feedback_recorded", input.feedback_id, json!({"session_id":session,"turn_id":turn})).await?;
        }
        Ok(json!({"session_id":session,"turn_id":turn,"feedback_id":input.feedback_id,"state":"recorded"}))
    }
}
