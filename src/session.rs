//! Immutable session-turn and feedback request contracts.
//!
//! Session and feedback ownership comes from the authenticated workspace and
//! route parameters. Callers report only evidence they actually used; retrieval
//! candidates are not implicitly treated as used evidence. Archive provenance
//! and stable feedback references are resolved and assigned by the server.

use std::collections::HashSet;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Maximum UTF-8 byte length of each question and answer.
pub const MAX_SESSION_TEXT_BYTES: usize = 64 * 1024;
/// Maximum number of distinct evidence references in one turn.
pub const MAX_USED_EVIDENCE: usize = 64;
/// Maximum UTF-8 byte length of an optional feedback comment.
pub const MAX_FEEDBACK_COMMENT_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Chunk,
    Entity,
    Relation,
}

/// A caller-supplied exact reference to evidence used to produce the answer.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields)]
pub struct UsedEvidence {
    pub kind: EvidenceKind,
    pub id: Uuid,
    pub asset_id: Uuid,
    pub asset_version: i32,
    pub chunk_id: Uuid,
}

impl UsedEvidence {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.id.is_nil() || self.asset_id.is_nil() || self.chunk_id.is_nil() {
            return Err("evidence identifiers must not be nil");
        }
        if self.asset_version <= 0 {
            return Err("asset_version must be positive");
        }
        if self.kind == EvidenceKind::Chunk && self.id != self.chunk_id {
            return Err("chunk evidence id must equal chunk_id");
        }
        Ok(())
    }
}

/// Immutable input for a turn in the session identified by the request path.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SessionTurnInput {
    pub turn_id: Uuid,
    pub question: String,
    pub answer: String,
    /// Required even when empty: an empty list explicitly reports no evidence.
    pub used_evidence: Vec<UsedEvidence>,
}

impl SessionTurnInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.turn_id.is_nil() {
            return Err("turn_id must not be nil");
        }
        if self.question.trim().is_empty() || self.answer.trim().is_empty() {
            return Err("question and answer must not be blank");
        }
        if self.question.len() > MAX_SESSION_TEXT_BYTES
            || self.answer.len() > MAX_SESSION_TEXT_BYTES
        {
            return Err("question and answer must each be at most 65536 UTF-8 bytes");
        }
        if self.used_evidence.len() > MAX_USED_EVIDENCE {
            return Err("used_evidence must contain at most 64 references");
        }
        let mut seen = HashSet::with_capacity(self.used_evidence.len());
        for reference in &self.used_evidence {
            reference.validate()?;
            if !seen.insert(reference) {
                return Err("used_evidence must not contain duplicate references");
            }
        }
        Ok(())
    }
}

/// Server-derived provenance captured when a turn is accepted.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArchivedEvidence {
    /// Server-issued stable identity for feedback on this archived reference.
    pub evidence_ref_id: Uuid,
    pub reference: UsedEvidence,
    /// Original member's supporting source, which may differ from the root.
    pub source_event_id: Uuid,
    /// Original member's source version, not necessarily the asset version.
    pub source_version: i32,
    /// Root source of the referenced asset version.
    pub version_source_event_id: Uuid,
    /// Original immutable locator, including any parser/extraction provenance.
    pub locator: Value,
}

impl ArchivedEvidence {
    pub fn validate(&self) -> Result<(), &'static str> {
        self.reference.validate()?;
        if self.evidence_ref_id.is_nil()
            || self.source_event_id.is_nil()
            || self.version_source_event_id.is_nil()
        {
            return Err("archived evidence identifiers must not be nil");
        }
        if self.source_version <= 0 {
            return Err("source_version must be positive");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq, Hash)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeedbackTarget {
    Turn,
    Evidence { evidence_ref_id: Uuid },
}

impl<'de> Deserialize<'de> for FeedbackTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // An empty struct variant rejects stray fields for the turn target too.
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
        enum Target {
            Turn {},
            Evidence { evidence_ref_id: Uuid },
        }

        Ok(match Target::deserialize(deserializer)? {
            Target::Turn {} => Self::Turn,
            Target::Evidence { evidence_ref_id } => Self::Evidence { evidence_ref_id },
        })
    }
}

