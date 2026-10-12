//! Typed internal jobs over the existing version-one durable JSON envelope.
//!
//! Queue and storage adapters keep their raw operation/payload contract. Decode
//! once at the application boundary; retain that original envelope for durable
//! `publication` and `semantic_capture_plan` recovery state. Encoding a typed
//! request intentionally contains only its accepted input fields.
use crate::{
    error::{AppError, Result},
    memory_identity::MemoryIdentity,
    memory_semantics::{Member, MemoryProperty},
    semantic_capture::CaptureTarget,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

pub const SCHEMA_VERSION: u32 = 1;

/// Persisted operation names are stable independently of the Rust variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Publish,
    Ingest,
    Restore,
    Extract,
    AmlIngest,
    SemanticPublish,
    SemanticExtract,
    Cleanup,
}
impl JobKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Publish => "publish",
            Self::Ingest => "ingest",
            Self::Restore => "restore",
            Self::Extract => "extract",
            Self::AmlIngest => "aml_ingest",
            Self::SemanticPublish => "semantic_publish",
            Self::SemanticExtract => "semantic_extract",
            Self::Cleanup => "cleanup",
        }
    }
}
impl TryFrom<&str> for JobKind {
    type Error = AppError;

    fn try_from(value: &str) -> Result<Self> {
        match value {
            "publish" => Ok(Self::Publish),
            "ingest" => Ok(Self::Ingest),
            "restore" => Ok(Self::Restore),
            "extract" => Ok(Self::Extract),
            "aml_ingest" => Ok(Self::AmlIngest),
            "semantic_publish" => Ok(Self::SemanticPublish),
            "semantic_extract" => Ok(Self::SemanticExtract),
            "cleanup" => Ok(Self::Cleanup),
            _ => Err(AppError::Invalid("unknown job operation".into())),
        }
    }
}
impl std::str::FromStr for JobKind {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self> {
        Self::try_from(value)
    }
}

/// The two extract variants share one persisted operation but never fall back
/// from a present, malformed identity to legacy extraction.
///
/// The private Serde discriminator is an implementation detail of the codec;
/// callers must use `decode`/`encode` for the durable payload representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "__job_payload_kind", rename_all = "snake_case")]
pub enum JobPayload {
    Publish {
        expected_version: Option<i32>,
        format: String,
        embedding_profile: Option<String>,
    },
    Ingest {
        format: String,
        title: String,
        expected_version: Option<i32>,
        embedding_profile: Option<String>,
    },
    Restore {
        target_version: i32,
        expected_version: i32,
        title: Option<String>,
        embedding_profile: Option<String>,
    },
    LegacyExtract {
        embedding_profile: Option<String>,
    },
    IdentifiedExtract {
        identity: MemoryIdentity,
        expected_version: Option<i32>,
        embedding_profile: Option<String>,
    },
    AmlIngest {
        expected_version: Option<i32>,
        embedding_profile: String,
        parser: String,
    },
    SemanticPublish {
        expected_version: Option<i32>,
        embedding_profile: Option<String>,
        semantic_members: Vec<Member>,
        semantic_status: String,
        suggested_identity: Option<MemoryIdentity>,
        parser: String,
    },
    SemanticExtract {
        targets: Vec<CaptureTarget>,
        properties: Vec<MemoryProperty>,
        extractor: String,
        extraction_profile: String,
        extraction_model: String,
        embedding_profile: Option<String>,
    },
    Cleanup {
        target: String,
        id: Uuid,
    },
}

fn invalid_payload() -> AppError {
    // Deserialization diagnostics may contain source text, paths, or tokens.
    AppError::Invalid("invalid job payload".into())
}

impl JobPayload {
    pub fn decode(kind: &str, payload: &Value) -> Result<Self> {
        let kind = JobKind::try_from(kind)?;
        let object = payload.as_object().ok_or_else(invalid_payload)?;
        if object.get("schema_version").and_then(Value::as_u64) != Some(u64::from(SCHEMA_VERSION)) {
            return Err(AppError::Invalid("unsupported job payload schema".into()));
        }
        let discriminator = match kind {
            JobKind::Extract if object.contains_key("identity") => "identified_extract",
            JobKind::Extract => "legacy_extract",
            _ => kind.as_str(),
        };
        let mut fields = object.clone();
        fields.insert("__job_payload_kind".into(), json!(discriminator));
        let decoded: Self =
            serde_json::from_value(Value::Object(fields)).map_err(|_| invalid_payload())?;
        decoded.validate_identity()?;
        Ok(decoded)
    }

