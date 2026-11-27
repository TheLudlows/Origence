//! Context assembly keeps final provenance, permission and visibility checks in
//! one fresh relational snapshot after all embedding/native retrieval IO.
use crate::{
    consolidation::{ConclusionState, MemoryView, effective_date_at},
    context_retrieval::{ContextCandidate, ContextLimit, ContextRoute, ContextSessionTurn},
    error::{AppError, Result},
    evidence::*,
    service::Service,
    storage::{DomainTx, Permission, StorageError, sqlite::now_ms},
    types::AuthContext,
};

impl Service {
    pub async fn evidence_bundle(
        &self,
        auth: &AuthContext,
        input: EvidenceInput,
    ) -> Result<EvidenceBundle> {
        self.observe_context(auth, input, true).await.0
    }

    pub(crate) async fn observed_evidence_bundle(
        &self,
        auth: &AuthContext,
        input: EvidenceInput,
    ) -> (
        Result<EvidenceBundle>,
        crate::retrieval_diagnostics::RetrievalDiagnostics,
    ) {
        // Server-prepared Run snapshots retain their existing citation contract.
        self.observe_context(auth, input, false).await
    }

    async fn observe_context(
        &self,
        auth: &AuthContext,
        input: EvidenceInput,
        include_long_term: bool,
    ) -> (
        Result<EvidenceBundle>,
        crate::retrieval_diagnostics::RetrievalDiagnostics,
    ) {
        let (mut result, observations) = crate::retrieval_diagnostics::capture(Box::pin(
            self.evidence_bundle_inner(auth, input, include_long_term),
        ))
        .await;
        if let Ok(bundle) = &mut result {
            bundle.retrieval = observations.clone();
        }
        (result, observations)
    }

