//! Context assembly keeps final provenance, permission and visibility checks in
//! one fresh relational snapshot after all embedding/native retrieval IO.
use crate::{
    context_retrieval::{ContextCandidate, ContextRoute, ContextSessionTurn},
    error::{AppError, Result},
    evidence::*,
    service::Service,
    storage::{DomainTx, Permission},
    types::AuthContext,
};

impl Service {
    pub async fn evidence_bundle(
        &self,
        auth: &AuthContext,
        input: EvidenceInput,
    ) -> Result<EvidenceBundle> {
        // A bounded public future prevents native retrieval state from being
        // duplicated on the stack of each embedding host's async call chain.
        Box::pin(self.evidence_bundle_inner(auth, input)).await
    }

    async fn evidence_bundle_inner(
        &self,
        auth: &AuthContext,
        mut input: EvidenceInput,
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
        let has_assets = input
            .routes
            .iter()
            .any(|r| r.route != EvidenceRoute::Session);
        let query = self
            .prepare_context_query(
                auth,
                &input.query,
                if has_assets { &input.mode } else { "keyword" },
                input.allow_partial,
            )
            .await?;
        let mut bundle = EvidenceBundle {
            tenant_id: auth.tenant_id, workspace_id: auth.workspace_id,
            policy: CONTEXT_POLICY.into(), validity: "passed_current_visibility_snapshot_not_factual_truth".into(),
            conflict_assessment: "explicit_single_value_identity_only_within_recalled_candidates; general_prose_not_assessed".into(),
            requested_mode: if has_assets { query.requested_mode.clone() } else { input.mode.clone() },
            effective_mode: if has_assets { query.effective_mode.clone() } else { "recent_lexical".into() },
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
                    let found = self
                        .context_candidates(
                            auth,
                            &query,
                            kind,
                            input.memory_identity.as_ref(),
                            route.limit,
                        )
                        .await?;
                    accounting.candidates = found.candidates.len();
                    assets.extend(found.candidates);
                    ranked_native.extend(found.ranked_native);
                }
                None => {
                    let found = self
                        .context_session_candidates(
                            auth,
                            &input.query,
                            input.session_id,
                            input.exclude_session_id,
                            route.limit,
                        )
                        .await?;
                    accounting.candidates = found.turns.len();
                    accounting.scanned = Some(found.scanned);
                    accounting.scan_exhausted = Some(found.scan_exhausted);
                    if found.scan_exhausted {
                        bundle.warnings.push("Session recall exhausted its 512-row recent scan window; older matching turns may exist.".into());
                    }
                    turns.extend(found.turns);
                }
            }
            bundle.routes.push(accounting);
        }
        let mut tx = self.read(auth, permission).await?;
        crate::retrieval::verify_native_candidates(&mut tx, &ranked_native).await?;
        let mut items = Vec::new();
        for candidate in assets {
            if let Some(mut fresh) = tx
                .context_candidate(
                    candidate.hit.chunk_id,
                    candidate.route,
                    input.memory_identity.as_ref(),
                )
                .await?
            {
                fresh.hit.score = candidate.hit.score;
                fresh.retrieval_routes = candidate.retrieval_routes;
                items.push(asset_item(fresh, &input, &query.effective_mode)?);
            } else {
                return Err(AppError::Conflict(
                    "native context candidate route changed".into(),
                ));
            }
        }
        let turn_keys: Vec<_> = turns
            .iter()
            .map(|candidate| (candidate.session_id, candidate.turn_id))
            .collect();
        let fresh_turns = tx.context_session_turns(&turn_keys).await?;
        for (candidate, fresh) in turns.into_iter().zip(fresh_turns) {
            if let Some(mut fresh) = fresh {
                fresh.score = candidate.score;
                items.push(session_item(fresh));
            } else {
                bundle
                    .routes
                    .iter_mut()
                    .find(|r| r.route == EvidenceRoute::Session)
                    .unwrap()
                    .invalidated_before_render += 1;
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
    } else if !conditions.is_empty()
        && conditions
            .iter()
            .all(|c| input.applicability_context.contains(c))
    {
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
fn session_item(turn: ContextSessionTurn) -> EvidenceItem {
    EvidenceItem {
        evidence_id: format!("turn:{}:{}", turn.session_id, turn.turn_id),
        route: EvidenceRoute::Session,
        retrieval_routes: vec!["recent_lexical".into()],
        title: "Archived session turn".into(),
        content: format!("Question: {}\nAnswer: {}", turn.question, turn.answer),
        provenance: EvidenceProvenance::SessionTurn {
            session_id: turn.session_id,
            turn_id: turn.turn_id,
            source_event_id: turn.source_event_id,
            used_evidence: turn.evidence,
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
        score_kind: "recent_visible_rank".into(),
        conflict_id: None,
        archive_roundtrip: "unsupported_session_turn_reference".into(),
        single_valued: false,
    }
}
