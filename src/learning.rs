//! Bounded, explicitly requested feedback learning. Model suggestions are data;
//! helpful feedback alone is not proof that an answer or proposed rule is true.
use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::session::{ArchivedEvidence, FeedbackInput};

pub const MAX_FEEDBACK: usize = 32;
pub const MAX_SNAPSHOT_BYTES: usize = 128 * 1024;
pub const MAX_CANDIDATES: usize = 8;
pub const MAX_CONDITIONS: usize = 8;
pub const MAX_CONDITION_BYTES: usize = 256;
pub const MAX_SUPPORTS: usize = 32;
pub const MAX_QUOTE_BYTES: usize = 4096;
pub const MAX_STATEMENT_BYTES: usize = 8192;
pub const MAX_WHY_BYTES: usize = 4096;

fn default_max_feedback() -> usize {
    MAX_FEEDBACK
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImproveInput {
    #[schemars(with = "String")]
    pub improve_id: Uuid,
    /// Caller-selected scope. A proposal must preserve this exact list.
    pub conditions: Vec<String>,
    #[serde(default = "default_max_feedback")]
    pub max_feedback: usize,
}

impl ImproveInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.improve_id.is_nil() {
            return Err("improve_id must not be nil");
        }
        validate_conditions(&self.conditions)?;
        if self.max_feedback == 0 || self.max_feedback > MAX_FEEDBACK {
            return Err("max_feedback must be between 1 and 32");
        }
        Ok(())
    }
}

