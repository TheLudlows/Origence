//! Relational authority for independent evidence routes.
//!
//! Type and exact identity filtering precede the branch candidate budget and
//! native top-k. Every retained result still uses the existing complete current
//! version / S2 support / transitive Learning visibility guard.

use super::visibility::CURRENT_CHUNK_ROWS;
use super::*;
use crate::{
    context_retrieval::{
        CONTEXT_CANDIDATE_LIMIT, ContextCandidate, ContextLearning, ContextLearningSupport,
        ContextRoute, ContextSessionResult, ContextSessionTurn, SESSION_SCAN_LIMIT,
    },
    learning::LearningCandidate,
    memory_identity::MemoryIdentity,
    memory_semantics::Cardinality,
    types::SearchHit,
};
use std::collections::HashSet;

impl SqliteTx {
    pub(crate) async fn context_route_matches(
        &mut self,
        asset: Uuid,
        route: ContextRoute,
    ) -> StorageResult<bool> {
        let kind: Option<String> = sqlx::query_scalar(
            "SELECT kind FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND id=? AND NOT deleted AND current_version IS NOT NULL",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(asset)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        let expected = match route {
            ContextRoute::Memory => "memory",
            ContextRoute::Knowledge | ContextRoute::Learning => "knowledge",
        };
        if kind.as_deref() != Some(expected) {
            return Ok(false);
        }
        let is_learning = self.is_learning_asset(asset).await?;
        if route != ContextRoute::Learning {
            // Includes orphaned Learning sources, not just catalog membership.
            return Ok(!is_learning);
        }
        if !is_learning || !super::improve::available(&mut self.tx).await? {
            return Ok(false);
        }
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM oc_learnings l JOIN oc_assets a ON a.tenant_id=l.tenant_id AND a.workspace_id=l.workspace_id AND a.id=l.asset_id JOIN oc_versions v ON v.tenant_id=a.tenant_id AND v.workspace_id=a.workspace_id AND v.asset_id=a.id AND v.version=a.current_version AND v.source_event_id=l.source_event_id WHERE l.tenant_id=? AND l.workspace_id=? AND l.asset_id=? AND l.status='published')")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(asset)
            .fetch_one(&mut *self.tx).await.map_err(sqlite_err)
    }

    /// No candidate LIMIT here. Independent type filtering happens before the
    /// lexical candidate cap or the native vector backend sees any identifiers.
    async fn context_route_rows(
        &mut self,
        route: ContextRoute,
        identity: Option<&MemoryIdentity>,
    ) -> StorageResult<Vec<(Uuid, String)>> {
        let kind = match route {
            ContextRoute::Memory => "memory",
            ContextRoute::Knowledge | ContextRoute::Learning => "knowledge",
        };
        let rows: Vec<(Uuid, Uuid, String, String)> = sqlx::query_as(
            &format!("SELECT c.id,c.asset_id,c.search_terms,c.locator {CURRENT_CHUNK_ROWS} AND a.kind=?"),
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(kind)
        .fetch_all(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        let mut membership = HashMap::new();
        let mut result = Vec::new();
        for (id, asset, terms, locator) in rows {
            let matches = if let Some(matches) = membership.get(&asset) {
                *matches
            } else {
                let matches = self.context_route_matches(asset, route).await?;
                membership.insert(asset, matches);
                matches
            };
            if !matches {
                continue;
            }
            if route == ContextRoute::Memory
                && let Some(identity) = identity
                && self.identity_for_asset(asset).await?.as_ref() != Some(identity)
            {
                let locator: Value = serde_json::from_str(&locator)
                    .map_err(|_| StorageError::Unavailable("invalid chunk locator".into()))?;
                if conflict_identity(&locator).as_ref() != Some(identity) {
                    continue;
                }
            }
            result.push((id, terms));
        }
        Ok(result)
    }

    pub(crate) async fn context_keyword_hits(
        &mut self,
        terms: &str,
        route: ContextRoute,
        identity: Option<&MemoryIdentity>,
    ) -> StorageResult<Vec<SearchHit>> {
        let words: HashSet<_> = terms.split_whitespace().collect();
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let rows = self.context_route_rows(route, identity).await?;
        let mut hits = Vec::new();
        for (id, text) in rows {
            // The complete guard runs before ranking and the candidate cap.
            let Some(mut hit) = self.visible_hit(id).await? else {
                continue;
            };
            let tokens: HashSet<_> = text.split_whitespace().collect();
            if words.is_subset(&tokens) {
                hit.score = words.len() as f64 / tokens.len().max(1) as f64;
                hits.push(hit);
            }
        }
        hits.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then(a.chunk_id.cmp(&b.chunk_id))
        });
        hits.truncate(CONTEXT_CANDIDATE_LIMIT);
        Ok(hits)
    }

