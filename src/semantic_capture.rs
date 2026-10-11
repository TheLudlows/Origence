//! Bounded model extraction into caller-selected identities. Model output is data.
use crate::{
    error::{AppError, Result},
    memory_identity::MemoryIdentity,
    memory_semantics::{SemanticMemoryInput, SemanticOperation, Statement},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub const EXTRACTOR: &str = "semantic-capture:v1";
pub const MAX_SOURCE_BYTES: usize = 64 * 1024;
pub const MAX_CANDIDATES: usize = 20;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureTarget {
    pub identity: MemoryIdentity,
    pub expected_version: i32,
    #[serde(default)]
    pub allow_update: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticCaptureInput {
    pub content: String,
    pub targets: Vec<CaptureTarget>,
}
impl SemanticCaptureInput {
    pub fn validate(&self) -> Result<()> {
        crate::parsing::validate_text(&self.content)?;
        if self.content.len() > MAX_SOURCE_BYTES || self.targets.len() > MAX_CANDIDATES {
            return Err(AppError::Invalid("semantic capture limit exceeded".into()));
        }
        let mut seen = BTreeSet::new();
        for target in &self.targets {
            target
                .identity
                .validate()
                .map_err(|_| AppError::Invalid("invalid capture identity".into()))?;
            if !(0..i32::MAX).contains(&target.expected_version)
                || !seen
                    .insert(serde_json::to_string(&target.identity).map_err(anyhow::Error::from)?)
            {
                return Err(AppError::Invalid(
                    "invalid or duplicate capture target".into(),
                ));
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub target: Option<usize>,
    pub operation: SemanticOperation,
    pub statements: Vec<Statement>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extraction {
    pub candidates: Vec<Candidate>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractionProvenance {
    pub parser: String,
    pub model: String,
}
impl Candidate {
    /// Model classification never grants overwrite authority.
    pub fn input(&self, request: &SemanticCaptureInput) -> Result<SemanticMemoryInput> {
        let target = self
            .target
            .map(|i| request.targets.get(i).ok_or_else(invalid))
            .transpose()?;
        let operation = if self.operation == SemanticOperation::Update
            && !target.is_some_and(|t| t.allow_update)
        {
            SemanticOperation::Ambiguous
        } else {
            self.operation
        };
        let input = SemanticMemoryInput {
            identity: target.map(|t| t.identity.clone()),
            content: request.content.clone(),
            operation,
            expected_version: target.map_or(0, |t| t.expected_version),
            statements: self.statements.clone(),
        };
        input.validate().map_err(|_| invalid())?;
        Ok(input)
    }
}
fn invalid() -> AppError {
    AppError::Unavailable("invalid semantic extraction".into())
}
pub fn parse_extraction(request: &SemanticCaptureInput, value: Value) -> Result<Extraction> {
    request.validate()?;
    let extraction: Extraction = serde_json::from_value(value).map_err(|_| invalid())?;
    if extraction.candidates.len() > MAX_CANDIDATES
        || extraction
            .candidates
            .iter()
            .map(|c| c.statements.len())
            .sum::<usize>()
            > crate::memory_semantics::MAX_MEMBERS
    {
        return Err(invalid());
    }
    let mut seen = BTreeSet::new();
    for candidate in &extraction.candidates {
        if let Some(target) = candidate.target
            && !seen.insert(target)
        {
            return Err(invalid());
        }
        candidate.input(request)?;
    }
    Ok(extraction)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn request() -> SemanticCaptureInput {
        serde_json::from_value(json!({"content":"前言🦀 é 使用 Rust","targets":[{"identity":{"subject":{"kind":"service","stable_id":"Atlas"},"predicate":"language","context":{"env":"prod"}},"expected_version":1}]})).unwrap()
    }
    fn candidate() -> Value {
        let r = request();
        let start = r.content.find("使用").unwrap();
        json!({"target":0,"operation":"assert","statements":[{"value":"Rust","quote":"使用 Rust","byte_start":start,"byte_end":r.content.len()}]})
    }
    #[test]
    fn semantic_capture_utf8_and_explicit_overwrite_authority() {
        let mut r = request();
        let mut c = candidate();
        c["operation"] = json!("update");
        let e = parse_extraction(&r, json!({"candidates":[c]})).unwrap();
        assert_eq!(
            e.candidates[0].input(&r).unwrap().operation,
            SemanticOperation::Ambiguous
        );
        r.targets[0].allow_update = true;
        assert_eq!(
            e.candidates[0].input(&r).unwrap().operation,
            SemanticOperation::Update
        );
        let mut c = candidate();
        c["target"] = Value::Null;
        let e = parse_extraction(&r, json!({"candidates":[c]})).unwrap();
        assert_eq!(
            e.candidates[0].input(&r).unwrap().uncertain_status(),
            Some("ambiguous")
        );
        assert!(
            parse_extraction(&r, json!({"candidates":[]}))
                .unwrap()
                .candidates
                .is_empty()
        );
    }
    #[test]
    fn semantic_capture_rejects_forged_scope_spans_and_partial_batches() {
        let r = request();
        for (path, value) in [
            ("target", json!(1)),
            ("identity", json!({})),
            ("approved", json!(true)),
        ] {
            let mut c = candidate();
            c[path] = value;
            assert!(parse_extraction(&r, json!({"candidates":[candidate(),c]})).is_err());
        }
        for field in ["byte_start", "byte_end"] {
            let mut c = candidate();
            c["statements"][0][field] = json!(1);
            assert!(parse_extraction(&r, json!({"candidates":[c]})).is_err());
        }
        let mut c = candidate();
        c["statements"][0]["quote"] = json!("invented");
        assert!(parse_extraction(&r, json!({"candidates":[c]})).is_err());
        assert!(parse_extraction(&r, json!({"candidates":[candidate(),candidate()]})).is_err());
        assert!(parse_extraction(&r, json!({"candidates":[],"publish":true})).is_err());
    }
    #[test]
    fn semantic_capture_bounds_input_and_model_output() {
        let mut r = request();
        r.targets.push(r.targets[0].clone());
        assert!(r.validate().is_err());
        r = request();
        r.content = "x".repeat(MAX_SOURCE_BYTES + 1);
        assert!(r.validate().is_err());
        r = request();
        r.targets[0].expected_version = i32::MAX;
        assert!(r.validate().is_err());
        let mut c = candidate();
        c["target"] = Value::Null;
        assert!(parse_extraction(&request(), json!({"candidates":vec![c.clone();21]})).is_err());
        c["statements"] = json!(vec![c["statements"][0].clone(); 33]);
        assert!(parse_extraction(&request(), json!({"candidates":[c]})).is_err());
    }
}