fn validate_conditions(conditions: &[String]) -> Result<(), &'static str> {
    if conditions.is_empty() || conditions.len() > MAX_CONDITIONS {
        return Err("conditions must contain between 1 and 8 entries");
    }
    let mut seen = HashSet::new();
    for condition in conditions {
        if condition.trim().is_empty() || condition.len() > MAX_CONDITION_BYTES {
            return Err("conditions must be nonblank and at most 256 UTF-8 bytes");
        }
        if !seen.insert(condition) {
            return Err("conditions must not contain duplicates");
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FrozenTurn {
    pub session_id: Uuid,
    pub turn_id: Uuid,
    pub source_event_id: Uuid,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub question: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub answer: String,
    pub used_evidence: Vec<ArchivedEvidence>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FrozenFeedback {
    pub sequence: i64,
    pub principal_id: Uuid,
    pub feedback: FeedbackInput,
    pub turn: FrozenTurn,
}

/// Explicit terminal accounting for input that was already withdrawn at freeze.
/// No source text or feedback comment from an invisible turn is retained here.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkippedFeedback {
    pub sequence: i64,
    pub principal_id: Uuid,
    pub feedback_id: Uuid,
    pub turn_id: Uuid,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FrozenSnapshot {
    pub items: Vec<FrozenFeedback>,
    pub skipped: Vec<SkippedFeedback>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ImproveRun {
    pub improve_id: Uuid,
    pub session_id: Uuid,
    pub job_id: Uuid,
    pub conditions: Vec<String>,
    pub lower_sequence: i64,
    pub target_sequence: i64,
    pub snapshot: FrozenSnapshot,
    pub proposal_plan: Option<Value>,
    pub verification_plan: Option<Value>,
    pub status: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SupportField {
    Question,
    Answer,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields)]
pub struct LearningSupport {
    pub turn_id: Uuid,
    pub field: SupportField,
    pub quote: String,
    pub byte_start: usize,
    pub byte_end: usize,
    pub feedback_id: Uuid,
    pub principal_id: Uuid,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LearningCandidate {
    pub statement: String,
    pub why: String,
    pub conditions: Vec<String>,
    pub supports: Vec<LearningSupport>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExperienceBasis {
    HumanConfirmation,
    AssetVersion {
        #[schemars(with = "String")]
        asset_id: Uuid,
        version: i32,
        content_sha256: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExperiencePublication {
    pub basis: ExperienceBasis,
    pub recorded_by: Uuid,
    pub recorded_at_ms: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DirectExperienceInput {
    pub statement: String,
    pub why: String,
    pub conditions: Vec<String>,
    pub basis: ExperienceBasis,
}

impl DirectExperienceInput {
    #[cfg(feature = "host")]
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        validate_conditions(&self.conditions)?;
        if self.statement.contains('\0')
            || self.why.contains('\0')
            || self
                .conditions
                .iter()
                .any(|condition| condition.contains('\0'))
        {
            return Err("experience text contains NUL");
        }
        validate_learning_text(&self.statement, &self.why)?;
        if let ExperienceBasis::AssetVersion {
            asset_id,
            version,
            content_sha256,
        } = &self.basis
            && (asset_id.is_nil()
                || *version <= 0
                || !crate::retrieval_diagnostics::digest(content_sha256))
        {
            return Err("experience requires an exact valid asset version and SHA-256");
        }
        Ok(())
    }
}

fn validate_learning_text(statement: &str, why: &str) -> Result<(), &'static str> {
    if statement.trim().is_empty() || statement.len() > MAX_STATEMENT_BYTES {
        return Err("learning statement must be nonblank and at most 8192 UTF-8 bytes");
    }
    if why.trim().is_empty() || why.len() > MAX_WHY_BYTES {
        return Err("learning why must be nonblank and at most 4096 UTF-8 bytes");
    }
    Ok(())
}

impl LearningCandidate {
    #[cfg(feature = "sqlite")]
    pub(crate) fn render_text(&self) -> String {
        format!(
            "Conditional learning\nConditions:\n{}\n\nStatement:\n{}\n\nWhy:\n{}",
            self.conditions.join("\n"),
            self.statement,
            self.why,
        )
    }

    /// Validate immutable exact UTF-8 evidence; this does not assert semantic truth.
    pub fn validate(
        &self,
        snapshot: &FrozenSnapshot,
        conditions: &[String],
    ) -> Result<(), &'static str> {
        self.validate_supports(snapshot, conditions, false)
    }

    #[cfg(feature = "sqlite")]
    pub(crate) fn validate_references(
        &self,
        snapshot: &FrozenSnapshot,
        conditions: &[String],
    ) -> Result<(), &'static str> {
        self.validate_supports(snapshot, conditions, true)
    }

    fn validate_supports(
        &self,
        snapshot: &FrozenSnapshot,
        conditions: &[String],
        digests: bool,
    ) -> Result<(), &'static str> {
        validate_conditions(conditions)?;
        if self.conditions != conditions {
            return Err("learning conditions must exactly match the request");
        }
        validate_learning_text(&self.statement, &self.why)?;
        if self.supports.is_empty() || self.supports.len() > MAX_SUPPORTS {
            return Err("learning must have between 1 and 32 supports");
        }
        let mut seen = HashSet::new();
        for support in &self.supports {
            if support.turn_id.is_nil()
                || support.feedback_id.is_nil()
                || support.principal_id.is_nil()
                || support.quote.is_empty()
                || support.quote.len() > MAX_QUOTE_BYTES
                || !seen.insert(support)
            {
                return Err("invalid or duplicate learning support");
            }
            let frozen = snapshot.items.iter().find(|item| {
                item.turn.turn_id == support.turn_id
                    && item.feedback.feedback_id == support.feedback_id
                    && item.principal_id == support.principal_id
            });
            let frozen = frozen.ok_or("learning support is outside the frozen feedback")?;
            let source = match support.field {
                SupportField::Question => &frozen.turn.question,
                SupportField::Answer => &frozen.turn.answer,
            };
            if support.byte_start >= support.byte_end
                || support.byte_end > crate::session::MAX_SESSION_TEXT_BYTES
                || support.byte_end - support.byte_start > MAX_QUOTE_BYTES
                || if digests && source.is_empty() {
                    !crate::retrieval_diagnostics::digest(&support.quote)
                } else {
                    source.get(support.byte_start..support.byte_end) != Some(support.quote.as_str())
                }
            {
                return Err("learning support must match exact UTF-8 source bytes");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{FeedbackRating, FeedbackTarget};

    #[test]
    fn request_limits_and_scope_cannot_be_model_supplied() {
        let id = Uuid::new_v4();
        let mut input: ImproveInput = serde_json::from_value(serde_json::json!({
            "improve_id":id,"conditions":["during rollout"]
        }))
        .unwrap();
        assert_eq!(input.max_feedback, MAX_FEEDBACK);
        input.validate().unwrap();
        input.conditions.clear();
        assert!(input.validate().is_err());
        input.conditions = vec!["x".into()];
        input.max_feedback = MAX_FEEDBACK + 1;
        assert!(input.validate().is_err());
        assert!(
            serde_json::from_value::<ImproveInput>(serde_json::json!({
                "improve_id":id,"conditions":["x"],"tenant_id":id
            }))
            .is_err()
        );
    }

    #[test]
    fn exact_unicode_support_binds_feedback_principal_and_conditions() {
        let turn = Uuid::new_v4();
        let feedback = Uuid::new_v4();
        let principal = Uuid::new_v4();
        let conditions = vec!["during rollout".to_string()];
        let snapshot = FrozenSnapshot {
            items: vec![FrozenFeedback {
                sequence: 1,
                principal_id: principal,
                feedback: FeedbackInput {
                    feedback_id: feedback,
                    target: FeedbackTarget::Turn,
                    rating: FeedbackRating::Helpful,
                    comment: None,
                },
                turn: FrozenTurn {
                    session_id: Uuid::new_v4(),
                    turn_id: turn,
                    source_event_id: Uuid::new_v4(),
                    question: "Check?".into(),
                    answer: "检查先".into(),
                    used_evidence: vec![],
                },
            }],
            skipped: vec![],
        };
        let mut candidate = LearningCandidate {
            statement: "Check first".into(),
            why: "A conditional suggestion supported by this turn".into(),
            conditions: conditions.clone(),
            supports: vec![LearningSupport {
                turn_id: turn,
                field: SupportField::Answer,
                quote: "检查".into(),
                byte_start: 0,
                byte_end: 6,
                feedback_id: feedback,
                principal_id: principal,
            }],
        };
        candidate.validate(&snapshot, &conditions).unwrap();
        candidate.supports[0].byte_end = 5;
        assert!(candidate.validate(&snapshot, &conditions).is_err());
        candidate.supports[0].byte_end = 6;
        candidate.supports[0].principal_id = Uuid::new_v4();
        assert!(candidate.validate(&snapshot, &conditions).is_err());
        candidate.supports[0].principal_id = principal;
        candidate.conditions.clear();
        assert!(candidate.validate(&snapshot, &conditions).is_err());
    }

    #[test]
    fn support_quote_byte_limit_accepts_boundary_and_rejects_one_more() {
        let id = Uuid::new_v4();
        let conditions = vec!["during rollout".to_string()];
        let answer = "x".repeat(MAX_QUOTE_BYTES + 1);
        let snapshot = FrozenSnapshot {
            items: vec![FrozenFeedback {
                sequence: 1,
                principal_id: id,
                feedback: FeedbackInput {
                    feedback_id: id,
                    target: FeedbackTarget::Turn,
                    rating: FeedbackRating::Helpful,
                    comment: None,
                },
                turn: FrozenTurn {
                    session_id: id,
                    turn_id: id,
                    source_event_id: id,
                    question: "Check?".into(),
                    answer: answer.clone(),
                    used_evidence: vec![],
                },
            }],
            skipped: vec![],
        };
        let mut candidate = LearningCandidate {
            statement: "Check first".into(),
            why: "Conditional suggestion".into(),
            conditions: conditions.clone(),
            supports: vec![LearningSupport {
                turn_id: id,
                field: SupportField::Answer,
                quote: answer[..MAX_QUOTE_BYTES].into(),
                byte_start: 0,
                byte_end: MAX_QUOTE_BYTES,
                feedback_id: id,
                principal_id: id,
            }],
        };
        candidate.validate(&snapshot, &conditions).unwrap();
        candidate.supports[0].quote = answer;
        candidate.supports[0].byte_end = MAX_QUOTE_BYTES + 1;
        assert!(candidate.validate(&snapshot, &conditions).is_err());
    }
}
