//! Opt-in, bounded, read-only context. Visibility is not factual verification.
use std::collections::BTreeSet;
#[cfg(any(feature = "local-storage", test))]
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::memory_identity::MemoryIdentity;

pub const CONTEXT_POLICY: &str = "evidence-bundle:utf8-whole-blocks:round-robin:v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceRoute {
    Memory,
    Knowledge,
    Learning,
    Session,
}

fn default_limit() -> usize { 20 }
fn default_bytes() -> usize { 4000 }
fn default_mode() -> String { "keyword".into() }

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RouteBudget {
    pub route: EvidenceRoute,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default = "default_bytes")]
    pub max_bytes: usize,
}

fn default_routes() -> Vec<RouteBudget> {
    [EvidenceRoute::Memory, EvidenceRoute::Knowledge, EvidenceRoute::Learning]
        .into_iter().map(|route| RouteBudget { route, limit: default_limit(), max_bytes: default_bytes() }).collect()
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EvidenceInput {
    pub query: String,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub allow_partial: bool,
    /// Actual UTF-8 bytes, including citations, conditions and conflict warnings.
    #[serde(default = "default_bytes")]
    pub budget: usize,
    #[serde(default = "default_routes")]
    pub routes: Vec<RouteBudget>,
    #[serde(default)]
    pub memory_identity: Option<MemoryIdentity>,
    /// Caller assertions only. Exact matching never proves applicability.
    #[serde(default)]
    pub applicability_context: Vec<String>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub session_id: Option<Uuid>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub exclude_session_id: Option<Uuid>,
}

impl EvidenceInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.query.trim().is_empty() || self.query.len() > 4000 || self.query.contains('\0') {
            return Err("query invalid");
        }
        if !["keyword", "vector", "hybrid"].contains(&self.mode.as_str()) || self.budget > 32000 {
            return Err("mode or budget invalid");
        }
        if self.routes.is_empty() || self.routes.len() > 4 {
            return Err("routes must contain 1 to 4 unique routes");
        }
        let mut seen = BTreeSet::new();
        for route in &self.routes {
            if !seen.insert(route.route) || !(1..=100).contains(&route.limit) || route.max_bytes > 32000 {
                return Err("route budget invalid");
            }
        }
        if self.memory_identity.as_ref().is_some_and(|i| i.validate().is_err()) {
            return Err("memory identity invalid");
        }
        if self.applicability_context.len() > 32 || self.applicability_context.iter().any(|s| s.trim().is_empty() || s.len() > 256 || s.contains('\0')) {
            return Err("applicability context invalid");
        }
        if self.session_id.is_some_and(|id| id.is_nil()) || self.exclude_session_id.is_some_and(|id| id.is_nil()) {
            return Err("session identifier invalid");
        }
        if (self.session_id.is_some() || self.exclude_session_id.is_some()) && !seen.contains(&EvidenceRoute::Session) {
            return Err("session filters require the session route");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EvidenceProvenance {
    AssetChunk {
        chunk_id: Uuid,
        asset_id: Uuid,
        asset_version: i32,
        source_event_id: Uuid,
        source_version: i32,
        version_root_source_event_id: Uuid,
        locator: Value,
    },
    SessionTurn {
        session_id: Uuid,
        turn_id: Uuid,
        source_event_id: Uuid,
        used_evidence: Vec<crate::session::ArchivedEvidence>,
        created_at: i64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LearningCitation {
    pub turn_id: Uuid,
    pub field: crate::learning::SupportField,
    pub byte_start: usize,
    pub byte_end: usize,
    pub feedback_id: Uuid,
    pub principal_id: Uuid,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LearningEvidence {
    pub learning_id: Uuid,
    pub improve_id: Uuid,
    pub session_id: Uuid,
    pub statement: String,
    pub why: String,
    pub conditions: Vec<String>,
    pub supports: Vec<LearningCitation>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvidenceItem {
    pub evidence_id: String,
    pub route: EvidenceRoute,
    pub retrieval_routes: Vec<String>,
    pub title: String,
    pub content: String,
    pub provenance: EvidenceProvenance,
    pub identity: Option<MemoryIdentity>,
    pub normalization_status: String,
    pub semantic_status: Option<String>,
    pub semantic_value: Option<Value>,
    pub conditions: Vec<String>,
    pub learning: Option<LearningEvidence>,
    pub applicability: String,
    pub score: f64,
    pub score_kind: String,
    pub conflict_id: Option<String>,
    pub archive_roundtrip: String,
    /// Internal deterministic rule eligibility, never inferred from prose.
    #[serde(skip)]
    #[cfg_attr(not(any(feature = "local-storage", test)), allow(dead_code))]
    pub(crate) single_valued: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvidenceConflict {
    pub conflict_id: String,
    pub identity: MemoryIdentity,
    pub evidence_ids: Vec<String>,
    pub warning: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RouteAccounting {
    pub route: EvidenceRoute,
    pub candidate_limit: usize,
    pub max_bytes: usize,
    pub candidates: usize,
    pub retained: usize,
    pub used_bytes: usize,
    pub omitted_for_budget: usize,
    pub invalidated_before_render: usize,
    pub scanned: Option<usize>,
    pub scan_exhausted: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvidenceBundle {
    pub tenant_id: Uuid,
    pub workspace_id: Uuid,
    pub policy: String,
    pub validity: String,
    pub conflict_assessment: String,
    pub requested_mode: String,
    pub effective_mode: String,
    pub rendered_context: String,
    pub items: Vec<EvidenceItem>,
    pub conflicts: Vec<EvidenceConflict>,
    pub warnings: Vec<String>,
    pub budget: usize,
    pub used_bytes: usize,
    pub omitted_conflict_groups: usize,
    pub routes: Vec<RouteAccounting>,
}

#[cfg(any(feature = "local-storage", test))]
struct Unit {
    route: EvidenceRoute,
    items: Vec<EvidenceItem>,
    conflict: Option<EvidenceConflict>,
}

#[cfg(any(feature = "local-storage", test))]
fn units(mut items: Vec<EvidenceItem>) -> Vec<Unit> {
    // Only duplicate exact typed locations coalesce; equal prose never does.
    let mut positions = BTreeMap::new();
    let mut unique: Vec<EvidenceItem> = Vec::new();
    for item in items.drain(..) {
        let key = serde_json::to_string(&item.provenance).expect("provenance is JSON");
        if let Some(&index) = positions.get(&key) {
            let previous: &mut EvidenceItem = &mut unique[index];
            previous.retrieval_routes.extend(item.retrieval_routes);
            previous.retrieval_routes.sort();
            previous.retrieval_routes.dedup();
        } else {
            positions.insert(key, unique.len());
            unique.push(item);
        }
    }
    let mut identities: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, item) in unique.iter().enumerate() {
        if item.single_valued && item.semantic_value.is_some() && matches!(item.semantic_status.as_deref(), Some("confirmed" | "conflict")) && let Some(identity) = &item.identity {
            identities.entry(serde_json::to_string(identity).expect("identity is JSON")).or_default().push(index);
        }
    }
    let mut grouped = BTreeSet::new();
    let mut result = Vec::new();
    for indexes in identities.values() {
        let values: BTreeSet<String> = indexes.iter().map(|&i| unique[i].semantic_value.as_ref().unwrap().to_string()).collect();
        let explicit_conflict = indexes.iter().any(|&i| unique[i].semantic_status.as_deref() == Some("conflict"));
        if values.len() < 2 && !explicit_conflict { continue; }
        let id = format!("conflict:{}", result.len() + 1);
        let mut members = Vec::new();
        for &index in indexes {
            grouped.insert(index);
            let mut item = unique[index].clone();
            item.conflict_id = Some(id.clone());
            members.push(item);
        }
        let conflict = EvidenceConflict {
            conflict_id: id,
            identity: members[0].identity.clone().unwrap(),
            evidence_ids: members.iter().map(|i| i.evidence_id.clone()).collect(),
            warning: if values.len() < 2 {
                "Unresolved explicit conflict evidence. Its counterpart may be outside the recalled candidates; do not treat this as a confirmed value."
            } else {
                "Unresolved explicit single-value conflict. Preserve all alternatives; do not choose a winner from retrieval rank."
            }.into(),
        };
        result.push(Unit { route: members[0].route, items: members, conflict: Some(conflict) });
    }
    for (index, item) in unique.into_iter().enumerate() {
        if !grouped.contains(&index) { result.push(Unit { route: item.route, items: vec![item], conflict: None }); }
    }
    result
}

#[cfg(any(feature = "local-storage", test))]
fn block(unit: &Unit) -> String {
    let mut text = String::new();
    if let Some(conflict) = &unit.conflict {
        text.push_str(&format!("CONFLICT {}: {}\n", conflict.conflict_id, conflict.warning));
    }
    for item in &unit.items {
        // JSON escaping keeps embedded newlines/quotes in evidence metadata data.
        let metadata = serde_json::json!({
            "route":item.route,"identity":item.identity,"normalization_status":item.normalization_status,
            "semantic_status":item.semantic_status,"semantic_value":item.semantic_value,
            "conditions":item.conditions,"applicability":item.applicability,"learning":item.learning,
            "provenance":item.provenance,"conflict_id":item.conflict_id,
            "archive_roundtrip":item.archive_roundtrip
        });
        text.push_str(&format!("[{}] {}\n{}\n{}\n\n", item.evidence_id, serde_json::to_string(&item.title).unwrap(), metadata, item.content));
    }
    text
}

#[cfg(any(feature = "local-storage", test))]
pub(crate) fn render(bundle: &mut EvidenceBundle, candidates: Vec<EvidenceItem>) {
    let mut queues: BTreeMap<EvidenceRoute, std::collections::VecDeque<Unit>> = BTreeMap::new();
    for unit in units(candidates) { queues.entry(unit.route).or_default().push_back(unit); }
    // Equal round-robin opportunity; ceilings are caps, not quality weights.
    loop {
        let mut progressed = false;
        for accounting in &mut bundle.routes {
            let Some(unit) = queues.get_mut(&accounting.route).and_then(|q| q.pop_front()) else { continue; };
            progressed = true;
            let text = block(&unit);
            if bundle.rendered_context.len() + text.len() > bundle.budget || accounting.used_bytes + text.len() > accounting.max_bytes {
                accounting.omitted_for_budget += unit.items.len();
                bundle.omitted_conflict_groups += usize::from(unit.conflict.is_some());
                continue;
            }
            accounting.used_bytes += text.len();
            accounting.retained += unit.items.len();
            bundle.rendered_context.push_str(&text);
            bundle.items.extend(unit.items);
            if let Some(conflict) = unit.conflict { bundle.conflicts.push(conflict); }
        }
        if !progressed { break; }
    }
    bundle.used_bytes = bundle.rendered_context.len();
    if bundle.omitted_conflict_groups > 0 { bundle.warnings.push("Conflict groups omitted whole due to byte ceilings; no side was rendered alone.".into()); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_identity::{MemorySubject, SubjectKind};

    fn item(id: u128, value: &str) -> EvidenceItem {
        EvidenceItem {
            evidence_id: format!("chunk:{}", Uuid::from_u128(id)),
            route: EvidenceRoute::Memory,
            retrieval_routes: vec!["keyword".into()],
            title: "中文 🦀".into(),
            content: format!("中文 🦀 {value}"),
            provenance: EvidenceProvenance::AssetChunk {
                chunk_id: Uuid::from_u128(id), asset_id: Uuid::from_u128(id), asset_version: 2,
                source_event_id: Uuid::from_u128(id + 10), source_version: 1,
                version_root_source_event_id: Uuid::from_u128(id + 20), locator: serde_json::json!({"byte_start":0,"byte_end":10}),
            },
            identity: Some(MemoryIdentity { subject: MemorySubject { kind: SubjectKind::Project, stable_id: "Atlas".into() }, predicate: "runtime".into(), context: BTreeMap::new() }),
            normalization_status: "explicit_identity".into(), semantic_status: Some("confirmed".into()),
            semantic_value: Some(serde_json::json!(value)), learning: None, conditions: vec!["生产 🦀".into()],
            applicability: "unverified".into(), score: 1.0, score_kind: "rrf60".into(), conflict_id: None,
            archive_roundtrip: "chunk_supported".into(), single_valued: true,
        }
    }
    fn bundle(budget: usize) -> EvidenceBundle {
        EvidenceBundle {
            tenant_id: Uuid::nil(), workspace_id: Uuid::nil(), policy: CONTEXT_POLICY.into(),
            validity: "current_visibility_snapshot".into(), conflict_assessment: "explicit_single_value_only".into(),
            requested_mode: "keyword".into(), effective_mode: "keyword".into(), rendered_context: String::new(),
            items: vec![], conflicts: vec![], warnings: vec![], budget, used_bytes: 0, omitted_conflict_groups: 0,
            routes: vec![RouteAccounting { route: EvidenceRoute::Memory, candidate_limit: 20, max_bytes: 32000, candidates: 2, retained: 0, used_bytes: 0, omitted_for_budget: 0, invalidated_before_render: 0, scanned: None, scan_exhausted: None }],
        }
    }
    #[test]
    fn whole_utf8_blocks_and_citations_share_exact_budget() {
        let candidate = item(1, "Rust");
        let mut full = bundle(32000);
        render(&mut full, vec![candidate.clone()]);
        let length = full.used_bytes;
        assert_eq!(length, full.rendered_context.len());
        assert!(full.rendered_context.contains("生产 🦀"));
        assert!(full.rendered_context.contains("\"source_version\":1"));
        assert!(full.rendered_context.contains("\"asset_version\":2"));
        for budget in [0, length - 1] {
            let mut b = bundle(budget);
            render(&mut b, vec![candidate.clone()]);
            assert!(b.items.is_empty() && b.rendered_context.is_empty());
        }
        let mut exact = bundle(length);
        render(&mut exact, vec![candidate]);
        assert_eq!(exact.rendered_context, full.rendered_context);
        assert_eq!(exact.items.len(), 1);
    }
    #[test]
    fn conflicting_alternatives_and_warning_are_indivisible() {
        let candidates = vec![item(1, "Rust"), item(2, "Python")];
        let mut full = bundle(32000);
        render(&mut full, candidates.clone());
        assert_eq!(full.conflicts.len(), 1);
        assert_eq!(full.items.len(), 2);
        assert!(full.rendered_context.contains("CONFLICT conflict:1"));
        let mut short = bundle(full.used_bytes - 1);
        render(&mut short, candidates);
        assert!(short.items.is_empty() && short.rendered_context.is_empty());
        assert_eq!(short.omitted_conflict_groups, 1);
        assert_eq!(short.routes[0].omitted_for_budget, 2);
    }
    #[test]
    fn singleton_explicit_conflict_keeps_its_warning_inside_budget() {
        let mut conflict = item(1, "Python");
        conflict.semantic_status = Some("conflict".into());
        let mut full = bundle(32000);
        render(&mut full, vec![conflict.clone()]);
        assert_eq!(full.conflicts.len(), 1);
        assert!(full.rendered_context.contains("counterpart may be outside"));
        let mut short = bundle(full.used_bytes - 1);
        render(&mut short, vec![conflict]);
        assert!(short.items.is_empty() && short.rendered_context.is_empty());
        assert_eq!(short.omitted_conflict_groups, 1);
    }
    #[test]
    fn multi_values_different_conditions_and_ambiguous_prose_are_not_conflicts() {
        for variant in 0..3 {
            let first = item(1, "Rust");
            let mut other = item(2, "Python");
            match variant {
                0 => other.single_valued = false,
                1 => { other.identity.as_mut().unwrap().context.insert("env".into(), "dev".into()); },
                _ => other.semantic_status = Some("ambiguous".into()),
            }
            let mut b = bundle(32000);
            render(&mut b, vec![first, other]);
            assert!(b.conflicts.is_empty());
            assert_eq!(b.items.len(), 2);
        }
    }
    #[test]
    fn exact_location_coalesces_routes_not_equal_text_at_other_locations() {
        let first = item(1, "Rust");
        let mut duplicate = first.clone();
        duplicate.retrieval_routes = vec!["vector".into()];
        let other = item(2, "Rust");
        let mut b = bundle(32000);
        render(&mut b, vec![first, duplicate, other]);
        assert_eq!(b.items.len(), 2);
        assert_eq!(b.items[0].retrieval_routes, ["keyword", "vector"]);
    }
    #[test]
    fn input_rejects_unknown_fields_duplicate_routes_and_excessive_budgets() {
        assert!(serde_json::from_value::<EvidenceInput>(serde_json::json!({"query":"Atlas","unknown":true})).is_err());
        let mut input: EvidenceInput = serde_json::from_value(serde_json::json!({"query":"Atlas"})).unwrap();
        assert!(input.validate().is_ok());
        assert!(!input.routes.iter().any(|r| r.route == EvidenceRoute::Session));
        input.routes.push(input.routes[0].clone());
        assert!(input.validate().is_err());
        input.routes.pop(); input.budget = 32001;
        assert!(input.validate().is_err());
    }
}
