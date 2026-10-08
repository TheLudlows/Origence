use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct AuthContext {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub workspace_id: Uuid,
    pub role: String,
}

impl AuthContext {
    pub fn require(&self, permission: &str) -> crate::error::Result<()> {
        let allowed = match permission {
            "read" => true,
            "write" => matches!(self.role.as_str(), "writer" | "reviewer" | "admin"),
            "review" | "publish" => matches!(self.role.as_str(), "reviewer" | "admin"),
            "delete" => self.role == "admin",
            _ => false,
        };
        if allowed {
            Ok(())
        } else {
            Err(crate::error::AppError::Forbidden)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryInput {
    pub fact_key: String,
    pub content: String,
    #[serde(default)]
    pub publish_if_authorized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentifiedMemoryInput {
    pub identity: crate::memory_identity::MemoryIdentity,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_version: Option<i32>,
}

impl IdentifiedMemoryInput {
    pub fn validate_version(&self) -> crate::error::Result<()> {
        if self.expected_version.is_some_and(|version| version < 0) {
            return Err(crate::error::AppError::Invalid(
                "expected_version must be >=0".into(),
            ));
        }
        Ok(())
    }

    pub fn check_version(&self, current: Option<i32>) -> crate::error::Result<()> {
        self.validate_version()?;
        if self
            .expected_version
            .is_some_and(|expected| expected != current.unwrap_or(0))
        {
            return Err(crate::error::AppError::Conflict(
                "expected_version mismatch".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureInput {
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeInput {
    pub title: String,
    pub content: Option<String>,
    pub file_id: Option<Uuid>,
    #[serde(default = "text_format")]
    pub format: String,
    pub asset_id: Option<Uuid>,
    pub expected_version: Option<i32>,
}
fn text_format() -> String {
    "text".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreInput {
    pub target_version: i32,
    pub expected_version: i32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchInput {
    #[serde(default)]
    pub memory_identity: Option<crate::memory_identity::MemoryIdentity>,
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default = "keyword_mode")]
    pub mode: String,
    #[serde(default)]
    pub allow_partial: bool,
}
fn default_limit() -> usize {
    10
}
fn keyword_mode() -> String {
    "keyword".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveInput {
    #[serde(default)]
    pub memory_identity: Option<crate::memory_identity::MemoryIdentity>,
    pub query: String,
    #[serde(default = "default_budget")]
    pub budget_tokens: usize,
    #[serde(default = "keyword_mode")]
    pub mode: String,
    #[serde(default)]
    pub allow_partial: bool,
}
fn default_budget() -> usize {
    2000
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SearchHit {
    pub asset_id: Uuid,
    pub version: i32,
    pub chunk_id: Uuid,
    pub kind: String,
    pub title: String,
    pub content: String,
    pub locator: Value,
    pub source_event_id: Uuid,
    pub score: f64,
    #[serde(default)]
    #[sqlx(skip)]
    pub identity: Option<crate::memory_identity::MemoryIdentity>,
}

impl SearchHit {
    pub fn matches_identity(
        &self,
        filter: Option<&crate::memory_identity::MemoryIdentity>,
    ) -> bool {
        filter.is_none_or(|identity| {
            self.kind == "memory" && self.identity.as_ref() == Some(identity)
        })
    }

    pub fn normalization_status(&self) -> &'static str {
        if self.kind != "memory" {
            "not_applicable"
        } else if self.identity.is_some() {
            "explicit_identity"
        } else {
            "legacy_unidentified"
        }
    }
}

#[cfg(test)]
mod identity_status_tests {
    use super::*;

    fn identified_input() -> IdentifiedMemoryInput {
        serde_json::from_value(serde_json::json!({
            "identity":{"subject":{"kind":"service","stable_id":"billing"},
            "predicate":"release.approval","context":{}},"content":"approval"
        }))
        .unwrap()
    }

    #[test]
    fn omitted_version_preserves_legacy_idempotency_payload() {
        let input = identified_input();
        assert!(input.check_version(None).is_ok());
        assert!(input.check_version(Some(7)).is_ok());
        let value = serde_json::to_value(&input).unwrap();
        assert!(value.get("expected_version").is_none());
        let mut explicit_null = value.clone();
        explicit_null["expected_version"] = Value::Null;
        let input: IdentifiedMemoryInput = serde_json::from_value(explicit_null).unwrap();
        assert_eq!(serde_json::to_value(input).unwrap(), value);
    }

    #[test]
    fn caller_version_precondition_distinguishes_empty_and_current() {
        let mut input = identified_input();
        input.expected_version = Some(0);
        assert!(input.check_version(None).is_ok());
        assert!(matches!(
            input.check_version(Some(1)),
            Err(crate::error::AppError::Conflict(_))
        ));
        input.expected_version = Some(2);
        assert!(input.check_version(Some(2)).is_ok());
        for current in [None, Some(1), Some(3)] {
            assert!(matches!(
                input.check_version(current),
                Err(crate::error::AppError::Conflict(_))
            ));
        }
        assert_eq!(serde_json::to_value(input).unwrap()["expected_version"], 2);
    }

    #[test]
    fn negative_version_is_invalid_before_acceptance() {
        let mut input = identified_input();
        input.expected_version = Some(-1);
        assert!(matches!(
            input.validate_version(),
            Err(crate::error::AppError::Invalid(_))
        ));
        assert!(matches!(
            input.check_version(None),
            Err(crate::error::AppError::Invalid(_))
        ));
    }

    #[test]
    fn legacy_hit_decodes_and_identity_status_preserves_type() {
        let mut hit: SearchHit = serde_json::from_value(serde_json::json!({
            "asset_id":Uuid::new_v4(),"version":1,"chunk_id":Uuid::new_v4(),
            "kind":"memory","title":"policy","content":"approval",
            "locator":{},"source_event_id":Uuid::new_v4(),"score":1.0
        }))
        .unwrap();
        assert_eq!(hit.normalization_status(), "legacy_unidentified");
        assert!(hit.matches_identity(None));
        hit.identity = Some(
            serde_json::from_value(serde_json::json!({
            "subject":{"kind":"service","stable_id":"billing"},
            "predicate":"release.approval","context":{"environment":"production"}
            }))
            .unwrap(),
        );
        assert_eq!(hit.normalization_status(), "explicit_identity");
        let identity = hit.identity.clone().unwrap();
        assert!(hit.matches_identity(Some(&identity)));
        let mut different = identity.clone();
        different.subject.stable_id = "shipping".into();
        assert!(!hit.matches_identity(Some(&different)));
        different = identity.clone();
        different
            .context
            .insert("environment".into(), "staging".into());
        assert!(!hit.matches_identity(Some(&different)));
        hit.kind = "knowledge".into();
        assert_eq!(hit.normalization_status(), "not_applicable");
        assert!(!hit.matches_identity(Some(&identity)));
        hit.kind = "memory".into();
        hit.identity = None;
        assert!(!hit.matches_identity(Some(&identity)));
    }

    #[test]
    fn retrieval_identity_filter_defaults_and_schema_preserve_contract() {
        let input: SearchInput =
            serde_json::from_value(serde_json::json!({"query":"policy"})).unwrap();
        assert!(input.memory_identity.is_none());
        let input: ResolveInput =
            serde_json::from_value(serde_json::json!({"query":"policy"})).unwrap();
        assert!(input.memory_identity.is_none());
        let schema = serde_json::to_value(schemars::schema_for!(SearchInput)).unwrap();
        assert!(schema["properties"]["memory_identity"].is_object());
        assert!(
            serde_json::from_value::<SearchInput>(serde_json::json!({
            "query":"policy","memory_identity":{"subject":{"kind":"service","stable_id":"billing"},
            "predicate":"release.approval","workspace_id":"other"}
            }))
            .is_err()
        );
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkItem {
    pub tenant_id: Uuid,
    pub workspace_id: Uuid,
    pub job_id: Uuid,
    pub generation: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub content: String,
    pub locator: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entity {
    pub name: String,
    pub entity_type: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Relation {
    pub source: String,
    pub predicate: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphExtraction {
    pub entities: Vec<Entity>,
    pub relations: Vec<Relation>,
}
