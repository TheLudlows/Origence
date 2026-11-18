//! Deterministic, explicitly selected experience → editable Skill candidate.
//! No model, archive mining, source text loading, or publication happens here.
use crate::{
    skill::{SkillCandidateInput, SkillDraft, SkillSource},
    storage::{StorageError, StorageResult},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const FORMAT: &str = "origence-selected-experience-v1";

/// Exactly one fixed source plus prose the owner has explicitly cleared for disclosure.
/// Learning uses its published asset ID, exact version and content hash, never its supports.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedExperience {
    pub source: SkillSource,
    pub name: String,
    pub description: String,
    pub steps: String,
    pub selected_basis: String,
    pub disclosure_confirmed: bool,
    #[serde(default)]
    pub private_notes: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Notes {
    format: String,
    selected_basis: String,
    private_notes: String,
}

impl SelectedExperience {
    pub fn candidate(&self) -> StorageResult<SkillCandidateInput> {
        if !self.disclosure_confirmed {
            return Err(StorageError::InvalidArgument(
                "confirm disclosure of method and selected basis",
            ));
        }
        // Apply the same bounded text-only validation to the selected review prose.
        let draft = SkillDraft {
            name: self.name.clone(),
            description: self.description.clone(),
            body: self.steps.clone(),
            references: BTreeMap::new(),
        };
        let mut basis_check = draft.clone();
        basis_check.body = self.selected_basis.clone();
        basis_check.validate()?;
        let candidate = SkillCandidateInput {
            draft,
            sources: vec![self.source.clone()],
            // Binding basis to the immutable revision makes basis-only edits invalidate approval.
            // This envelope is never included in a review or distributed package.
            private_notes: crate::skill::encode(&Notes {
                format: FORMAT.into(),
                selected_basis: self.selected_basis.clone(),
                private_notes: self.private_notes.clone(),
            })?,
        };
        candidate.validate()?;
        Ok(candidate)
    }
}

/// Returns only explicitly selected prose, never private notes or original source contents.
/// Legacy manually authored candidates continue to use their explicit review input.
pub fn selected_basis(candidate: &SkillCandidateInput) -> StorageResult<Option<String>> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&candidate.private_notes) else {
        return Ok(None);
    };
    if value["format"].as_str() != Some(FORMAT) {
        return Ok(None);
    }
    let notes: Notes = serde_json::from_value(value)
        .map_err(|_| StorageError::InvalidArgument("invalid selected experience notes"))?;
    Ok(Some(notes.selected_basis))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requires_explicit_disclosure_and_binds_basis_without_exporting_it() {
        let mut selection = SelectedExperience {
            source: SkillSource::Run {
                run_id: uuid::Uuid::new_v4(),
                through_sequence: 2,
                digest: "a".repeat(64),
            },
            name: "check-results".into(),
            description: "Check results".into(),
            steps: "Verify outputs independently.".into(),
            selected_basis: "Redacted observation".into(),
            disclosure_confirmed: false,
            private_notes: "PRIVATE support".into(),
        };
        assert!(selection.candidate().is_err());
        selection.disclosure_confirmed = true;
        let original = selection.candidate().unwrap();
        assert_eq!(original, selection.candidate().unwrap());
        assert_eq!(
            selected_basis(&original).unwrap().unwrap(),
            "Redacted observation"
        );
        assert!(
            !serde_json::to_string(&original.draft)
                .unwrap()
                .contains("PRIVATE")
        );
        selection.selected_basis = "Changed evidence".into();
        assert_ne!(original, selection.candidate().unwrap());
        selection.steps = "!`execute`".into();
        assert!(selection.candidate().is_err());
    }
}
