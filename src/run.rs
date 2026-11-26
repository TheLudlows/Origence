//! Immutable, explicitly attributed private task evidence. Missing events are unknown.
use crate::session::UsedEvidence;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RunInput {
    pub run_id: Uuid,
    /// A short, sanitized task description. No credentials or raw tool payloads.
    pub summary: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunReporter {
    Client,
    User,
    Test,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RunEvent {
    /// A client report, not proof of platform delivery or model use.
    ContextReported {
        references: Vec<UsedEvidence>,
    },
    ToolReported {
        tool: String,
        summary: String,
    },
    ResultReported {
        outcome: RunOutcome,
        summary: String,
    },
    /// A caller-observed total for the whole Run. Omitted metrics stay unknown.
    MeteringReported {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_tokens: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        estimated_cost: Option<RunCost>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        billed_cost: Option<RunCost>,
    },
    Feedback {
        outcome: RunOutcome,
        summary: String,
    },
    /// Claimed loading of an exact Skill version, not a server execution attestation.
    SkillLoadedReported {
        skill_id: Uuid,
        version: u32,
        digest: String,
    },
    /// Opaque claimed output identity. No URL, contents, or access grant.
    ArtifactReported {
        artifact_id: Uuid,
        summary: String,
    },
    /// Declares collection limits; omission is always unknown, never complete.
    Coverage {
        component: String,
        status: CoverageStatus,
        summary: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RunCost {
    /// Three-letter uppercase currency code.
    pub currency: String,
    /// Millionths of one currency unit.
    pub micros: u64,
}
impl RunCost {
    fn validate(&self) -> Result<(), &'static str> {
        if self.currency.len() != 3
            || !self.currency.bytes().all(|b| b.is_ascii_uppercase())
            || self.micros > 9_007_199_254_740_991
        {
            return Err("invalid Run cost");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    Success,
    Failure,
    Unknown,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CoverageStatus {
    Reported,
    Partial,
    NotCollected,
    Unknown,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RunEventInput {
    pub event_id: Uuid,
    /// Claimed role of the reporter; authenticated principal is stored separately.
    pub reporter: RunReporter,
    pub observed_at_ms: i64,
    pub event: RunEvent,
}
impl RunInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.run_id.is_nil() || self.summary.trim().is_empty() || self.summary.len() > 4096 {
            return Err("invalid run identity or summary");
        }
        Ok(())
    }
}
impl RunEventInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.event_id.is_nil() || self.observed_at_ms < 0 {
            return Err("invalid event identity or observation time");
        }
        match &self.event {
            RunEvent::ContextReported { references } => {
                if references.len() > 64 {
                    return Err("too many context references");
                }
                let mut seen = std::collections::HashSet::new();
                for r in references {
                    r.validate()?;
                    if !seen.insert(r) {
                        return Err("duplicate context reference");
                    }
                }
            }
            RunEvent::ToolReported { tool, summary }
            | RunEvent::Coverage {
                component: tool,
                summary,
                ..
            } => {
                if tool.trim().is_empty() || tool.len() > 200 || summary.len() > 4096 {
                    return Err("invalid event summary");
                }
            }
            RunEvent::SkillLoadedReported {
                skill_id,
                version,
                digest,
            } => {
                if skill_id.is_nil()
                    || *version == 0
                    || digest.len() != 64
                    || !digest
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    return Err("invalid reported Skill version");
                }
            }
            RunEvent::ArtifactReported {
                artifact_id,
                summary,
            } => {
                if artifact_id.is_nil() || summary.len() > 4096 {
                    return Err("invalid reported artifact");
                }
            }
            RunEvent::ResultReported { summary, .. } | RunEvent::Feedback { summary, .. } => {
                if summary.len() > 4096 {
                    return Err("invalid event summary");
                }
            }
            RunEvent::MeteringReported {
                duration_ms,
                agent_tokens,
                estimated_cost,
                billed_cost,
            } => {
                if duration_ms.is_none()
                    && agent_tokens.is_none()
                    && estimated_cost.is_none()
                    && billed_cost.is_none()
                {
                    return Err("empty Run metering report");
                }
                for cost in [estimated_cost, billed_cost].into_iter().flatten() {
                    cost.validate()?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input(event: RunEvent) -> RunEventInput {
        RunEventInput {
            event_id: Uuid::new_v4(),
            reporter: RunReporter::Client,
            observed_at_ms: 0,
            event,
        }
    }
    #[test]
    fn reported_skill_and_artifact_are_typed_bounded_claims() {
        let skill = RunEvent::SkillLoadedReported {
            skill_id: Uuid::new_v4(),
            version: 1,
            digest: "a".repeat(64),
        };
        let report = input(skill);
        assert!(report.validate().is_ok());
        assert_eq!(
            serde_json::from_value::<RunEventInput>(serde_json::to_value(&report).unwrap())
                .unwrap(),
            report
        );
        for (id, version, digest) in [
            (Uuid::nil(), 1, "a".repeat(64)),
            (Uuid::new_v4(), 0, "a".repeat(64)),
            (Uuid::new_v4(), 1, "https://example.invalid".into()),
            (Uuid::new_v4(), 1, "A".repeat(64)),
        ] {
            assert!(
                input(RunEvent::SkillLoadedReported {
                    skill_id: id,
                    version,
                    digest
                })
                .validate()
                .is_err()
            );
        }
        assert!(
            input(RunEvent::ArtifactReported {
                artifact_id: Uuid::new_v4(),
                summary: "generated report".into()
            })
            .validate()
            .is_ok()
        );
        assert!(
            input(RunEvent::ArtifactReported {
                artifact_id: Uuid::nil(),
                summary: "".into()
            })
            .validate()
            .is_err()
        );
        assert!(
            input(RunEvent::ArtifactReported {
                artifact_id: Uuid::new_v4(),
                summary: "x".repeat(4097)
            })
            .validate()
            .is_err()
        );
        assert!(serde_json::from_value::<RunEvent>(serde_json::json!({"kind":"artifact_reported","artifact_id":"https://example.invalid/secret","summary":""})).is_err());
    }
}