    pub(crate) async fn context_vector_candidates(
        &mut self,
        profile: &str,
        dimension: usize,
        generation: i64,
        route: ContextRoute,
        identity: Option<&MemoryIdentity>,
    ) -> StorageResult<Vec<Uuid>> {
        let route_ids: HashSet<_> = self
            .context_route_rows(route, identity)
            .await?
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        // This existing shared helper has no top-k or LIMIT: it checks full
        // visibility plus ready index generation and committed publication.
        let mut ids = self
            .vector_candidates(profile, dimension, generation, None)
            .await?;
        ids.retain(|id| route_ids.contains(id));
        ids.sort_unstable();
        Ok(ids)
    }

    /// Exact bundle identity scope includes a current conflict's explicitly
    /// proposed identity, so filtering cannot hide the counter-evidence. Other
    /// unnormalized statuses never acquire identity-filter membership.
    pub(crate) async fn context_identity_matches(
        &mut self,
        hit: &SearchHit,
        identity: Option<&MemoryIdentity>,
    ) -> StorageResult<bool> {
        let Some(identity) = identity else {
            return Ok(true);
        };
        if hit.kind != "memory" {
            return Ok(false);
        }
        Ok(
            self.identity_for_asset(hit.asset_id).await?.as_ref() == Some(identity)
                || conflict_identity(&hit.locator).as_ref() == Some(identity),
        )
    }