    async fn evidence_bundle_inner(
        &self,
        auth: &AuthContext,
        mut input: EvidenceInput,
        include_long_term: bool,
    ) -> Result<EvidenceBundle> {
        input
            .validate()
            .map_err(|reason| AppError::Invalid(reason.into()))?;
        input.routes.sort_by_key(|r| r.route);
        let sessions = input
            .routes
            .iter()
            .any(|r| r.route == EvidenceRoute::Session);
        let permission = if sessions {
            Permission::Write
        } else {
            Permission::Read
        };
        // A forbidden raw-session request fails before any model/native IO.
        self.read(auth, permission).await?.rollback().await?;
        let query = self
            .prepare_context_query(auth, &input.query, &input.mode, input.allow_partial)
            .await?;
        let mut bundle = EvidenceBundle {
            retrieval: Default::default(),
            tenant_id: auth.tenant_id, workspace_id: auth.workspace_id,
            policy: CONTEXT_POLICY.into(), validity: "passed_current_visibility_snapshot_not_factual_truth".into(),
            conflict_assessment: "explicit_single_value_identity_only_within_recalled_candidates; general_prose_not_assessed".into(),
            requested_mode: query.requested_mode.clone(),
            effective_mode: query.effective_mode.clone(),
            rendered_context: String::new(), items: Vec::new(), conflicts: Vec::new(), warnings: query.warnings.clone(),
            budget: input.budget, used_bytes: 0, omitted_conflict_groups: 0, routes: Vec::new(),
        };
        if sessions {
            bundle.warnings.push("Session-turn citations cannot be submitted as SessionTurn used_evidence; that archive contract supports only chunk/entity/relation references.".into());
        }
        let mut assets = Vec::new();
        let mut ranked_native = Vec::new();
        let mut turns = Vec::new();
        for route in &input.routes {
            let mut accounting = RouteAccounting {
                route: route.route,
                candidate_limit: route.limit,
                max_bytes: route.max_bytes,
                candidates: 0,
                retained: 0,
                used_bytes: 0,
                omitted_for_budget: 0,
                invalidated_before_render: 0,
                scanned: None,
                scan_exhausted: None,
            };
            match asset_route(route.route) {
                Some(kind) => {
                    let found = crate::retrieval_diagnostics::in_route(
                        route.route,
                        self.context_candidates(
                            auth,
                            &query,
                            kind,
                            input.memory_identity.as_ref(),
                            ContextLimit::Chunks(route.limit),
                        ),
                    )
                    .await?;
                    accounting.candidates = found.candidates.len();
                    assets.extend(found.candidates);
                    ranked_native.extend(found.ranked_native);
                }
                None => {
                    let (scanned, found) = self
                        .context_session_candidates(auth, &input, &query, route.limit)
                        .await?;
                    accounting.candidates = found.len();
                    accounting.scanned = Some(scanned);
                    accounting.scan_exhausted = Some(false);
                    turns.extend(found);
                }
            }
            bundle.routes.push(accounting);
        }
        let facts = if include_long_term
            && input.memory_identity.is_none()
            && input
                .routes
                .iter()
                .any(|route| route.route == EvidenceRoute::Memory)
        {
            let found = self.search_long_term_facts(auth, &query.terms).await?;
            if found.exhausted {
                bundle.warnings.push("Long-term memory recall exhausted its 512-row fact scan; other matching topics may exist.".into());
            }
            Some(found)
        } else {
            None
        };
        let mut tx = self.read(auth, permission).await?;
        tx.verify_native_candidates(&ranked_native).await?;
        let mut items = Vec::new();
        let mut memory_accounting = bundle
            .routes
            .iter_mut()
            .find(|r| r.route == EvidenceRoute::Memory)
            .filter(|_| include_long_term);
        if let Some(accounting) = memory_accounting.as_deref_mut() {
            bundle.conflict_assessment = "explicit_single_value_identity_only_within_recalled_candidates; consolidated_conflicts_at_selected_business_time; general_prose_not_assessed".into();
            let as_of_ms = input.as_of_ms.unwrap_or_else(now_ms);
            if let Some(identity) = &input.memory_identity {
                if let Some(view) = tx.long_term_memory(identity, None, as_of_ms).await? {
                    items.push(long_term_item(view, as_of_ms, "exact_identity", 1.0));
                }
            } else if let Some(found) = facts {
                accounting.scanned = Some(found.scanned);
                accounting.scan_exhausted = Some(found.exhausted);
                for (id, matched) in found.ranked {
                    if let Some(view) = tx.refresh_long_term_fact(id, &matched, as_of_ms).await? {
                        items.push(long_term_item(
                            view,
                            as_of_ms,
                            "fact_lexical_overlap",
                            matched.score as f64,
                        ));
                        if items.len() == accounting.candidate_limit {
                            break;
                        }
                    } else {
                        accounting.invalidated_before_render += 1;
                    }
                }
            }
            accounting.candidates = items.len();
        }
        // A recalled consolidated topic is the date-aware result for its complete
        // identity. Enforce this before budgeting so omission cannot expose an
        // undated value for that identity.
        let dated_identities: Vec<_> = items
            .iter()
            .filter_map(|item| item.identity.clone())
            .collect();
        for candidate in assets {
            let accounting = if candidate.route == ContextRoute::Memory {
                memory_accounting.as_deref_mut()
            } else {
                None
            };
            if accounting
                .as_ref()
                .is_some_and(|r| r.candidates == r.candidate_limit)
            {
                continue;
            }
            let fresh = tx
                .refresh_context_candidate(candidate, input.memory_identity.as_ref())
                .await?;
            let item = asset_item(fresh, &input, &query.effective_mode)?;
            if let Some(accounting) = accounting {
                if item
                    .identity
                    .as_ref()
                    .is_some_and(|identity| dated_identities.contains(identity))
                {
                    continue;
                }
                accounting.candidates += 1;
            }
            items.push(item);
        }
        for candidate in turns {
            match candidate.deliver(&mut tx).await {
                Ok(turn) => items.push(session_item(turn, &query.effective_mode)),
                Err(StorageError::NotFound) => {
                    bundle
                        .routes
                        .iter_mut()
                        .find(|r| r.route == EvidenceRoute::Session)
                        .unwrap()
                        .invalidated_before_render += 1;
                }
                Err(error) => return Err(error.into()),
            }
        }
        render(&mut bundle, items);
        // Rechecks live role; source checks and rendering share this snapshot.
        tx.commit().await?;
        Ok(bundle)
    }
}