impl FeedbackTarget {
    pub fn validate(&self) -> Result<(), &'static str> {
        if let Self::Evidence { evidence_ref_id } = self
            && evidence_ref_id.is_nil()
        {
            return Err("evidence_ref_id must not be nil");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackRating {
    Helpful,
    Unhelpful,
    Neutral,
}

/// Immutable feedback on a turn or one of its archived evidence references.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FeedbackInput {
    pub feedback_id: Uuid,
    pub target: FeedbackTarget,
    pub rating: FeedbackRating,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

impl FeedbackInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.feedback_id.is_nil() {
            return Err("feedback_id must not be nil");
        }
        self.target.validate()?;
        if self
            .comment
            .as_ref()
            .is_some_and(|comment| comment.len() > MAX_FEEDBACK_COMMENT_BYTES)
        {
            return Err("comment must be at most 4096 UTF-8 bytes");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn evidence() -> UsedEvidence {
        let chunk_id = Uuid::new_v4();
        UsedEvidence {
            kind: EvidenceKind::Chunk,
            id: chunk_id,
            asset_id: Uuid::new_v4(),
            asset_version: 1,
            chunk_id,
        }
    }

    fn turn() -> SessionTurnInput {
        SessionTurnInput {
            turn_id: Uuid::new_v4(),
            question: "What changed?".into(),
            answer: "The supported value changed.".into(),
            used_evidence: Vec::new(),
        }
    }

    fn feedback() -> FeedbackInput {
        FeedbackInput {
            feedback_id: Uuid::new_v4(),
            target: FeedbackTarget::Turn,
            rating: FeedbackRating::Helpful,
            comment: None,
        }
    }

    #[test]
    fn turn_requires_every_field_and_explicit_empty_evidence_is_valid() {
        let input = turn();
        let value = serde_json::to_value(&input).unwrap();
        let parsed: SessionTurnInput = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(parsed, input);
        assert_eq!(parsed.validate(), Ok(()));
        for field in ["turn_id", "question", "answer", "used_evidence"] {
            let mut missing = value.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<SessionTurnInput>(missing).is_err());
            let mut null = value.clone();
            null[field] = Value::Null;
            assert!(serde_json::from_value::<SessionTurnInput>(null).is_err());
        }
        for field in ["session_id", "unknown", "source_event_id"] {
            let mut extra = value.clone();
            extra[field] = json!(Uuid::new_v4());
            assert!(serde_json::from_value::<SessionTurnInput>(extra).is_err());
        }
    }

    #[test]
    fn used_evidence_is_strict_and_has_only_supported_kinds() {
        let value = serde_json::to_value(evidence()).unwrap();
        for field in ["kind", "id", "asset_id", "asset_version", "chunk_id"] {
            let mut missing = value.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<UsedEvidence>(missing).is_err());
        }
        for field in ["locator", "source_event_id", "evidence_ref_id"] {
            let mut extra = value.clone();
            extra[field] = json!("caller cannot set archive provenance");
            assert!(serde_json::from_value::<UsedEvidence>(extra).is_err());
        }
        for kind in ["chunk", "entity", "relation"] {
            let mut candidate = value.clone();
            candidate["kind"] = json!(kind);
            assert!(serde_json::from_value::<UsedEvidence>(candidate).is_ok());
        }
        for kind in ["memory", "summary", "Chunk", "other"] {
            let mut invalid = value.clone();
            invalid["kind"] = json!(kind);
            assert!(serde_json::from_value::<UsedEvidence>(invalid).is_err());
        }
    }

    #[test]
    fn used_evidence_rejects_nil_ids_nonpositive_versions_and_wrong_chunk() {
        let valid = evidence();
        assert_eq!(valid.validate(), Ok(()));
        for field in ["id", "asset_id", "chunk_id"] {
            let mut value = serde_json::to_value(&valid).unwrap();
            value[field] = json!(Uuid::nil());
            let invalid: UsedEvidence = serde_json::from_value(value).unwrap();
            assert!(invalid.validate().is_err());
        }
        for version in [0, -1, i32::MIN] {
            let mut invalid = valid.clone();
            invalid.asset_version = version;
            assert!(invalid.validate().is_err());
        }
        let mut different = valid;
        different.id = Uuid::new_v4();
        assert!(different.validate().is_err());
        for kind in [EvidenceKind::Entity, EvidenceKind::Relation] {
            different.kind = kind;
            assert_eq!(different.validate(), Ok(()));
        }
    }

    #[test]
    fn turn_rejects_nil_id_and_blank_text() {
        let mut input = turn();
        input.turn_id = Uuid::nil();
        assert!(input.validate().is_err());
        for blank in ["", " \t\r\n", "\u{3000}"] {
            let mut input = turn();
            input.question = blank.into();
            assert!(input.validate().is_err());
            let mut input = turn();
            input.answer = blank.into();
            assert!(input.validate().is_err());
        }
    }

    #[test]
    fn turn_text_limits_count_utf8_bytes_and_include_boundary() {
        for question in [true, false] {
            let mut input = turn();
            let text = "界".repeat(MAX_SESSION_TEXT_BYTES / 3) + "a";
            assert_eq!(text.len(), MAX_SESSION_TEXT_BYTES);
            if question {
                input.question = text;
            } else {
                input.answer = text;
            }
            assert_eq!(input.validate(), Ok(()));
            if question {
                input.question.push('a');
            } else {
                input.answer.push('a');
            }
            assert!(input.validate().is_err());
        }
    }

