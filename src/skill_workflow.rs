//! Standalone authenticated CLI workflow over existing Skill authority endpoints.
use crate::{
    client::HostClient,
    error::{AppError, Result},
    skill::{SkillCandidateInput, SkillReviewInput, SkillTarget},
    skill_candidate::{SelectedExperience, selected_basis},
};
use clap::{Parser, Subcommand};
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{io::Read, path::PathBuf};
use uuid::Uuid;

#[derive(Parser)]
#[command(about = "Explicit selected-experience Skill drafting and review; never auto-publishes")]
pub struct SkillCli {
    #[arg(
        long,
        env = "OC_SERVER_URL",
        default_value = "http://127.0.0.1:8080",
        global = true
    )]
    server_url: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Read an owner-only fixed Run prefix address; does not read or disclose its prose.
    RunSource { run_id: Uuid, through_sequence: i64 },
    /// Read an exact asset/Learning address; no implicit latest version.
    AssetSource { asset_id: Uuid, version: i32 },
    /// Save deterministic prefill or a human edit from SelectedExperience JSON. Zero creates.
    Save {
        candidate_id: Uuid,
        #[arg(long)]
        expected_revision: u32,
        #[arg(long)]
        input: PathBuf,
    },
    /// Owner-only editable candidate, including private notes. Keep output private.
    Candidate { candidate_id: Uuid },
    /// Submit the disclosed basis and package for review by the administrator.
    Submit {
        candidate_id: Uuid,
        review_id: Uuid,
        #[arg(long)]
        revision: u32,
        #[arg(long)]
        tenant_id: Uuid,
        #[arg(long)]
        workspace_id: Uuid,
        #[arg(long)]
        skill_id: Uuid,
        #[arg(long)]
        version: u32,
    },
    /// Read the selected packet and its exact digest before deciding.
    Review { review_id: Uuid },
    /// Explicitly publish the exact approved packet, subject to live permission/source checks.
    Publish { review_id: Uuid, digest: String },
    /// Fetch an exact online package; source/Skill withdrawal denies subsequent fetches.
    Package { skill_id: Uuid, version: u32 },
    /// Withdraw one online version. Does not remove exported offline copies.
    Withdraw { skill_id: Uuid, version: u32 },
}

impl SkillCli {
    pub async fn run(self) -> Result<Value> {
        let token = std::env::var("OC_API_KEY").map_err(|_| AppError::Unauthorized)?;
        let client = HostClient::new(&self.server_url, token)?;
        match self.command {
            Command::RunSource {
                run_id,
                through_sequence,
            } => {
                let mut value = client
                    .call(
                        Method::GET,
                        &format!("/v1/runs/{run_id}/prefix?through_sequence={through_sequence}"),
                        None,
                        None,
                    )
                    .await?;
                value["kind"] = json!("run");
                Ok(value)
            }
            Command::AssetSource { asset_id, version } => {
                if version <= 0 {
                    return Err(AppError::Invalid("positive exact version required".into()));
                }
                let value = client
                    .call(
                        Method::GET,
                        &format!("/v1/assets/{asset_id}?version={version}"),
                        None,
                        None,
                    )
                    .await?;
                let digest = value["content_hash"]
                    .as_str()
                    .ok_or_else(|| AppError::Unavailable("missing asset digest".into()))?;
                Ok(
                    json!({"kind":"asset","asset_id":asset_id,"version":version,"content_sha256":digest}),
                )
            }
            Command::Save {
                candidate_id,
                expected_revision,
                input,
            } => {
                let selection: SelectedExperience = read_input(input).await?;
                let candidate = selection.candidate()?;
                client
                    .call(
                        Method::PUT,
                        &format!("/v1/skill-candidates/{candidate_id}"),
                        Some(&json!({"expected_revision":expected_revision,"candidate":candidate})),
                        None,
                    )
                    .await
            }
            Command::Candidate { candidate_id } => {
                client
                    .call(
                        Method::GET,
                        &format!("/v1/skill-candidates/{candidate_id}"),
                        None,
                        None,
                    )
                    .await
            }
            Command::Submit {
                candidate_id,
                review_id,
                revision,
                tenant_id,
                workspace_id,
                skill_id,
                version,
            } => {
                let current = client
                    .call(
                        Method::GET,
                        &format!("/v1/skill-candidates/{candidate_id}"),
                        None,
                        None,
                    )
                    .await?;
                if current["revision"].as_u64() != Some(u64::from(revision)) {
                    return Err(AppError::Conflict("Skill revision changed".into()));
                }
                let candidate: SkillCandidateInput =
                    serde_json::from_value(current["input"].clone())
                        .map_err(|_| AppError::Unavailable("invalid candidate response".into()))?;
                let basis = selected_basis(&candidate)?.ok_or_else(|| AppError::Invalid("candidate has no confirmed selected basis; save an explicit selection first".into()))?;
                let input = SkillReviewInput {
                    revision,
                    target: SkillTarget {
                        tenant_id,
                        workspace_id,
                        skill_id,
                        version,
                    },
                    basis,
                };
                client
                    .call(
                        Method::POST,
                        &format!("/v1/skill-candidates/{candidate_id}/reviews/{review_id}"),
                        Some(&json!(input)),
                        None,
                    )
                    .await
            }
            Command::Review { review_id } => {
                client
                    .call(
                        Method::GET,
                        &format!("/v1/skill-reviews/{review_id}"),
                        None,
                        None,
                    )
                    .await
            }
            Command::Publish { review_id, digest } => {
                client
                    .call(
                        Method::POST,
                        &format!("/v1/skill-reviews/{review_id}/publish"),
                        Some(&json!({"digest":digest})),
                        None,
                    )
                    .await
            }
            Command::Package { skill_id, version } => {
                client
                    .call(
                        Method::GET,
                        &format!("/v1/skills/{skill_id}/versions/{version}/package"),
                        None,
                        None,
                    )
                    .await
            }
            Command::Withdraw { skill_id, version } => {
                client
                    .call(
                        Method::DELETE,
                        &format!("/v1/skills/{skill_id}/versions/{version}"),
                        None,
                        None,
                    )
                    .await
            }
        }
    }
}

async fn read_input<T: DeserializeOwned + Send + 'static>(path: PathBuf) -> Result<T> {
    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(path)
            .map_err(|_| AppError::Invalid("cannot read selection file".into()))?;
        // Bound actual reads, not metadata. No raw diagnostics/content/paths in errors.
        let mut bytes = Vec::new();
        file.take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| AppError::Invalid("cannot read selection file".into()))?;
        if bytes.len() > 1024 * 1024 {
            return Err(AppError::Invalid("selection file exceeds 1 MiB".into()));
        }
        serde_json::from_slice(&bytes)
            .map_err(|_| AppError::Invalid("invalid selected-experience JSON".into()))
    })
    .await
    .map_err(|_| AppError::Unavailable("selection read failed".into()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn selection_read_bounds_actual_bytes_and_sanitizes_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("PRIVATE-path.json");
        tokio::fs::write(&path, b"PRIVATE invalid JSON")
            .await
            .unwrap();
        let error = read_input::<SelectedExperience>(path.clone())
            .await
            .unwrap_err();
        assert!(!error.safe_message().contains("PRIVATE"));
        tokio::fs::write(&path, vec![b' '; 1024 * 1024 + 1])
            .await
            .unwrap();
        assert!(
            matches!(read_input::<SelectedExperience>(path).await, Err(AppError::Invalid(message)) if message == "selection file exceeds 1 MiB")
        );
    }
}
