//! Synthetic data and adapter probes for repository acceptance scripts.
use anyhow::Context;
use origence::{archive_maintenance, models::Models, storage::anyseek::AnySeekConfig};
use serde::Deserialize;
use serde_json::Value;
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
    }
}