    #[test]
    fn turn_caps_used_evidence_and_rejects_exact_duplicates() {
        let mut input = turn();
        input.used_evidence = (0..MAX_USED_EVIDENCE).map(|_| evidence()).collect();
        assert_eq!(input.validate(), Ok(()));
        input.used_evidence.push(evidence());
        assert!(input.validate().is_err());

        let original = evidence();
        input.used_evidence = vec![original.clone(), original.clone()];
        assert!(input.validate().is_err());
        let mut next_version = original;
        next_version.asset_version += 1;
        input.used_evidence[1] = next_version;
        assert_eq!(input.validate(), Ok(()));
    }

    #[test]
    fn turn_validates_each_used_reference() {
        let mut input = turn();
        let mut invalid = evidence();
        invalid.asset_version = 0;
        input.used_evidence.push(invalid);
        assert!(input.validate().is_err());
    }

    #[test]
    fn archived_evidence_preserves_original_and_version_root_provenance() {
        let mut reference = evidence();
        reference.asset_version = 3;
        let archive = ArchivedEvidence {
            evidence_ref_id: Uuid::new_v4(),
            reference,
            source_event_id: Uuid::new_v4(),
            source_version: 1,
            version_source_event_id: Uuid::new_v4(),
            locator: json!({"parser":"semantic-capture:v1", "start":0, "end":9}),
        };
        assert_ne!(archive.source_event_id, archive.version_source_event_id);
        assert_eq!(archive.validate(), Ok(()));
        assert_eq!(
            serde_json::from_value::<ArchivedEvidence>(
                serde_json::to_value(&archive).unwrap()
            )
            .unwrap(),
            archive
        );
        for field in ["evidence_ref_id", "source_event_id", "version_source_event_id"] {
            let mut value = serde_json::to_value(&archive).unwrap();
            value[field] = json!(Uuid::nil());
            let invalid: ArchivedEvidence = serde_json::from_value(value).unwrap();
            assert!(invalid.validate().is_err());
        }
        let mut invalid = archive;
        invalid.source_version = 0;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn feedback_is_strict_with_optional_comment_and_closed_ratings() {
        let input = feedback();
        let value = serde_json::to_value(&input).unwrap();
        assert!(value.get("comment").is_none());
        let parsed: FeedbackInput = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(parsed, input);
        assert_eq!(parsed.validate(), Ok(()));
        for field in ["feedback_id", "target", "rating"] {
            let mut missing = value.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<FeedbackInput>(missing).is_err());
        }
        for field in ["session_id", "turn_id", "unknown"] {
            let mut extra = value.clone();
            extra[field] = json!(Uuid::new_v4());
            assert!(serde_json::from_value::<FeedbackInput>(extra).is_err());
        }
        for rating in ["helpful", "unhelpful", "neutral"] {
            let mut candidate = value.clone();
            candidate["rating"] = json!(rating);
            assert!(serde_json::from_value::<FeedbackInput>(candidate).is_ok());
        }
        for rating in [json!("free text"), json!("Helpful"), json!(1), Value::Null] {
            let mut invalid = value.clone();
            invalid["rating"] = rating;
            assert!(serde_json::from_value::<FeedbackInput>(invalid).is_err());
        }
    }

    #[test]
    fn feedback_targets_reject_missing_fields_unknown_fields_and_old_shape() {
        let id = Uuid::new_v4();
        for target in [
            FeedbackTarget::Turn,
            FeedbackTarget::Evidence { evidence_ref_id: id },
        ] {
            let value = serde_json::to_value(&target).unwrap();
            assert_eq!(
                serde_json::from_value::<FeedbackTarget>(value).unwrap(),
                target
            );
            assert_eq!(target.validate(), Ok(()));
        }
        for invalid in [
            json!({}),
            json!({"type":"unknown"}),
            json!({"type":"turn", "unknown":true}),
            json!({"type":"turn", "evidence_ref_id":id}),
            json!({"type":"evidence"}),
            json!({"type":"evidence", "evidence_ref_id":null}),
            json!({"type":"evidence", "evidence_ref_id":id, "unknown":true}),
            json!({"type":"evidence", "reference":evidence()}),
            json!({"type":"evidence", "evidence_ref_id":"invalid-uuid"}),
        ] {
            assert!(serde_json::from_value::<FeedbackTarget>(invalid).is_err());
        }
        assert!(
            FeedbackTarget::Evidence {
                evidence_ref_id: Uuid::nil()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn feedback_validates_ids_and_comment_utf8_byte_limit() {
        let mut input = feedback();
        input.feedback_id = Uuid::nil();
        assert!(input.validate().is_err());
        input = feedback();
        input.target = FeedbackTarget::Evidence {
            evidence_ref_id: Uuid::nil(),
        };
        assert!(input.validate().is_err());
        input = feedback();
        input.comment = Some("界".repeat(MAX_FEEDBACK_COMMENT_BYTES / 3) + "a");
        assert_eq!(input.comment.as_ref().unwrap().len(), MAX_FEEDBACK_COMMENT_BYTES);
        assert_eq!(input.validate(), Ok(()));
        input.comment.as_mut().unwrap().push('a');
        assert!(input.validate().is_err());
        input.comment = Some(String::new());
        assert_eq!(input.validate(), Ok(()));
    }
}
