//! Owner-transcribed assessments; never platform verification or access grants.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationConclusion {
    Passed,
    Failed,
    Inconclusive,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReportedSkillVersion {
    pub skill_id: Uuid,
    pub version: u32,
    pub digest: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RunVerificationInput {
    pub verification_id: Uuid,
    pub through_sequence: i64,
    pub prefix_digest: String,
    pub verifier_label: String,
    pub assessed_at_ms: i64,
    pub conclusion: VerificationConclusion,
    pub basis_summary: String,
    pub basis_sequences: Vec<i64>,
    pub reported_skill_version: Option<ReportedSkillVersion>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WithdrawRunVerificationInput {
    pub reason: String,
}
impl RunVerificationInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.verification_id.is_nil()
            || !(1..=10000).contains(&self.through_sequence)
            || !crate::retrieval_diagnostics::digest(&self.prefix_digest)
            || self.verifier_label.trim().is_empty()
            || self.verifier_label.len() > 200
            || self.assessed_at_ms < 0
            || self.basis_summary.trim().is_empty()
            || self.basis_summary.len() > 4096
            || self.basis_sequences.is_empty()
            || self.basis_sequences.len() > 64
        {
            return Err("invalid Run verification");
        }
        let mut seen = std::collections::HashSet::new();
        if self
            .basis_sequences
            .iter()
            .any(|s| *s < 1 || *s > self.through_sequence || !seen.insert(*s))
        {
            return Err("invalid verification basis sequences");
        }
        if let Some(v) = &self.reported_skill_version
            && (v.skill_id.is_nil()
                || v.version == 0
                || !crate::retrieval_diagnostics::digest(&v.digest))
        {
            return Err("invalid reported Skill version");
        }
        Ok(())
    }
}
impl WithdrawRunVerificationInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.reason.trim().is_empty() || self.reason.len() > 4096 {
            return Err("invalid withdrawal reason");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verification_dto_rejects_role_forgery_and_inexact_evidence() {
        let value = serde_json::json!({"verification_id":Uuid::new_v4(),"through_sequence":2,
            "prefix_digest":"a".repeat(64),"verifier_label":"external test","assessed_at_ms":0,
            "conclusion":"inconclusive","basis_summary":"synthetic basis","basis_sequences":[1,2],
            "reported_skill_version":{"skill_id":Uuid::new_v4(),"version":2,"digest":"b".repeat(64)}});
        let input: RunVerificationInput = serde_json::from_value(value.clone()).unwrap();
        input.validate().unwrap();
        for field in [
            "reporter",
            "owner_key_id",
            "recorder_principal_id",
            "recorded_at_ms",
            "platform_verification",
        ] {
            let mut forged = value.clone();
            forged[field] = serde_json::json!("test");
            assert!(serde_json::from_value::<RunVerificationInput>(forged).is_err());
        }
        for (field, invalid) in [
            ("basis_sequences", serde_json::json!([1, 1])),
            ("basis_sequences", serde_json::json!([3])),
            ("basis_sequences", serde_json::json!([])),
            ("prefix_digest", serde_json::json!("A".repeat(64))),
            ("through_sequence", serde_json::json!(0)),
            ("basis_summary", serde_json::json!(" ")),
            ("assessed_at_ms", serde_json::json!(-1)),
        ] {
            let mut bad = value.clone();
            bad[field] = invalid;
            assert!(
                serde_json::from_value::<RunVerificationInput>(bad)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        let mut bad = input.clone();
        bad.reported_skill_version.as_mut().unwrap().version = 0;
        assert!(bad.validate().is_err());
    }
}