fn asset_route(route: EvidenceRoute) -> Option<ContextRoute> {
    match route {
        EvidenceRoute::Memory => Some(ContextRoute::Memory),
        EvidenceRoute::Knowledge => Some(ContextRoute::Knowledge),
        EvidenceRoute::Learning => Some(ContextRoute::Learning),
        EvidenceRoute::Session => None,
    }
}
fn evidence_route(route: ContextRoute) -> EvidenceRoute {
    match route {
        ContextRoute::Memory => EvidenceRoute::Memory,
        ContextRoute::Knowledge => EvidenceRoute::Knowledge,
        ContextRoute::Learning => EvidenceRoute::Learning,
    }
}
fn asset_item(
    candidate: ContextCandidate,
    input: &EvidenceInput,
    mode: &str,
) -> Result<EvidenceItem> {
    let hit = candidate.hit;
    let learning: Option<LearningEvidence> = candidate
        .learning
        .map(|learning| serde_json::from_value(serde_json::json!(learning)))
        .transpose()
        .map_err(|_| AppError::Internal(anyhow::anyhow!("invalid context learning metadata")))?;
    let conditions = learning
        .as_ref()
        .map(|l| l.conditions.clone())
        .unwrap_or_default();
    let applicability = if learning.is_none() {
        "not_applicable"
    } else if conditions_match(&conditions, &input.applicability_context) {
        "caller_context_exact_match"
    } else {
        "unverified"
    };
    let normalization_status = hit.normalization_status().into();
    let semantic_status = hit
        .locator
        .get("semantic_status")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    Ok(EvidenceItem {
        evidence_id: format!("chunk:{}", hit.chunk_id),
        route: evidence_route(candidate.route),
        retrieval_routes: candidate.retrieval_routes,
        title: hit.title,
        content: hit.content,
        provenance: EvidenceProvenance::AssetChunk {
            chunk_id: hit.chunk_id,
            asset_id: hit.asset_id,
            asset_version: hit.version,
            source_event_id: hit.source_event_id,
            source_version: candidate.source_version,
            version_root_source_event_id: candidate.version_root_source_event_id,
            locator: hit.locator,
        },
        identity: candidate.semantic_identity.or(hit.identity),
        normalization_status,
        semantic_status,
        semantic_value: candidate.semantic_value,
        conditions,
        learning,
        applicability: applicability.into(),
        score: hit.score,
        score_kind: match mode {
            "hybrid" => "rrf60",
            "vector" => "native_similarity",
            _ => "lexical_overlap",
        }
        .into(),
        conflict_id: None,
        archive_roundtrip: "chunk_supported".into(),
        single_valued: candidate.single_valued,
    })
}
fn session_item(turn: ContextSessionTurn, mode: &str) -> EvidenceItem {
    EvidenceItem {
        evidence_id: format!("turn:{}:{}", turn.session_id, turn.turn_id),
        route: EvidenceRoute::Session,
        score_kind: match mode {
            "vector" => "native_similarity",
            "hybrid" => "rrf60",
            _ => "lexical_relevance",
        }
        .into(),
        retrieval_routes: turn.retrieval_routes,
        title: "Archived session turn".into(),
        content: format!("Question: {}\nAnswer: {}", turn.question, turn.answer),
        provenance: EvidenceProvenance::SessionTurn {
            session_id: turn.session_id,
            turn_id: turn.turn_id,
            source_event_id: turn.source_event_id,
            used_evidence: turn.evidence,
            evidence_use: turn.evidence_use,
            created_at: turn.created_at,
        },
        identity: None,
        normalization_status: "not_applicable".into(),
        semantic_status: None,
        semantic_value: None,
        conditions: Vec::new(),
        learning: None,
        applicability: "not_applicable".into(),
        score: turn.score,
        conflict_id: None,
        archive_roundtrip: "unsupported_session_turn_reference".into(),
        single_valued: false,
    }
}

fn long_term_item(view: MemoryView, as_of_ms: i64, origin: &str, score: f64) -> EvidenceItem {
    let (status, content, value) = match &view.state {
        ConclusionState::Current { value } => (
            "current",
            format!("Applicable value: {value}"),
            Some(serde_json::json!(value)),
        ),
        ConclusionState::Conflicting => (
            "conflicting",
            "Conflicting values for this business date. Do not choose a single value.".into(),
            None,
        ),
        ConclusionState::Unknown => (
            "unknown",
            "No applicable evidence for this business date.".into(),
            None,
        ),
    };
    EvidenceItem {
        evidence_id: format!("long_term_memory:{}:v{}", view.memory_id, view.version),
        route: EvidenceRoute::Memory,
        retrieval_routes: vec![origin.into()],
        title: format!(
            "{} · {}",
            view.identity.subject.stable_id, view.identity.predicate
        ),
        content,
        provenance: EvidenceProvenance::LongTermMemory {
            memory_id: view.memory_id,
            version: view.version,
            as_of_ms,
            effective_from_ms: effective_date_at(
                view.evidence
                    .iter()
                    .map(|evidence| evidence.effective_at_ms),
                as_of_ms,
            )
            .flatten(),
            state: view.state,
            timeline: view.timeline,
            citations: view.evidence,
        },
        conditions: view
            .identity
            .context
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect(),
        identity: Some(view.identity),
        normalization_status: "consolidated".into(),
        semantic_status: Some(status.into()),
        semantic_value: value,
        learning: None,
        applicability: if origin == "exact_identity" {
            "caller_identity_exact_match"
        } else {
            "unverified"
        }
        .into(),
        score,
        score_kind: origin.into(),
        conflict_id: None,
        archive_roundtrip: "unsupported_long_term_memory_reference".into(),
        single_valued: false,
    }
}
