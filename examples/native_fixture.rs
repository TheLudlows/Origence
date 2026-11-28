//! Synthetic data and adapter probes for repository acceptance scripts.
use anyhow::Context;
use origence::{
    archive_maintenance,
    client::HostClient,
    client_skill::{SkillClient, export_skill},
    models::Models,
    storage::{Scope, anyseek::AnySeekConfig},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{io::Read, path::PathBuf};
use uuid::Uuid;

#[path = "../tests/support/fixture_workspace.rs"]
mod fixture_workspace;

#[derive(Deserialize)]
#[serde(tag = "fixture", rename_all = "snake_case")]
enum Fixture {
    MemberWorkspace {
        name: String,
        same_tenant_as: Option<Uuid>,
    },
    ArchiveManifest {
        embedded: bool,
    },
    InstallSkill {
        client: String,
        project: PathBuf,
        tenant: Uuid,
        workspace: Uuid,
        skill: Uuid,
        version: u32,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut input = Vec::new();
    std::io::stdin()
        .lock()
        .take(16385)
        .read_to_end(&mut input)?;
    anyhow::ensure!(input.len() <= 16384, "fixture input too large");
    println!("{}", run(serde_json::from_slice(&input)?).await?);
    Ok(())
}

async fn run(fixture: Fixture) -> anyhow::Result<Value> {
    match fixture {
        Fixture::MemberWorkspace {
            name,
            same_tenant_as,
        } => {
            let dir = PathBuf::from(
                std::env::var_os("OC_DATA_DIR").context("missing fixture directory")?,
            );
            fixture_workspace::seed_member_workspace(&dir, &name, same_tenant_as).await
        }
        Fixture::ArchiveManifest { embedded } => {
            let dir = PathBuf::from(
                std::env::var_os("OC_DATA_DIR").context("missing fixture directory")?,
            );
            let models = Models::from_env()?;
            if embedded {
                #[cfg(feature = "embedded-anyseek")]
                return Ok(archive_maintenance::run_embedded(&dir, models).await?);
                #[cfg(not(feature = "embedded-anyseek"))]
                anyhow::bail!("embedded fixture support is unavailable");
            }
            Ok(archive_maintenance::run(&dir, models, AnySeekConfig::from_env()?).await?)
        }
        Fixture::InstallSkill {
            client,
            project,
            tenant,
            workspace,
            skill,
            version,
        } => {
            let target = match client.as_str() {
                "codex" => SkillClient::Codex,
                "claude-code" => SkillClient::ClaudeCode,
                _ => anyhow::bail!("invalid fixture client"),
            };
            let host = HostClient::new(
                &std::env::var("OC_SERVER_URL")?,
                std::env::var("OC_API_KEY")?,
            )?;
            let scope = Scope {
                tenant_id: tenant,
                workspace_id: workspace,
            };
            let path = export_skill(&host, target, &project, scope, skill, version).await?;
            Ok(json!({"skill_id":skill,"version":version,"path":path,"exported":true}))
        }
    }
}
