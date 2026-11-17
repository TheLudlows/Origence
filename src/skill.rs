//! Reviewed text-only Skill packages. Publication grants no tool execution rights.
use crate::storage::{Scope, StorageError, StorageResult, hash};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

const MAX_TEXT: usize = 256 * 1024;
const MAX_PACKAGE: usize = 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillDraft {
    pub name: String,
    pub description: String,
    pub body: String,
    #[serde(default)]
    pub references: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SkillSource {
    Asset {
        asset_id: Uuid,
        version: i32,
        content_sha256: String,
    },
    Run {
        run_id: Uuid,
        through_sequence: i64,
        digest: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillCandidateInput {
    pub draft: SkillDraft,
    pub sources: Vec<SkillSource>,
    /// Kept private; never included in a review packet or distributed package.
    pub private_notes: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillTarget {
    pub tenant_id: Uuid,
    pub workspace_id: Uuid,
    pub skill_id: Uuid,
    pub version: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillManifest {
    pub format_version: u32,
    pub tenant_id: Uuid,
    pub workspace_id: Uuid,
    pub skill_id: Uuid,
    pub version: u32,
    pub name: String,
    pub description: String,
    pub artifact_sha256: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillPackage {
    pub manifest: SkillManifest,
    pub files: BTreeMap<String, String>,
    pub digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillReviewInput {
    pub revision: u32,
    pub reviewer_id: Uuid,
    pub target: SkillTarget,
    /// Explicitly selected, redacted explanation; not the original private sources.
    pub basis: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillReviewPacket {
    pub review_id: Uuid,
    pub revision: u32,
    pub target: SkillTarget,
    pub package: SkillPackage,
    pub basis: String,
    pub digest: String,
}

pub(crate) fn encode<T: Serialize>(value: &T) -> StorageResult<String> {
    serde_json::to_string(value)
        .map_err(|_| StorageError::Unavailable("invalid Skill record".into()))
}
#[cfg(feature = "sqlite")]
pub(crate) fn decode<T: serde::de::DeserializeOwned>(value: &str) -> StorageResult<T> {
    serde_json::from_str(value)
        .map_err(|_| StorageError::Unavailable("invalid Skill record".into()))
}
fn text(value: &str, limit: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= limit
        && !value.contains('\0')
        // Claude preprocesses inline and fenced shell placeholders before model
        // invocation. Reject even quoted examples; text-only packages never need them.
        && !value.contains("!`")
        && !value.lines().any(|line| {
            let line = line.trim_start();
            ["```", "~~~"].iter().any(|fence| {
                line.strip_prefix(fence).is_some_and(|rest|
                    rest.trim_start_matches(['`', '~']).trim_start().starts_with('!'))
            })
        })
}
fn slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && !matches!(value, "con" | "prn" | "aux" | "nul")
        && !(value.len() == 4
            && (value.starts_with("com") || value.starts_with("lpt"))
            && matches!(value.as_bytes()[3], b'1'..=b'9'))
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}
impl SkillCandidateInput {
    pub fn validate(&self) -> StorageResult<()> {
        self.draft.validate()?;
        if self.sources.is_empty() || self.sources.len() > 32 || self.private_notes.len() > MAX_TEXT
        {
            return Err(StorageError::InvalidArgument(
                "Skill requires 1..32 bounded sources",
            ));
        }
        for source in &self.sources {
            let valid = match source {
                SkillSource::Asset {
                    asset_id,
                    version,
                    content_sha256,
                } => !asset_id.is_nil() && *version > 0 && digest(content_sha256),
                SkillSource::Run {
                    run_id,
                    through_sequence,
                    digest: hash,
                } => !run_id.is_nil() && *through_sequence >= 0 && digest(hash),
            };
            if !valid {
                return Err(StorageError::InvalidArgument("invalid Skill source"));
            }
        }
        Ok(())
    }
}
impl SkillDraft {
    pub fn validate(&self) -> StorageResult<()> {
        if !slug(&self.name)
            || !text(&self.description, 1024)
            || !text(&self.body, MAX_TEXT)
            || self.references.len() > 16
        {
            return Err(StorageError::InvalidArgument(
                "invalid text-only Skill draft",
            ));
        }
        let mut bytes = self.body.len() + self.description.len();
        for (path, content) in &self.references {
            let name = path
                .strip_prefix("references/")
                .and_then(|s| s.strip_suffix(".md").or_else(|| s.strip_suffix(".txt")));
            if !name.is_some_and(slug) || !text(content, MAX_TEXT) {
                return Err(StorageError::InvalidArgument(
                    "Skill references must be bounded Markdown or text files",
                ));
            }
            bytes += content.len();
        }
        if bytes > MAX_PACKAGE {
            return Err(StorageError::InvalidArgument("Skill package is too large"));
        }
        Ok(())
    }
    pub fn package(&self, target: &SkillTarget, scope: Scope) -> StorageResult<SkillPackage> {
        self.validate()?;
        if target.tenant_id != scope.tenant_id
            || target.workspace_id != scope.workspace_id
            || target.skill_id.is_nil()
            || target.version == 0
        {
            return Err(StorageError::InvalidArgument(
                "invalid Skill publication target",
            ));
        }
        let mut files = self.references.clone();
        // JSON strings are a YAML subset. No caller-provided frontmatter or executable metadata.
        files.insert(
            "SKILL.md".into(),
            format!(
                "---\nname: {}\ndescription: {}\n---\n\n{}\n",
                encode(&self.name)?,
                encode(&self.description)?,
                self.body
            ),
        );
        let manifest = SkillManifest {
            format_version: 1,
            tenant_id: target.tenant_id,
            workspace_id: target.workspace_id,
            skill_id: target.skill_id,
            version: target.version,
            name: self.name.clone(),
            description: self.description.clone(),
            artifact_sha256: files.iter().map(|(p, c)| (p.clone(), hash(c))).collect(),
        };
        let digest = hash(encode(&(&manifest, &files))?);
        Ok(SkillPackage {
            manifest,
            files,
            digest,
        })
    }
}
impl SkillPackage {
    /// Verify untrusted delivery bytes before export. This does not establish live
    /// publication/source authority; callers must fetch through the authorized service.
    pub fn validate(&self) -> StorageResult<()> {
        let m = &self.manifest;
        if m.format_version != 1 || m.tenant_id.is_nil() || m.workspace_id.is_nil() {
            return Err(StorageError::InvalidArgument("invalid Skill manifest"));
        }
        let header = format!(
            "---\nname: {}\ndescription: {}\n---\n\n",
            encode(&m.name)?,
            encode(&m.description)?
        );
        let body = self
            .files
            .get("SKILL.md")
            .and_then(|s| s.strip_prefix(&header))
            .and_then(|s| s.strip_suffix('\n'))
            .ok_or(StorageError::InvalidArgument("invalid Skill frontmatter"))?;
        let mut references = self.files.clone();
        references.remove("SKILL.md");
        let draft = SkillDraft {
            name: m.name.clone(),
            description: m.description.clone(),
            body: body.into(),
            references,
        };
        let target = SkillTarget {
            tenant_id: m.tenant_id,
            workspace_id: m.workspace_id,
            skill_id: m.skill_id,
            version: m.version,
        };
        let expected = draft.package(
            &target,
            Scope {
                tenant_id: m.tenant_id,
                workspace_id: m.workspace_id,
            },
        )?;
        if &expected != self {
            return Err(StorageError::InvalidArgument(
                "Skill package digest or manifest mismatch",
            ));
        }
        Ok(())
    }
}
impl SkillReviewPacket {
    #[cfg(feature = "sqlite")]
    pub(crate) fn build(
        id: Uuid,
        input: &SkillReviewInput,
        package: SkillPackage,
    ) -> StorageResult<Self> {
        if input.reviewer_id.is_nil() || !text(&input.basis, MAX_TEXT) {
            return Err(StorageError::InvalidArgument(
                "invalid selected Skill review basis",
            ));
        }
        let digest = hash(encode(&(
            id,
            input.revision,
            &input.target,
            &package,
            &input.basis,
        ))?);
        Ok(Self {
            review_id: id,
            revision: input.revision,
            target: input.target.clone(),
            package,
            basis: input.basis.clone(),
            digest,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn package_is_text_only_scoped_and_digest_bound() {
        let mut draft = SkillDraft {
            name: "safe-skill".into(),
            description: "A workflow".into(),
            body: "Check the result.".into(),
            references: BTreeMap::new(),
        };
        let scope = Scope {
            tenant_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
        };
        let target = SkillTarget {
            tenant_id: scope.tenant_id,
            workspace_id: scope.workspace_id,
            skill_id: Uuid::new_v4(),
            version: 1,
        };
        let p = draft.package(&target, scope).unwrap();
        p.validate().unwrap();
        let mut tampered = p.clone();
        tampered
            .files
            .get_mut("SKILL.md")
            .unwrap()
            .push_str("changed");
        assert!(tampered.validate().is_err());
        let mut executable = p.clone();
        executable.files.insert("hooks.json".into(), "{}".into());
        assert!(executable.validate().is_err());

        assert_eq!(
            p.manifest.artifact_sha256["SKILL.md"],
            hash(&p.files["SKILL.md"])
        );
        draft.body.push_str(" Changed.");
        assert_ne!(p.digest, draft.package(&target, scope).unwrap().digest);
        for file in [
            "hooks.json",
            "references/a.sh",
            "references/../secret.txt",
            "references/a/b.md",
            "SKILL.md",
        ] {
            draft.references.insert(file.into(), "text".into());
            assert!(draft.validate().is_err());
            draft.references.clear();
        }
        for extra in ["hooks", "allowed-tools", "tools", "command", "credentials"] {
            let mut value = serde_json::to_value(&draft).unwrap();
            value[extra] = serde_json::json!("forbidden");
            assert!(serde_json::from_value::<SkillDraft>(value).is_err());
        }
        let wrong = Scope {
            workspace_id: Uuid::new_v4(),
            ..scope
        };
        assert!(draft.package(&target, wrong).is_err());
    }
    #[test]
    fn shell_preprocessing_and_device_names_are_rejected() {
        let base = SkillDraft {
            name: "text-only".into(),
            description: "A method".into(),
            body: "Read and verify.".into(),
            references: BTreeMap::new(),
        };
        for dynamic in [
            "!`printf example`",
            "Context: !`printf example`",
            "```!\nprintf example\n```",
            "  ``` !\nprintf example\n```",
            "~~~!\nprintf example\n~~~",
        ] {
            let mut draft = base.clone();
            draft.body = dynamic.into();
            assert!(draft.validate().is_err());
            draft = base.clone();
            draft.description = dynamic.into();
            assert!(draft.validate().is_err());
            draft = base.clone();
            draft
                .references
                .insert("references/example.md".into(), dynamic.into());
            assert!(draft.validate().is_err());
        }
        for name in ["con", "prn", "aux", "nul", "com1", "com9", "lpt1", "lpt9"] {
            let mut draft = base.clone();
            draft.name = name.into();
            assert!(draft.validate().is_err());
            draft = base.clone();
            draft
                .references
                .insert(format!("references/{name}.txt"), "Text".into());
            assert!(draft.validate().is_err());
        }
    }
}