    pub fn encode(self) -> Result<(JobKind, Value)> {
        self.validate_identity()?;
        let kind = self.kind();
        let mut payload = serde_json::to_value(self).map_err(|_| invalid_payload())?;
        let fields = payload.as_object_mut().ok_or_else(invalid_payload)?;
        fields.remove("__job_payload_kind");
        fields.insert("schema_version".into(), json!(SCHEMA_VERSION));
        Ok((kind, payload))
    }

    fn validate_identity(&self) -> Result<()> {
        let validate =
            |identity: &MemoryIdentity| identity.validate().map_err(|_| invalid_payload());
        match self {
            Self::IdentifiedExtract { identity, .. } => validate(identity),
            Self::SemanticPublish {
                suggested_identity: Some(identity),
                ..
            } => validate(identity),
            Self::SemanticExtract { targets, .. } => {
                for target in targets {
                    validate(&target.identity)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub const fn kind(&self) -> JobKind {
        match self {
            Self::Publish { .. } => JobKind::Publish,
            Self::Ingest { .. } => JobKind::Ingest,
            Self::Restore { .. } => JobKind::Restore,
            Self::LegacyExtract { .. } | Self::IdentifiedExtract { .. } => JobKind::Extract,
            Self::AmlIngest { .. } => JobKind::AmlIngest,
            Self::SemanticPublish { .. } => JobKind::SemanticPublish,
            Self::SemanticExtract { .. } => JobKind::SemanticExtract,
            Self::Cleanup { .. } => JobKind::Cleanup,
        }
    }

    pub fn embedding_profile(&self) -> Option<&str> {
        match self {
            Self::Publish {
                embedding_profile, ..
            }
            | Self::Ingest {
                embedding_profile, ..
            }
            | Self::Restore {
                embedding_profile, ..
            }
            | Self::LegacyExtract { embedding_profile }
            | Self::IdentifiedExtract {
                embedding_profile, ..
            }
            | Self::SemanticPublish {
                embedding_profile, ..
            }
            | Self::SemanticExtract {
                embedding_profile, ..
            } => embedding_profile.as_deref(),
            Self::AmlIngest {
                embedding_profile, ..
            } => Some(embedding_profile),
            Self::Cleanup { .. } => None,
        }
    }

    pub fn expected_version(&self) -> Option<i32> {
        match self {
            Self::Publish {
                expected_version, ..
            }
            | Self::Ingest {
                expected_version, ..
            }
            | Self::IdentifiedExtract {
                expected_version, ..
            }
            | Self::AmlIngest {
                expected_version, ..
            }
            | Self::SemanticPublish {
                expected_version, ..
            } => *expected_version,
            Self::Restore {
                expected_version, ..
            } => Some(*expected_version),
            Self::LegacyExtract { .. } | Self::SemanticExtract { .. } | Self::Cleanup { .. } => {
                None
            }
        }
    }

    pub fn title(&self) -> Option<&str> {
        match self {
            Self::Ingest { title, .. } => Some(title),
            Self::Restore { title, .. } => title.as_deref(),
            _ => None,
        }
    }

    pub fn target_version(&self) -> Option<i32> {
        match self {
            Self::Restore { target_version, .. } => Some(*target_version),
            _ => None,
        }
    }

    pub fn identity(&self) -> Option<&MemoryIdentity> {
        match self {
            Self::IdentifiedExtract { identity, .. } => Some(identity),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_semantics::{Cardinality, Statement};

    fn identity() -> MemoryIdentity {
        serde_json::from_value(json!({
            "subject":{"kind":"service","stable_id":"Atlas"},
            "predicate":"language", "context":{"env":"prod"}
        }))
        .unwrap()
    }

    fn fixtures() -> Vec<(JobPayload, &'static str, Value)> {
        let identity = identity();
        let member = Member {
            statement: Statement {
                value: "Rust".into(),
                quote: "uses Rust".into(),
                byte_start: 0,
                byte_end: 9,
            },
            source_event_id: Uuid::from_u128(1),
            source_version: 1,
            extraction: None,
        };
        let target = CaptureTarget {
            identity: identity.clone(),
            expected_version: 1,
            allow_update: false,
        };
        let property = MemoryProperty {
            predicate: "language".into(),
            cardinality: Cardinality::Single,
            required_context: vec!["env".into()],
        };
        vec![
            (
                JobPayload::Publish {
                    expected_version: None,
                    format: "text".into(),
                    embedding_profile: None,
                },
                "publish",
                json!({"schema_version":1,"expected_version":null,"format":"text","embedding_profile":null}),
            ),
            (
                JobPayload::Ingest {
                    format: "markdown".into(),
                    title: "Knowledge".into(),
                    expected_version: Some(2),
                    embedding_profile: Some("embedding:v1".into()),
                },
                "ingest",
                json!({"schema_version":1,"format":"markdown","title":"Knowledge","expected_version":2,"embedding_profile":"embedding:v1"}),
            ),
            (
                JobPayload::Restore {
                    target_version: 1,
                    expected_version: 3,
                    title: None,
                    embedding_profile: Some("embedding:v1".into()),
                },
                "restore",
                json!({"schema_version":1,"target_version":1,"expected_version":3,"title":null,"embedding_profile":"embedding:v1"}),
            ),
            (
                JobPayload::LegacyExtract {
                    embedding_profile: None,
                },
                "extract",
                json!({"schema_version":1,"embedding_profile":null}),
            ),
            (
                JobPayload::IdentifiedExtract {
                    identity: identity.clone(),
                    expected_version: Some(2),
                    embedding_profile: Some("embedding:v1".into()),
                },
                "extract",
                json!({"schema_version":1,"identity":identity,"expected_version":2,"embedding_profile":"embedding:v1"}),
            ),
            (
                JobPayload::AmlIngest {
                    expected_version: None,
                    embedding_profile: "embedding:v1".into(),
                    parser: "aml-message-ranges-v1".into(),
                },
                "aml_ingest",
                json!({"schema_version":1,"expected_version":null,"embedding_profile":"embedding:v1","parser":"aml-message-ranges-v1"}),
            ),
            (
                JobPayload::SemanticPublish {
                    expected_version: Some(1),
                    embedding_profile: None,
                    semantic_members: vec![member.clone()],
                    semantic_status: "confirmed".into(),
                    suggested_identity: Some(identity.clone()),
                    parser: "memory-semantics:v1".into(),
                },
                "semantic_publish",
                json!({"schema_version":1,"expected_version":1,"embedding_profile":null,"semantic_members":[member],"semantic_status":"confirmed","suggested_identity":identity,"parser":"memory-semantics:v1"}),
            ),
            (
                JobPayload::SemanticExtract {
                    targets: vec![target.clone()],
                    properties: vec![property.clone()],
                    extractor: "semantic-capture:v1".into(),
                    extraction_profile: "extract:v1".into(),
                    extraction_model: "model".into(),
                    embedding_profile: None,
                },
                "semantic_extract",
                json!({"schema_version":1,"targets":[target],"properties":[property],"extractor":"semantic-capture:v1","extraction_profile":"extract:v1","extraction_model":"model","embedding_profile":null}),
            ),
            (
                JobPayload::Cleanup {
                    target: "event".into(),
                    id: Uuid::from_u128(1),
                },
                "cleanup",
                json!({"schema_version":1,"target":"event","id":"00000000-0000-0000-0000-000000000001"}),
            ),
        ]
    }

    #[test]
    fn all_job_variants_preserve_golden_version_one_payloads() {
        let fixtures = fixtures();
        assert_eq!(fixtures.len(), 9);
        for (typed, operation, golden) in fixtures {
            let (kind, encoded) = typed.clone().encode().unwrap();
            assert_eq!(kind.as_str(), operation);
            assert_eq!(JobKind::try_from(operation).unwrap(), kind);
            assert_eq!(encoded, golden);
            let decoded = JobPayload::decode(operation, &golden).unwrap();
            assert_eq!(
                std::mem::discriminant(&decoded),
                std::mem::discriminant(&typed)
            );
            assert_eq!(decoded.kind(), typed.kind());
            assert_eq!(decoded.embedding_profile(), typed.embedding_profile());
            assert_eq!(decoded.expected_version(), typed.expected_version());
            assert_eq!(decoded.title(), typed.title());
            assert_eq!(decoded.target_version(), typed.target_version());
            assert_eq!(decoded.identity(), typed.identity());
            assert_eq!(decoded.encode().unwrap(), (kind, golden));
        }
    }

    #[test]
    fn malformed_fields_and_required_fields_fail_without_raw_diagnostics() {
        for (_, operation, golden) in fixtures() {
            for (field, value) in golden.as_object().unwrap() {
                if field == "schema_version" || value.is_null() {
                    continue;
                }
                let mut malformed = golden.clone();
                malformed[field] = json!({"private_source":"must not appear in errors"});
                let error = JobPayload::decode(operation, &malformed).unwrap_err();
                assert!(
                    matches!(error, AppError::Invalid(ref text) if text == "invalid job payload")
                );
            }
        }
        for (operation, fields) in [
            ("publish", &["format"][..]),
            ("ingest", &["format", "title"][..]),
            ("restore", &["target_version", "expected_version"][..]),
            ("aml_ingest", &["embedding_profile", "parser"][..]),
            (
                "semantic_publish",
                &["semantic_members", "semantic_status", "parser"][..],
            ),
            (
                "semantic_extract",
                &[
                    "targets",
                    "properties",
                    "extractor",
                    "extraction_profile",
                    "extraction_model",
                ][..],
            ),
            ("cleanup", &["target", "id"][..]),
        ] {
            let golden = fixtures()
                .into_iter()
                .find(|(_, op, _)| *op == operation)
                .unwrap()
                .2;
            for field in fields {
                let mut malformed = golden.clone();
                malformed.as_object_mut().unwrap().remove(*field);
                assert!(JobPayload::decode(operation, &malformed).is_err());
            }
        }
        for (_, operation, golden) in fixtures() {
            for field in [
                "embedding_profile",
                "expected_version",
                "title",
                "suggested_identity",
            ] {
                if golden.get(field).is_some_and(Value::is_null) {
                    let mut malformed = golden.clone();
                    malformed[field] = json!(["private source"]);
                    assert!(JobPayload::decode(operation, &malformed).is_err());
                }
            }
        }
    }

    #[test]
    fn identity_presence_never_falls_back_to_legacy_extraction() {
        let mut value = json!({"schema_version":1,"embedding_profile":null});
        assert!(matches!(
            JobPayload::decode("extract", &value).unwrap(),
            JobPayload::LegacyExtract { .. }
        ));
        for identity in [
            Value::Null,
            json!("private source"),
            json!({}),
            json!({
                "subject":{"kind":"service","stable_id":""},
                "predicate":"language", "context":{}
            }),
        ] {
            value["identity"] = identity;
            assert!(
                matches!(JobPayload::decode("extract", &value), Err(AppError::Invalid(ref text)) if text == "invalid job payload")
            );
        }
        value["identity"] = json!(identity());
        assert!(matches!(
            JobPayload::decode("extract", &value).unwrap(),
            JobPayload::IdentifiedExtract { .. }
        ));
        let mut invalid_identity = identity();
        invalid_identity.subject.stable_id.clear();
        assert!(
            JobPayload::IdentifiedExtract {
                identity: invalid_identity,
                expected_version: None,
                embedding_profile: None,
            }
            .encode()
            .is_err()
        );
    }

    #[test]
    fn absent_optional_fields_keep_existing_none_semantics() {
        for (typed, operation, golden) in fixtures() {
            let mut absent = golden.clone();
            for field in [
                "embedding_profile",
                "expected_version",
                "title",
                "suggested_identity",
            ] {
                if golden.get(field).is_some_and(Value::is_null) {
                    absent.as_object_mut().unwrap().remove(field);
                }
            }
            assert_eq!(
                JobPayload::decode(operation, &absent)
                    .unwrap()
                    .encode()
                    .unwrap(),
                typed.encode().unwrap()
            );
        }
    }

    #[test]
    fn schema_and_unknown_operation_errors_are_fixed_and_redacted() {
        let base = json!({"schema_version":1,"embedding_profile":null});
        for version in [
            Value::Null,
            json!(0),
            json!(2),
            json!("1"),
            json!(1.0),
            json!(-1),
        ] {
            let mut value = base.clone();
            value["schema_version"] = version;
            assert!(
                matches!(JobPayload::decode("extract", &value), Err(AppError::Invalid(ref text)) if text == "unsupported job payload schema")
            );
        }
        assert!(JobPayload::decode("extract", &json!({})).is_err());
        for malformed in [Value::Null, json!([]), json!("private source")] {
            assert!(
                matches!(JobPayload::decode("extract", &malformed), Err(AppError::Invalid(ref text)) if text == "invalid job payload")
            );
        }
        assert!(
            matches!(JobPayload::decode("private source", &base), Err(AppError::Invalid(ref text)) if text == "unknown job operation")
        );
    }

    #[test]
    fn recovery_extras_stay_in_original_envelope_and_do_not_affect_decode() {
        for (_, operation, golden) in fixtures() {
            let mut envelope = golden.clone();
            envelope["publication"] = json!({"generation":7,"opaque":"saved publication"});
            envelope["semantic_capture_plan"] = json!({"opaque":"saved plan"});
            envelope["future_field"] = json!([1, 2, 3]);
            // A payload field cannot override the operation selected by storage.
            envelope["__job_payload_kind"] = json!("cleanup");
            let original = envelope.clone();
            let decoded = JobPayload::decode(operation, &envelope).unwrap();
            assert_eq!(envelope, original);
            assert_eq!(decoded.encode().unwrap().1, golden);
        }
    }
}