    /// Rebuild an exact current chunk under the caller's final snapshot. The
    /// renderer may keep this transaction open until its response is assembled.
    pub(crate) async fn context_candidate(
        &mut self,
        chunk: Uuid,
        route: ContextRoute,
        identity: Option<&MemoryIdentity>,
    ) -> StorageResult<Option<ContextCandidate>> {
        self.check_permission(Permission::Read).await?;
        let Some(mut hit) = self.visible_hit(chunk).await? else {
            return Ok(None);
        };
        if !self.context_route_matches(hit.asset_id, route).await? {
            return Ok(None);
        }
        hit.identity = self.identity_for_asset(hit.asset_id).await?;
        if route == ContextRoute::Memory && !self.context_identity_matches(&hit, identity).await? {
            return Ok(None);
        }
        let version_root_source_event_id: Uuid = sqlx::query_scalar(
            "SELECT source_event_id FROM oc_versions WHERE tenant_id=? AND workspace_id=? AND asset_id=? AND version=?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(hit.asset_id)
        .bind(hit.version)
        .fetch_one(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        let semantic = hit.locator["parser"] == crate::memory_semantics::PARSER;
        let source_version = if semantic {
            hit.locator["source_version"]
                .as_i64()
                .and_then(|version| i32::try_from(version).ok())
                .filter(|version| *version > 0)
                .ok_or_else(|| StorageError::Unavailable("invalid semantic provenance".into()))?
        } else {
            hit.version
        };
        let semantic_identity = if route == ContextRoute::Memory && semantic {
            match hit.locator["semantic_status"].as_str() {
                Some("confirmed") => hit.identity.clone(),
                Some("conflict") => conflict_identity(&hit.locator),
                _ => None,
            }
        } else {
            None
        };
        let single_valued = if let Some(identity) = &semantic_identity {
            self.memory_property(&identity.predicate)
                .await?
                .is_some_and(|property| {
                    property.cardinality == Cardinality::Single
                        && property.check_identity(identity).is_ok()
                })
        } else {
            false
        };
        let semantic_value = if semantic {
            hit.locator.get("semantic_value").cloned()
        } else {
            None
        };
        let learning = if route == ContextRoute::Learning {
            Some(self.context_learning(hit.asset_id).await?)
        } else {
            None
        };
        Ok(Some(ContextCandidate {
            route,
            hit,
            version_root_source_event_id,
            source_version,
            learning,
            retrieval_routes: Vec::new(),
            single_valued,
            semantic_identity,
            semantic_value,
        }))
    }

    async fn context_learning(&mut self, asset: Uuid) -> StorageResult<ContextLearning> {
        // Candidate's visible_hit has already checked the complete transitive
        // support closure. Do not call the writer-only raw session API here.
        let (learning_id, improve_id, session_id, body): (Uuid, Uuid, Uuid, String) =
            sqlx::query_as("SELECT id,improve_id,session_id,candidate FROM oc_learnings WHERE tenant_id=? AND workspace_id=? AND asset_id=? AND status='published'")
                .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(asset)
                .fetch_one(&mut *self.tx).await.map_err(sqlite_err)?;
        let candidate: LearningCandidate = serde_json::from_str(&body)
            .map_err(|_| StorageError::Unavailable("invalid learning record".into()))?;
        Ok(ContextLearning {
            learning_id,
            improve_id,
            session_id,
            statement: candidate.statement,
            why: candidate.why,
            conditions: candidate.conditions,
            supports: candidate
                .supports
                .into_iter()
                .map(|support| ContextLearningSupport {
                    turn_id: support.turn_id,
                    field: support.field,
                    byte_start: support.byte_start,
                    byte_end: support.byte_end,
                    feedback_id: support.feedback_id,
                    principal_id: support.principal_id,
                })
                .collect(),
        })
    }

    /// The reference is a real (session_id, turn_id, source_event_id) tuple,
    /// never a fabricated chunk or asset version.
    pub(crate) async fn context_session_turn(
        &mut self,
        session: Uuid,
        turn: Uuid,
    ) -> StorageResult<Option<ContextSessionTurn>> {
        self.check_permission(Permission::Write).await?;
        let (source_event_id, evidence) = match self.require_turn_visible_local(session, turn).await
        {
            Ok(value) => value,
            Err(StorageError::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        match self.require_archived_evidence_set_visible(&evidence).await {
            Ok(()) => {}
            Err(StorageError::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        }
        let row: Option<(String, i64)> = sqlx::query_as("SELECT e.content,t.created_at FROM oc_session_turns t JOIN oc_events e ON e.tenant_id=t.tenant_id AND e.workspace_id=t.workspace_id AND e.id=t.source_event_id WHERE t.tenant_id=? AND t.workspace_id=? AND t.session_id=? AND t.turn_id=? AND e.id=? AND e.kind='session_turn' AND e.state='active'")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(session).bind(turn).bind(source_event_id)
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        let Some((body, created_at)) = row else {
            return Ok(None);
        };
        #[derive(serde::Deserialize)]
        struct Body {
            question: String,
            answer: String,
        }
        let body: Body = serde_json::from_str(&body)
            .map_err(|_| StorageError::Unavailable("invalid session record".into()))?;
        Ok(Some(ContextSessionTurn {
            session_id: session,
            turn_id: turn,
            source_event_id,
            question: body.question,
            answer: body.answer,
            evidence,
            created_at,
            score: 0.0,
        }))
    }

    pub(crate) async fn context_recent_turns(
        &mut self,
        terms: &str,
        session: Option<Uuid>,
        exclude_session: Option<Uuid>,
        limit: usize,
    ) -> StorageResult<ContextSessionResult> {
        self.check_permission(Permission::Write).await?;
        let rows: Vec<(Uuid, Uuid)> = sqlx::query_as("SELECT t.session_id,t.turn_id FROM oc_session_turns t JOIN oc_events e ON e.tenant_id=t.tenant_id AND e.workspace_id=t.workspace_id AND e.id=t.source_event_id WHERE t.tenant_id=? AND t.workspace_id=? AND e.state='active' AND e.kind='session_turn' AND (? IS NULL OR t.session_id=?) AND (? IS NULL OR t.session_id!=?) ORDER BY t.created_at DESC,t.session_id,t.turn_id LIMIT ?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id)
            .bind(session).bind(session).bind(exclude_session).bind(exclude_session)
            .bind((SESSION_SCAN_LIMIT + 1) as i64)
            .fetch_all(&mut *self.tx).await.map_err(sqlite_err)?;
        let scan_exhausted = rows.len() > SESSION_SCAN_LIMIT;
        let words: HashSet<_> = terms.split_whitespace().collect();
        let mut turns = Vec::new();
        let mut scanned = 0;
        for (session, turn) in rows.into_iter().take(SESSION_SCAN_LIMIT) {
            scanned += 1;
            let Some(mut item) = self.context_session_turn(session, turn).await? else {
                continue;
            };
            let text = crate::parsing::lexical(&format!("{}\n{}", item.question, item.answer));
            let tokens: HashSet<_> = text.split_whitespace().collect();
            if words.is_subset(&tokens) && turns.len() < limit {
                // Recent-first rank counts visible retained matches only.
                item.score = 1.0 / (turns.len() + 1) as f64;
                turns.push(item);
            }
        }
        Ok(ContextSessionResult {
            turns,
            scanned,
            scan_exhausted,
        })
    }
}

fn conflict_identity(locator: &Value) -> Option<MemoryIdentity> {
    if locator["parser"] != crate::memory_semantics::PARSER
        || locator["semantic_status"] != "conflict"
    {
        return None;
    }
    let identity: MemoryIdentity =
        serde_json::from_value(locator["suggested_identity"].clone()).ok()?;
    identity.validate().ok()?;
    Some(identity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_scope_includes_only_exact_explicit_conflict_suggestions() {
        let identity: MemoryIdentity = serde_json::from_value(json!({
            "subject":{"kind":"service","stable_id":"Atlas"},
            "predicate":"release.approval","context":{"environment":"production"}
        }))
        .unwrap();
        let mut locator = json!({
            "parser":crate::memory_semantics::PARSER,
            "semantic_status":"conflict", "suggested_identity":identity
        });
        assert_eq!(conflict_identity(&locator), Some(identity.clone()));
        for status in ["confirmed", "proposed", "ambiguous", "negated"] {
            locator["semantic_status"] = json!(status);
            assert_eq!(conflict_identity(&locator), None);
        }
        locator["semantic_status"] = json!("conflict");
        locator["suggested_identity"]["subject"]["stable_id"] = json!("");
        assert_eq!(conflict_identity(&locator), None);
    }
}
