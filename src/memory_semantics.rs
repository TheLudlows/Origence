//! Explicit, deterministic S2 contracts. No inferred aliases or semantic overwrite.
use crate::{
    error::{AppError, Result},
    memory_identity::{MemoryIdentity, MemorySubject, SubjectKind},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cardinality {
    Single,
    Multi,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryProperty {
    pub predicate: String,
    pub cardinality: Cardinality,
    /// Required conditions prevent a conditional property from becoming global.
    #[serde(default)]
    pub required_context: Vec<String>,
}
impl MemoryProperty {
    pub fn validate(&self) -> Result<()> {
        let mut context = BTreeMap::new();
        for key in &self.required_context {
            if context.insert(key.clone(), "required".into()).is_some() {
                return Err(AppError::Invalid("duplicate required context".into()));
            }
        }
        MemoryIdentity {
            subject: MemorySubject {
                kind: SubjectKind::Service,
                stable_id: "validation".into(),
            },
            predicate: self.predicate.clone(),
            context,
        }
        .validate()
        .map_err(|_| AppError::Invalid("invalid memory property".into()))
    }
    pub fn check_identity(&self, identity: &MemoryIdentity) -> Result<()> {
        if self.predicate != identity.predicate
            || self
                .required_context
                .iter()
                .any(|key| !identity.context.contains_key(key))
        {
            return Err(AppError::Invalid("missing property context".into()));
        }
        Ok(())
    }
}

/// Extraction/provenance contract for explicit, caller-classified assertions.
pub const PARSER: &str = "memory-semantics:v1";
pub const MAX_MEMBERS: usize = 32;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticOperation {
    Assert,
    Update,
    Supplement,
    Negate,
    Propose,
    Ambiguous,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Statement {
    pub value: String,
    pub quote: String,
    pub byte_start: usize,
    pub byte_end: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticMemoryInput {
    pub identity: Option<MemoryIdentity>,
    pub content: String,
    pub operation: SemanticOperation,
    pub expected_version: i32,
    pub statements: Vec<Statement>,
}
impl SemanticMemoryInput {
    pub fn validate(&self) -> Result<()> {
        crate::parsing::validate_text(&self.content)?;
        if !(0..i32::MAX).contains(&self.expected_version)
            || self.statements.is_empty()
            || self.statements.len() > MAX_MEMBERS
        {
            return Err(AppError::Invalid(
                "invalid semantic version or statement count".into(),
            ));
        }
        if let Some(identity) = &self.identity {
            identity
                .validate()
                .map_err(|_| AppError::Invalid("invalid memory identity".into()))?;
        } else if self.expected_version != 0 {
            return Err(AppError::Invalid(
                "unidentified input requires expected_version zero".into(),
            ));
        }
        for s in &self.statements {
            if s.value.is_empty()
                || s.value.len() > 256
                || s.value.trim() != s.value
                || s.value.chars().any(char::is_control)
                || s.quote.trim().is_empty()
                || s.quote.len() > 8192
                || self.content.get(s.byte_start..s.byte_end) != Some(s.quote.as_str())
            {
                return Err(AppError::Invalid(
                    "invalid semantic value or source span".into(),
                ));
            }
        }
        Ok(())
    }
    pub fn uncertain_status(&self) -> Option<&'static str> {
        match self.operation {
            SemanticOperation::Negate => Some("negated"),
            SemanticOperation::Propose => Some("proposed"),
            SemanticOperation::Ambiguous => Some("ambiguous"),
            _ if self.identity.is_none() => Some("ambiguous"),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub statement: Statement,
    pub source_event_id: uuid::Uuid,
    pub source_version: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extraction: Option<crate::semantic_capture::ExtractionProvenance>,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Duplicate,
    Publish,
    Unnormalized(&'static str),
}
/// Only exact values and explicit operations determine routing. Similar text is
/// never an update authorization. The service separately enforces versions.
pub fn decide(
    input: &SemanticMemoryInput,
    cardinality: Cardinality,
    current: &[Member],
) -> Result<Decision> {
    use std::collections::BTreeSet;
    if let Some(status) = input.uncertain_status() {
        return Ok(Decision::Unnormalized(status));
    }
    let new: BTreeSet<_> = input.statements.iter().map(|s| &s.value).collect();
    let old: BTreeSet<_> = current.iter().map(|m| &m.statement.value).collect();
    if cardinality == Cardinality::Single && new.len() > 1 {
        return Ok(Decision::Unnormalized("conflict"));
    }
    match (cardinality, input.operation) {
        (Cardinality::Single, SemanticOperation::Supplement)
        | (Cardinality::Multi, SemanticOperation::Update) => {
            return Err(AppError::Invalid(
                "operation incompatible with property cardinality".into(),
            ));
        }
        (Cardinality::Single, SemanticOperation::Assert) if !old.is_empty() && new != old => {
            return Ok(Decision::Unnormalized("conflict"));
        }
        (Cardinality::Multi, SemanticOperation::Assert)
            if !old.is_empty() && !new.is_subset(&old) =>
        {
            return Err(AppError::Invalid(
                "multi-value additions require supplement".into(),
            ));
        }
        _ => {}
    }
    Ok(if new.is_subset(&old) {
        Decision::Duplicate
    } else {
        Decision::Publish
    })
}
pub fn chunks(
    members: &[Member],
    status: &str,
    suggested_identity: Option<&MemoryIdentity>,
) -> Vec<crate::types::Chunk> {
    let support: std::collections::BTreeSet<_> =
        members.iter().map(|m| m.source_event_id).collect();
    members.iter().map(|m| crate::types::Chunk {
        content: m.statement.quote.clone(),
        locator: serde_json::json!({"parser":PARSER,"semantic_status":status,"semantic_value":m.statement.value,
            "source_event_id":m.source_event_id,"source_version":m.source_version,"support_sources":support,
            "byte_basis":"source_utf8","source_span":true,"byte_start":m.statement.byte_start,"byte_end":m.statement.byte_end,
            "suggested_identity":suggested_identity,"extraction":m.extraction}),
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn input() -> SemanticMemoryInput {
        serde_json::from_value(json!({"identity":{"subject":{"kind":"service","stable_id":"Atlas"},"predicate":"database.engine","context":{"env":"production"}},"content":"使用 PostgreSQL","operation":"assert","expected_version":0,"statements":[{"value":"postgres","quote":"使用 PostgreSQL","byte_start":0,"byte_end":"使用 PostgreSQL".len()}]})).unwrap()
    }
    fn existing(value: &str) -> Vec<Member> {
        let mut statement = input().statements.remove(0);
        statement.value = value.into();
        vec![Member {
            statement,
            source_event_id: uuid::Uuid::new_v4(),
            source_version: 1,
            extraction: None,
        }]
    }
    #[test]
    fn exact_values_route_duplicate_conflict_update_and_set_addition() {
        let mut i = input();
        i.validate().unwrap();
        assert_eq!(
            decide(&i, Cardinality::Single, &[]).unwrap(),
            Decision::Publish
        );
        assert_eq!(
            decide(&i, Cardinality::Single, &existing("postgres")).unwrap(),
            Decision::Duplicate
        );
        assert_eq!(
            decide(&i, Cardinality::Single, &existing("mysql")).unwrap(),
            Decision::Unnormalized("conflict")
        );
        i.operation = SemanticOperation::Update;
        assert_eq!(
            decide(&i, Cardinality::Single, &existing("mysql")).unwrap(),
            Decision::Publish
        );
        assert!(decide(&i, Cardinality::Multi, &existing("mysql")).is_err());
        i.operation = SemanticOperation::Supplement;
        assert!(decide(&i, Cardinality::Single, &[]).is_err());
        assert_eq!(
            decide(&i, Cardinality::Multi, &existing("mysql")).unwrap(),
            Decision::Publish
        );
        assert_eq!(
            decide(&i, Cardinality::Multi, &existing("postgres")).unwrap(),
            Decision::Duplicate
        );
    }
    #[test]
    fn uncertain_operations_and_missing_identity_never_publish_current_facts() {
        let mut i = input();
        for (operation, status) in [
            (SemanticOperation::Propose, "proposed"),
            (SemanticOperation::Negate, "negated"),
            (SemanticOperation::Ambiguous, "ambiguous"),
        ] {
            i.operation = operation;
            assert_eq!(
                decide(&i, Cardinality::Single, &existing("postgres")).unwrap(),
                Decision::Unnormalized(status)
            );
        }
        i.operation = SemanticOperation::Assert;
        i.identity = None;
        assert_eq!(
            decide(&i, Cardinality::Single, &[]).unwrap(),
            Decision::Unnormalized("ambiguous")
        );
    }
    #[test]
    fn quote_boundaries_limits_and_required_conditions_are_enforced() {
        let mut i = input();
        i.statements[0].byte_start = 1;
        assert!(i.validate().is_err());
        i = input();
        i.statements[0].quote = "invented".into();
        assert!(i.validate().is_err());
        i = input();
        i.statements = vec![i.statements[0].clone(); MAX_MEMBERS + 1];
        assert!(i.validate().is_err());
        i = input();
        i.expected_version = i32::MAX;
        assert!(i.validate().is_err());
        let property = MemoryProperty {
            predicate: "database.engine".into(),
            cardinality: Cardinality::Single,
            required_context: vec!["region".into()],
        };
        assert!(
            property
                .check_identity(input().identity.as_ref().unwrap())
                .is_err()
        );
    }
}
