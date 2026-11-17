//! JOIN-free private Run storage; ownership is checked before replay and readback.
use super::*;
use crate::run::{RunEvent, RunEventInput, RunInput};
use crate::session::ArchivedEvidence;
pub(super) const SCHEMA: &str = include_str!("../run-schema.sql");
fn encode<T: serde::Serialize>(v: &T) -> StorageResult<String> {
    serde_json::to_string(v).map_err(|_| StorageError::InvalidArgument("invalid run payload"))
}
fn decode<T: serde::de::DeserializeOwned>(s: &str) -> StorageResult<T> {
    serde_json::from_str(s).map_err(|_| StorageError::Unavailable("invalid run record".into()))
}
impl TursoTx {
    pub(crate) async fn owned_run(&mut self, run: Uuid) -> StorageResult<(Uuid, String, i64)> {
        self.check_permission(Permission::Read).await?;
        let key = self.private_key_id().await?;
        let row:Option<(Uuid,Uuid,String,i64)>=native_turso::query_as("SELECT session_id,owner_key_id,input,created_at FROM oc_runs WHERE tenant_id=? AND workspace_id=? AND run_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(run).fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        let (session, owner, input, created) = row.ok_or(StorageError::NotFound)?;
        if owner != key {
            return Err(StorageError::NotFound);
        }
        self.require_private_session(session).await?;
        Ok((session, input, created))
    }
    pub async fn create_private_run(&mut self, input: &RunInput) -> StorageResult<Value> {
        self.check_permission(Permission::Write).await?;
        input.validate().map_err(StorageError::InvalidArgument)?;
        let session = self.private_session_id().await?;
        let owner = self.private_key_id().await?;
        let payload = encode(input)?;
        let digest = hash(&payload);
        let existing: Option<String> = native_turso::query_scalar(
            "SELECT input_hash FROM oc_runs WHERE tenant_id=? AND workspace_id=? AND run_id=?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(input.run_id)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(turso_err)?;
        if let Some(old) = existing {
            self.owned_run(input.run_id).await?;
            if old != digest {
                return Err(StorageError::Conflict(
                    "run identity reused with different input".into(),
                ));
            }
        } else {
            native_turso::query("INSERT INTO oc_runs(tenant_id,workspace_id,run_id,session_id,owner_key_id,input,input_hash,created_at) VALUES(?,?,?,?,?,?,?,?)")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(input.run_id).bind(session).bind(owner).bind(payload).bind(digest).bind(now_ms())
                .execute(&mut *self.tx).await.map_err(turso_err)?;
        }
        Ok(json!({"run_id":input.run_id,"session_id":session}))
    }
    pub async fn append_private_run_event(
        &mut self,
        run: Uuid,
        input: &RunEventInput,
    ) -> StorageResult<Value> {
        self.check_permission(Permission::Write).await?;
        self.owned_run(run).await?;
        input.validate().map_err(StorageError::InvalidArgument)?;
        let payload = encode(input)?;
        let digest = hash(&payload);
        let old:Option<(String,i64)>=native_turso::query_as("SELECT payload_hash,sequence FROM oc_run_events WHERE tenant_id=? AND workspace_id=? AND run_id=? AND event_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(run).bind(input.event_id).fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        if let Some((old, seq)) = old {
            if old != digest {
                return Err(StorageError::Conflict(
                    "event identity reused with different payload".into(),
                ));
            }
            return Ok(json!({"run_id":run,"event_id":input.event_id,"sequence":seq}));
        }
        let mut refs = Vec::new();
        if let RunEvent::ContextReported { references } = &input.event {
            for r in references {
                let reference = self.archive_evidence_local(r, Uuid::new_v4()).await?;
                self.require_run_reference_visible(&reference).await?;
                refs.push(reference);
            }
        }
        let last:i64=native_turso::query_scalar("SELECT COALESCE(MAX(sequence),0) FROM oc_run_events WHERE tenant_id=? AND workspace_id=? AND run_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(run).fetch_one(&mut *self.tx).await.map_err(turso_err)?;
        if last >= 10000 {
            return Err(StorageError::InvalidArgument("run event budget exceeded"));
        }
        let seq = last + 1;
        native_turso::query("INSERT INTO oc_run_events(tenant_id,workspace_id,run_id,event_id,sequence,reporter_principal_id,payload,payload_hash,references_json,recorded_at) VALUES(?,?,?,?,?,?,?,?,?,?)")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(run).bind(input.event_id).bind(seq).bind(self.principal_id).bind(payload).bind(digest).bind(encode(&refs)?).bind(now_ms())
            .execute(&mut *self.tx).await.map_err(turso_err)?;
        Ok(json!({"run_id":run,"event_id":input.event_id,"sequence":seq}))
    }
    pub async fn private_run_trace(
        &mut self,
        run: Uuid,
        after: i64,
        limit: usize,
    ) -> StorageResult<Value> {
        let (session, input, created) = self.owned_run(run).await?;
        if after < 0 || limit == 0 || limit > 100 {
            return Err(StorageError::InvalidArgument("invalid run page"));
        }
        let rows:Vec<(i64,Uuid,String,String,i64)>=native_turso::query_as("SELECT sequence,reporter_principal_id,payload,references_json,recorded_at FROM oc_run_events WHERE tenant_id=? AND workspace_id=? AND run_id=? AND sequence>? ORDER BY sequence LIMIT ?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(run).bind(after).bind((limit+1) as i64).fetch_all(&mut *self.tx).await.map_err(turso_err)?;
        let more = rows.len() > limit;
        let mut events = Vec::new();
        let mut cursor = after;
        for (seq, principal, payload, references, recorded) in rows.into_iter().take(limit) {
            let refs: Vec<ArchivedEvidence> = decode(&references)?;
            for reference in &refs {
                self.require_run_reference_visible(reference).await?;
            }
            let mut report: Value = decode(&payload)?;
            self.validate_prepared_payload(&report).await?;
            // The private retry snapshot is not duplicated in paginated traces.
            if let Some(object) = report.as_object_mut() {
                object.remove("replay_bundle");
            }
            events.push(json!({"sequence":seq,"reporter_principal_id":principal,"report":report,"references":refs,"recorded_at_ms":recorded,"evidence_origin":if report["kind"]=="context_prepared" {"server_prepared"} else if report["kind"]=="context_preparation_failed" {"server_observed"} else {"reported"}}));
            cursor = seq;
        }
        Ok(
            json!({"run":decode::<RunInput>(&input)?,"session_id":session,"created_at_ms":created,"events":events,"next_after":if more{Some(cursor)}else{None},"unreported_activity":"unknown","coverage_notice":"Only explicitly recorded events are visible; MCP cannot observe unrelated client or tool activity."}),
        )
    }
    /// Digest of an immutable, authorized event prefix for private Skill evidence.
    /// Appending later events does not change a previously selected prefix.
    pub async fn private_run_prefix_digest(
        &mut self,
        run: Uuid,
        through_sequence: i64,
    ) -> StorageResult<String> {
        self.owned_run(run).await?;
        let owner = self.private_key_id().await?;
        self.run_prefix_digest_for_owner(owner, run, through_sequence)
            .await
    }
    /// Internal digest-only check for a stored, explicitly selected Skill basis.
    /// Never expose this owner parameter through a caller-controlled route.
    pub(super) async fn run_prefix_digest_for_owner(
        &mut self,
        owner_key_id: Uuid,
        run: Uuid,
        through_sequence: i64,
    ) -> StorageResult<String> {
        self.check_permission(Permission::Read).await?;
        let input:Option<String>=native_turso::query_scalar("SELECT input FROM oc_runs WHERE tenant_id=? AND workspace_id=? AND run_id=? AND owner_key_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(run).bind(owner_key_id).fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        let input = input.ok_or(StorageError::NotFound)?;
        if !(0..=10000).contains(&through_sequence) {
            return Err(StorageError::InvalidArgument("invalid run prefix"));
        }
        let rows:Vec<(i64,String,String,String)>=native_turso::query_as("SELECT sequence,payload_hash,references_json,payload FROM oc_run_events WHERE tenant_id=? AND workspace_id=? AND run_id=? AND sequence<=? ORDER BY sequence")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(run).bind(through_sequence).fetch_all(&mut *self.tx).await.map_err(turso_err)?;
        if rows.len() != through_sequence as usize {
            return Err(StorageError::NotFound);
        }
        for (_, _, references, payload) in &rows {
            self.validate_prepared_payload(&decode::<Value>(payload)?)
                .await?;
            for r in decode::<Vec<ArchivedEvidence>>(references)? {
                self.require_run_reference_visible(&r).await?;
            }
        }
        Ok(hash(encode(&(
            self.scope.tenant_id,
            self.scope.workspace_id,
            run,
            input,
            rows,
        ))?))
    }
}

impl TursoTx {
    /// Revalidate private ownership and all original sources before replay.
    #[cfg_attr(not(feature = "local-storage"), allow(dead_code))]
    pub(crate) async fn replay_run_context_prepared(
        &mut self,
        run: Uuid,
        event_id: Uuid,
        request_digest: &str,
    ) -> StorageResult<Option<Value>> {
        self.check_permission(Permission::Write).await?;
        self.owned_run(run).await?;
        if event_id.is_nil() || !crate::retrieval_diagnostics::digest(request_digest) {
            return Err(StorageError::InvalidArgument(
                "invalid preparation identity",
            ));
        }
        let old: Option<(String, i64, String)> = native_turso::query_as("SELECT payload,sequence,references_json FROM oc_run_events WHERE tenant_id=? AND workspace_id=? AND run_id=? AND event_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(run).bind(event_id)
            .fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        let Some((payload, seq, references)) = old else {
            return Ok(None);
        };
        let payload: Value = decode(&payload)?;
        if payload["context_request_digest"] != request_digest {
            return Err(StorageError::Conflict(
                "event identity reused with different payload".into(),
            ));
        }
        if payload["kind"] == "context_preparation_failed" {
            return Ok(None);
        }
        if payload["kind"] != "context_prepared" {
            return Err(StorageError::Conflict(
                "event identity reused with different payload".into(),
            ));
        }
        for reference in decode::<Vec<ArchivedEvidence>>(&references)? {
            self.require_run_reference_visible(&reference).await?;
        }
        self.validate_prepared_payload(&payload).await?;
        let bundle: crate::evidence::EvidenceBundle =
            serde_json::from_value(payload["replay_bundle"].clone()).map_err(|_| {
                StorageError::Unavailable("invalid prepared context snapshot".into())
            })?;
        if bundle.tenant_id != self.scope.tenant_id
            || bundle.workspace_id != self.scope.workspace_id
            || payload["bundle_digest"] != hash(encode(&bundle)?)
        {
            return Err(StorageError::Unavailable(
                "invalid prepared context snapshot".into(),
            ));
        }
        Ok(Some(
            json!({"run_id":run,"event_id":event_id,"sequence":seq,"recorded_bundle":bundle}),
        ))
    }

    async fn require_run_reference_visible(
        &mut self,
        reference: &ArchivedEvidence,
    ) -> StorageResult<()> {
        let current = self
            .archive_evidence_local(&reference.reference, reference.evidence_ref_id)
            .await?;
        if current != *reference {
            return Err(StorageError::NotFound);
        }
        self.require_archived_evidence_visible(reference).await
    }

    async fn validate_prepared_payload(&mut self, payload: &Value) -> StorageResult<()> {
        if payload["kind"] != "context_prepared" {
            return Ok(());
        }
        let citations: Vec<crate::evidence::EvidenceProvenance> =
            serde_json::from_value(payload["citations"].clone())
                .map_err(|_| StorageError::Unavailable("invalid prepared citations".into()))?;
        for citation in citations {
            if let crate::evidence::EvidenceProvenance::SessionTurn {
                session_id,
                turn_id,
                source_event_id,
                ..
            } = citation
            {
                self.require_private_session(session_id).await?;
                if self.require_turn_visible(session_id, turn_id).await? != source_event_id {
                    return Err(StorageError::NotFound);
                }
            }
        }
        Ok(())
    }
    /// Only service-produced bundles may enter this path; public client event DTOs
    /// cannot deserialize a prepared event or choose its server attribution.
    #[cfg_attr(not(feature = "local-storage"), allow(dead_code))]
    pub(crate) async fn record_run_context_prepared(
        &mut self,
        run: Uuid,
        event_id: Uuid,
        request_digest: &str,
        bundle: &crate::evidence::EvidenceBundle,
    ) -> StorageResult<Value> {
        use crate::evidence::EvidenceProvenance;
        self.check_permission(Permission::Write).await?;
        self.owned_run(run).await?;
        if event_id.is_nil()
            || !crate::retrieval_diagnostics::digest(request_digest)
            || bundle.tenant_id != self.scope.tenant_id
            || bundle.workspace_id != self.scope.workspace_id
        {
            return Err(StorageError::InvalidArgument(
                "invalid prepared context scope",
            ));
        }
        if let Some(prepared) = self
            .replay_run_context_prepared(run, event_id, request_digest)
            .await?
        {
            return Ok(prepared);
        }
        let mut refs = Vec::new();
        let citations: Vec<_> = bundle.items.iter().map(|i| i.provenance.clone()).collect();
        for citation in &citations {
            match citation {
                EvidenceProvenance::AssetChunk {
                    chunk_id,
                    asset_id,
                    asset_version,
                    source_event_id,
                    source_version,
                    version_root_source_event_id,
                    locator,
                } => {
                    let r = ArchivedEvidence {
                        evidence_ref_id: Uuid::new_v4(),
                        reference: crate::session::UsedEvidence {
                            kind: crate::session::EvidenceKind::Chunk,
                            id: *chunk_id,
                            asset_id: *asset_id,
                            asset_version: *asset_version,
                            chunk_id: *chunk_id,
                        },
                        source_event_id: *source_event_id,
                        source_version: *source_version,
                        version_source_event_id: *version_root_source_event_id,
                        locator: locator.clone(),
                    };
                    self.require_run_reference_visible(&r).await?;
                    refs.push(r);
                }
                EvidenceProvenance::SessionTurn { used_evidence, .. } => {
                    for r in used_evidence {
                        self.require_run_reference_visible(r).await?;
                        refs.push(r.clone());
                    }
                }
            }
        }
        let old: Option<i64> = native_turso::query_scalar("SELECT sequence FROM oc_run_events WHERE tenant_id=? AND workspace_id=? AND run_id=? AND event_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(run).bind(event_id)
            .fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        if old.is_some() {
            return Err(StorageError::Conflict(
                "event identity reused with different payload".into(),
            ));
        }
        let payload = json!({"kind":"context_prepared","event_id":event_id,"reporter":"server","context_request_digest":request_digest,"bundle_digest":hash(encode(bundle)?),"replay_bundle":bundle,"citations":citations,"retrieval":bundle.retrieval,"routes":bundle.routes,"warnings":bundle.warnings,"budget":bundle.budget,"used_bytes":bundle.used_bytes,"omitted_conflict_groups":bundle.omitted_conflict_groups,"delivery_status":"prepared_not_confirmed_received_or_used"});
        self.validate_prepared_payload(&payload).await?;
        let payload = encode(&payload)?;
        let digest = hash(&payload);
        let last:i64=native_turso::query_scalar("SELECT COALESCE(MAX(sequence),0) FROM oc_run_events WHERE tenant_id=? AND workspace_id=? AND run_id=?").bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(run).fetch_one(&mut *self.tx).await.map_err(turso_err)?;
        if last >= 10000 {
            return Err(StorageError::InvalidArgument("run event budget exceeded"));
        }
        let seq = last + 1;
        native_turso::query("INSERT INTO oc_run_events(tenant_id,workspace_id,run_id,event_id,sequence,reporter_principal_id,payload,payload_hash,references_json,recorded_at) VALUES(?,?,?,?,?,?,?,?,?,?)").bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(run).bind(event_id).bind(seq).bind(self.principal_id).bind(payload).bind(digest).bind(encode(&refs)?).bind(now_ms()).execute(&mut *self.tx).await.map_err(turso_err)?;
        Ok(json!({"run_id":run,"event_id":event_id,"sequence":seq,"recorded_bundle":bundle}))
    }
}

impl TursoTx {
    /// Record a failed server preparation attempt without any raw query or error body.
    #[cfg(feature = "local-storage")]
    pub(crate) async fn record_run_context_failed(
        &mut self,
        run: Uuid,
        event_id: Uuid,
        request_digest: &str,
        error_code: &'static str,
        retrieval: &crate::retrieval_diagnostics::RetrievalDiagnostics,
    ) -> StorageResult<Option<Value>> {
        self.check_permission(Permission::Write).await?;
        self.owned_run(run).await?;
        if event_id.is_nil() || !crate::retrieval_diagnostics::digest(request_digest) {
            return Err(StorageError::InvalidArgument(
                "invalid preparation event ID",
            ));
        }
        if let Some(prepared) = self
            .replay_run_context_prepared(run, event_id, request_digest)
            .await?
        {
            return Ok(Some(prepared));
        }
        let payload = encode(
            &json!({"kind":"context_preparation_failed","event_id":event_id,"reporter":"server",
            "context_request_digest":request_digest,"error_code":error_code,"retrieval":retrieval,"delivery_status":"not_prepared"}),
        )?;
        let digest = hash(&payload);
        let old: Option<String> = native_turso::query_scalar("SELECT payload FROM oc_run_events WHERE tenant_id=? AND workspace_id=? AND run_id=? AND event_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(run).bind(event_id)
            .fetch_optional(&mut *self.tx).await.map_err(turso_err)?;
        if let Some(old) = old {
            let old: Value = decode(&old)?;
            if old["kind"] != "context_preparation_failed"
                || old["context_request_digest"] != request_digest
                || old["error_code"] != error_code
            {
                return Err(StorageError::Conflict(
                    "event identity reused with different payload".into(),
                ));
            }
            return Ok(None);
        }
        let last: i64 = native_turso::query_scalar("SELECT COALESCE(MAX(sequence),0) FROM oc_run_events WHERE tenant_id=? AND workspace_id=? AND run_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(run).fetch_one(&mut *self.tx).await.map_err(turso_err)?;
        if last >= 10000 {
            return Err(StorageError::InvalidArgument("run event budget exceeded"));
        }
        native_turso::query("INSERT INTO oc_run_events(tenant_id,workspace_id,run_id,event_id,sequence,reporter_principal_id,payload,payload_hash,references_json,recorded_at) VALUES(?,?,?,?,?,?,?,?,?,?)")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(run).bind(event_id).bind(last+1)
            .bind(self.principal_id).bind(payload).bind(digest).bind("[]").bind(now_ms()).execute(&mut *self.tx).await.map_err(turso_err)?;
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::{RunOutcome, RunReporter};
    async fn tx(pool: &Pool, scope: Scope, principal_id: Uuid) -> TursoTx {
        TursoTx {
            tx: pool.begin_with("BEGIN IMMEDIATE").await.unwrap(),
            scope,
            principal_id,
            required: Vec::new(),
            command: None,
        }
    }
    #[tokio::test]
    async fn private_run_durable_replay_isolation_and_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runs.db");
        let pool = Pool::open(&path).await.unwrap();
        {
            let mut c = pool.acquire().await.unwrap();
            super::super::schema::exec_schema(&mut c).await.unwrap();
        }
        let scope = Scope {
            tenant_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
        };
        native_turso::query(
            "INSERT INTO oc_workspaces(tenant_id,id,name,created_at) VALUES(?,?,'test',0)",
        )
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .execute(&pool)
        .await
        .unwrap();
        let owner = Uuid::new_v4();
        let other = Uuid::new_v4();
        for id in [owner, other] {
            native_turso::query("INSERT INTO oc_api_keys(id,logical_key_id,tenant_id,workspace_id,token_hash,role,created_at) VALUES(?,?,?,?,?,'admin',0)").bind(id).bind(id).bind(scope.tenant_id).bind(scope.workspace_id).bind(id.to_string()).execute(&pool).await.unwrap();
        }
        let input = RunInput {
            run_id: Uuid::new_v4(),
            summary: "inspect failure".into(),
        };
        let mut t = tx(&pool, scope, owner).await;
        let receipt = t.create_private_run(&input).await.unwrap();
        assert_eq!(receipt, t.create_private_run(&input).await.unwrap());
        let mut changed = input.clone();
        changed.summary = "different".into();
        assert!(matches!(
            t.create_private_run(&changed).await,
            Err(StorageError::Conflict(_))
        ));
        let event = RunEventInput {
            event_id: Uuid::new_v4(),
            reporter: RunReporter::Client,
            observed_at_ms: 1,
            event: RunEvent::ResultReported {
                outcome: RunOutcome::Unknown,
                summary: "tool unavailable".into(),
            },
        };
        let missing = Uuid::new_v4();
        let forged = RunEventInput {
            event_id: Uuid::new_v4(),
            reporter: RunReporter::Client,
            observed_at_ms: 1,
            event: RunEvent::ContextReported {
                references: vec![crate::session::UsedEvidence {
                    kind: crate::session::EvidenceKind::Chunk,
                    id: missing,
                    chunk_id: missing,
                    asset_id: Uuid::new_v4(),
                    asset_version: 1,
                }],
            },
        };
        assert!(
            t.append_private_run_event(input.run_id, &forged)
                .await
                .is_err()
        );
        let first = t
            .append_private_run_event(input.run_id, &event)
            .await
            .unwrap();
        assert_eq!(
            first,
            t.append_private_run_event(input.run_id, &event)
                .await
                .unwrap()
        );
        let digest = t.private_run_prefix_digest(input.run_id, 1).await.unwrap();
        let mut changed = event.clone();
        changed.observed_at_ms = 2;
        assert!(matches!(
            t.append_private_run_event(input.run_id, &changed).await,
            Err(StorageError::Conflict(_))
        ));
        changed.event_id = Uuid::new_v4();
        t.append_private_run_event(input.run_id, &changed)
            .await
            .unwrap();
        assert_eq!(
            digest,
            t.private_run_prefix_digest(input.run_id, 1).await.unwrap()
        );
        assert!(t.private_run_prefix_digest(input.run_id, 3).await.is_err());
        let bundle = crate::evidence::EvidenceBundle {
            retrieval: Default::default(),
            tenant_id: scope.tenant_id,
            workspace_id: scope.workspace_id,
            policy: "test".into(),
            validity: "snapshot".into(),
            conflict_assessment: "unknown".into(),
            requested_mode: "keyword".into(),
            effective_mode: "keyword".into(),
            rendered_context: "".into(),
            items: vec![],
            conflicts: vec![],
            warnings: vec!["no context found".into()],
            budget: 100,
            used_bytes: 0,
            omitted_conflict_groups: 0,
            routes: vec![],
        };
        let prepared_id = Uuid::new_v4();
        let prepared = t
            .record_run_context_prepared(input.run_id, prepared_id, &hash("request"), &bundle)
            .await
            .unwrap();
        assert_eq!(
            prepared,
            t.record_run_context_prepared(input.run_id, prepared_id, &hash("request"), &bundle)
                .await
                .unwrap()
        );
        let mut altered = bundle.clone();
        altered.budget = 101;
        assert_eq!(
            prepared,
            t.record_run_context_prepared(input.run_id, prepared_id, &hash("request"), &altered)
                .await
                .unwrap()
        );
        #[cfg(feature = "local-storage")]
        assert_eq!(
            Some(prepared.clone()),
            t.record_run_context_failed(
                input.run_id,
                prepared_id,
                &hash("request"),
                "DEPENDENCY_UNAVAILABLE",
                &Default::default()
            )
            .await
            .unwrap()
        );
        assert!(matches!(
            t.record_run_context_prepared(
                input.run_id,
                prepared_id,
                &hash("changed request"),
                &altered
            )
            .await,
            Err(StorageError::Conflict(_))
        ));
        let prepared_trace = t.private_run_trace(input.run_id, 2, 1).await.unwrap();
        assert_eq!(
            prepared_trace["events"][0]["evidence_origin"],
            "server_prepared"
        );
        assert_eq!(
            prepared_trace["events"][0]["report"]["delivery_status"],
            "prepared_not_confirmed_received_or_used"
        );
        t.commit().await.unwrap();
        pool.shutdown().await.unwrap();
        drop(pool);
        let pool = Pool::open(&path).await.unwrap();
        let mut t = tx(&pool, scope, owner).await;
        let trace = t.private_run_trace(input.run_id, 0, 1).await.unwrap();
        assert_eq!(trace["events"].as_array().unwrap().len(), 1);
        assert_eq!(trace["next_after"], 1);
        assert_eq!(trace["unreported_activity"], "unknown");
        assert_eq!(trace["events"][0]["evidence_origin"], "reported");
        assert_eq!(trace["events"][0]["reporter_principal_id"], json!(owner));
        assert_eq!(
            t.private_run_trace(input.run_id, 1, 1).await.unwrap()["events"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        t.commit().await.unwrap();
        let mut t = tx(&pool, scope, other).await;
        assert!(matches!(
            t.private_run_trace(input.run_id, 0, 10).await,
            Err(StorageError::NotFound)
        ));
        assert!(matches!(
            t.append_private_run_event(input.run_id, &event).await,
            Err(StorageError::NotFound)
        ));
        assert!(matches!(
            t.create_private_run(&input).await,
            Err(StorageError::NotFound)
        ));
        t.commit().await.unwrap();
        native_turso::query("UPDATE oc_api_keys SET revoked=1 WHERE id=?")
            .bind(owner)
            .execute(&pool)
            .await
            .unwrap();
        let mut t = tx(&pool, scope, owner).await;
        assert!(matches!(
            t.private_run_trace(input.run_id, 0, 10).await,
            Err(StorageError::Forbidden)
        ));
        assert!(matches!(
            t.append_private_run_event(input.run_id, &event).await,
            Err(StorageError::Forbidden)
        ));
    }
    #[test]
    fn client_cannot_claim_server_origin() {
        let input = json!({"event_id":Uuid::new_v4(),"reporter":"server","observed_at_ms":0,"event":{"kind":"result_reported","outcome":"success","summary":""}});
        assert!(serde_json::from_value::<RunEventInput>(input).is_err());
    }
}
