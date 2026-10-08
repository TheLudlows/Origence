//! Deterministic P1 identity codec for explicit HTTP writes and SQLite asset slots.
//! A key identifies scope + business subject + predicate + explicit conditions.
//! It does not authorize access or establish semantic equivalence.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::storage::Scope;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubjectKind {
    User,
    Agent,
    Project,
    Service,
    Team,
}

impl SubjectKind {
    fn name(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
            Self::Project => "project",
            Self::Service => "service",
            Self::Team => "team",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemorySubject {
    pub kind: SubjectKind,
    /// Stable business ID, never an inferred display-name alias or API key ID.
    pub stable_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryIdentity {
    pub subject: MemorySubject,
    pub predicate: String,
    #[serde(default)]
    pub context: BTreeMap<String, String>,
}

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("invalid memory identity field: {0}")]
    Invalid(&'static str),
    #[error("memory identity encoding failed")]
    Encoding(#[from] serde_json::Error),
}

fn text_valid(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn name_valid(value: &str, max_bytes: usize) -> bool {
    text_valid(value, max_bytes)
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-".contains(&c))
        && value.split('.').all(|segment| !segment.is_empty())
}

impl MemoryIdentity {
    pub fn validate(&self) -> Result<(), IdentityError> {
        if !text_valid(&self.subject.stable_id, 256) {
            return Err(IdentityError::Invalid("subject.stable_id"));
        }
        if !name_valid(&self.predicate, 128) {
            return Err(IdentityError::Invalid("predicate"));
        }
        if self.context.len() > 32 {
            return Err(IdentityError::Invalid("context"));
        }
        for (name, value) in &self.context {
            if !name_valid(name, 64) || !text_valid(value, 256) {
                return Err(IdentityError::Invalid("context"));
            }
        }
        Ok(())
    }

    /// Scope must be supplied by the authenticated application, not the model.
    /// v1 is a JSON tuple with sorted context keys; fields retain case and Unicode.
    /// Content, principal, source and version are excluded so updates share identity.
    pub fn key(&self, scope: Scope) -> Result<String, IdentityError> {
        self.validate()?;
        let bytes = serde_json::to_vec(&(
            1_u8,
            scope.tenant_id,
            scope.workspace_id,
            self.subject.kind.name(),
            &self.subject.stable_id,
            &self.predicate,
            &self.context,
        ))?;
        Ok(format!("memory-identity:v1:{:x}", Sha256::digest(bytes)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn scope() -> Scope {
        Scope {
            tenant_id: Uuid::from_u128(1),
            workspace_id: Uuid::from_u128(2),
        }
    }

    fn identity() -> MemoryIdentity {
        MemoryIdentity {
            subject: MemorySubject {
                kind: SubjectKind::Project,
                stable_id: "Atlas".into(),
            },
            predicate: "database.engine".into(),
            context: BTreeMap::from([("env".into(), "production".into())]),
        }
    }

    #[test]
    fn subject_kind_id_predicate_and_conditions_are_distinct() {
        let base = identity();
        let key = base.key(scope()).unwrap();
        let mut other = base.clone();
        other.subject.stable_id = "Boreal".into();
        assert_ne!(key, other.key(scope()).unwrap());
        other = base.clone();
        other.subject.kind = SubjectKind::Service;
        assert_ne!(key, other.key(scope()).unwrap());
        other = base.clone();
        other.predicate = "database.version".into();
        assert_ne!(key, other.key(scope()).unwrap());
        other = base;
        other.context.insert("env".into(), "test".into());
        assert_ne!(key, other.key(scope()).unwrap());
    }

    #[test]
    fn tenant_and_workspace_are_part_of_identity() {
        let base = scope();
        let key = identity().key(base).unwrap();
        assert_ne!(
            key,
            identity()
                .key(Scope {
                    tenant_id: Uuid::from_u128(3),
                    ..base
                })
                .unwrap()
        );
        assert_ne!(
            key,
            identity()
                .key(Scope {
                    workspace_id: Uuid::from_u128(3),
                    ..base
                })
                .unwrap()
        );
    }

    #[test]
    fn json_field_order_does_not_change_key() {
        let a: MemoryIdentity = serde_json::from_str(
            r#"{"subject":{"kind":"project","stable_id":"Atlas"},"predicate":"database.engine","context":{"env":"production","region":"cn"}}"#,
        )
        .unwrap();
        let b: MemoryIdentity = serde_json::from_str(
            r#"{"context":{"region":"cn","env":"production"},"predicate":"database.engine","subject":{"stable_id":"Atlas","kind":"project"}}"#,
        )
        .unwrap();
        assert_eq!(a.key(scope()).unwrap(), b.key(scope()).unwrap());
    }

    #[test]
    fn stable_ids_preserve_case_and_unicode() {
        let a = identity();
        let mut b = a.clone();
        b.subject.stable_id = "atlas".into();
        assert_ne!(a.key(scope()).unwrap(), b.key(scope()).unwrap());
        b.subject.stable_id = "项目甲".into();
        assert!(b.key(scope()).is_ok());
    }

    #[test]
    fn invalid_fields_are_rejected_even_after_deserialization() {
        for invalid in ["", " Atlas", "Atlas\n", "A\0B"] {
            let mut value = identity();
            value.subject.stable_id = invalid.into();
            assert!(value.key(scope()).is_err());
        }
        for invalid in [
            "Database.engine",
            "database..engine",
            "database.",
            "database engine",
        ] {
            let mut value = identity();
            value.predicate = invalid.into();
            assert!(value.key(scope()).is_err());
        }
        let mut value = identity();
        value.context.insert("env".into(), "".into());
        assert!(value.key(scope()).is_err());
    }

    #[test]
    fn v1_encoding_is_frozen() {
        assert_eq!(
            identity().key(scope()).unwrap(),
            "memory-identity:v1:c1def26f51ec027bb5efb297db4e94db9993918e4e4bdf1144ccb096a9304f6c"
        );
    }

    #[test]
    fn oversized_fields_and_context_are_rejected() {
        let mut value = identity();
        value.subject.stable_id = "x".repeat(257);
        assert!(value.key(scope()).is_err());
        value = identity();
        value.context.insert("env".into(), "x".repeat(257));
        assert!(value.key(scope()).is_err());
        value = identity();
        value.context = (0..33)
            .map(|n| (format!("condition{n}"), "value".into()))
            .collect();
        assert!(value.key(scope()).is_err());
    }

    #[test]
    fn key_is_versioned_and_fits_existing_slot_limit() {
        let key = identity().key(scope()).unwrap();
        assert!(key.starts_with("memory-identity:v1:"));
        assert_eq!(key.len(), "memory-identity:v1:".len() + 64);
        assert!(key.len() < 256);
    }
}
