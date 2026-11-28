//! Installed hooks use the same scope as MCP and retry queued metadata on later events.
use super::{
    Capture, Client, HOOK_LIMIT, HookInput, Profile, derived_uuid, digest, read_bounded,
    valid_token,
};
use crate::{client::HostClient, error::AppError, types::AuthContext};
use anyhow::{Context, ensure};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use uuid::Uuid;

const BUDGET: Duration = Duration::from_secs(2);
const KEY_ENV: &str = "OC_API_KEY";

#[derive(Default)]
pub struct PluginHookReport {
    pub run_delivery_failed: bool,
    pub turn_sync_notice: Option<&'static str>,
}

/// A plugin invocation uses the MCP launch environment and the client's writable
/// plugin data directory. It never accepts an origin, credential or scope from stdin.
pub async fn capture_plugin_event(
    client: Client,
    origin: &str,
) -> anyhow::Result<PluginHookReport> {
    let run_enabled = std::env::var_os("OC_PLUGIN_RUN_HOOK").is_none_or(|value| value != "0");
    let archive = std::env::var_os("OC_TURN_SYNC_PROJECT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    if !run_enabled && archive.is_none() {
        return Ok(PluginHookReport::default());
    }
    let Some(token) = std::env::var(KEY_ENV)
        .ok()
        .filter(|value| valid_token(value))
    else {
        return Ok(PluginHookReport::default());
    };
    let workspace = std::env::var_os("OC_WORKSPACE_ID")
        .map(|value| {
            value
                .to_str()
                .and_then(|value| value.parse::<Uuid>().ok())
                .context("invalid OC_WORKSPACE_ID")
        })
        .transpose()?;
    let data_var = match client {
        Client::Codex => "PLUGIN_DATA",
        Client::ClaudeCode => "CLAUDE_PLUGIN_DATA",
    };
    let data =
        PathBuf::from(std::env::var_os(data_var).context("client did not provide plugin storage")?);
    let payload: Arc<[u8]> =
        tokio::task::spawn_blocking(|| read_bounded(std::io::stdin().lock(), HOOK_LIMIT))
            .await??
            .into();
    let run = async {
        if run_enabled {
            return tokio::time::timeout(
                BUDGET,
                record(
                    client,
                    origin,
                    workspace,
                    token.clone(),
                    data.clone(),
                    payload.clone(),
                ),
            )
            .await
            .context("hook delivery deadline elapsed")?;
        }
        Ok(())
    };
    let sync = async {
        match archive {
            Some(project) => {
                super::turn_sync::sync_plugin_turn(
                    client,
                    origin,
                    workspace,
                    token.clone(),
                    data.clone(),
                    project,
                    payload.clone(),
                )
                .await
            }
            None => Ok(None),
        }
    };
    let (run, sync) = tokio::join!(run, sync);
    Ok(PluginHookReport {
        run_delivery_failed: run.is_err(),
        turn_sync_notice: sync.unwrap_or(Some("synchronization could not finish")),
    })
}

async fn record(
    client: Client,
    origin: &str,
    workspace: Option<Uuid>,
    token: String,
    data: PathBuf,
    payload: Arc<[u8]>,
) -> anyhow::Result<()> {
    let (project, payload) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let cwd = std::env::current_dir()?.canonicalize()?;
        Ok((project_root(&cwd).to_path_buf(), payload))
    })
    .await??;
    let input: HookInput<'_> =
        serde_json::from_slice(&payload).context("invalid client hook input")?;
    let Some(observation) = input.observation(client) else {
        return Ok(());
    };
    if input.turn_key(client).is_none()
        || input.session_id.is_none()
        || observation.source_id(&input).is_none()
    {
        return Ok(());
    }

    let base = HostClient::parse_base(origin)?;
    ensure!(base.path() == "/", "hook server URL must be an origin");
    let host = HostClient::with_timeout(base.clone(), token.clone(), BUDGET)?;
    let host = match workspace {
        Some(id) => host.with_workspace(id),
        None => host,
    };
    let identity = match host
        .call(reqwest::Method::GET, "/v1/whoami", None, None)
        .await
    {
        Ok(value) => {
            serde_json::from_value::<AuthContext>(value).context("invalid hook identity")?
        }
        Err(AppError::Unauthorized | AppError::Forbidden) => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if identity.require("write").is_err() {
        return Ok(());
    }
    let actual = identity.workspace_id;
    ensure!(
        !actual.is_nil() && workspace.is_none_or(|id| id == actual),
        "host did not confirm the hook workspace"
    );
    let host = host.with_workspace(actual);
    let session = host
        .call(reqwest::Method::GET, "/v1/session", None, None)
        .await?;
    ensure!(
        id(&session["workspace_id"]) == Some(actual),
        "host did not confirm the hook session workspace"
    );
    let logical_key = id(&session["key_id"]).context("missing hook logical key")?;

    let queued = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let queue = plugin_queue(&data, &project, base.as_str(), actual, logical_key)?;
        let profile = Profile {
            clients: vec![client],
            project,
            server: base,
            logical_key,
            workspace: Some(actual),
            key_env: KEY_ENV.into(),
            queue,
        };
        let input = serde_json::from_slice(&payload).context("invalid client hook input")?;
        match profile.enqueue_input(client, &token, &input)? {
            Capture::Queued | Capture::Duplicate | Capture::Full => {
                let binding = profile.binding(&token);
                Ok(Some((profile, binding)))
            }
            _ => Ok(None),
        }
    })
    .await??;
    if let Some((profile, binding)) = queued
        && let Some(batch) = profile.batch(binding).await?
    {
        let report = profile
            .deliver(&host, batch, super::Flush::default())
            .await?;
        ensure!(report.blocked.is_none(), "hook delivery deferred");
    }
    Ok(())
}

pub(super) fn project_root(cwd: &Path) -> &Path {
    cwd.ancestors()
        .find(|path| {
            let marker = path.join(".git");
            marker.is_file() || marker.join("HEAD").is_file()
        })
        .unwrap_or(cwd)
}

fn plugin_queue(
    data: &Path,
    project: &Path,
    origin: &str,
    workspace: Uuid,
    key: Uuid,
) -> anyhow::Result<super::Queue> {
    let directory = private_plugin_storage(data, project)?;
    let scope = derived_uuid(digest(
        b"origence-plugin-hook-queue-v1",
        [
            project.as_os_str().as_encoded_bytes(),
            origin.as_bytes(),
            workspace.as_bytes(),
            key.as_bytes(),
        ],
    ));
    Ok(super::Queue::new(
        directory.join(format!("run-{scope}.queue")),
    ))
}

pub(super) fn private_plugin_storage(data: &Path, project: &Path) -> anyhow::Result<PathBuf> {
    ensure!(data.is_absolute(), "plugin storage path must be absolute");
    let parent = data
        .ancestors()
        .find(|path| path.symlink_metadata().is_ok())
        .context("cannot resolve client plugin storage")?
        .canonicalize()?;
    ensure!(
        !parent.starts_with(project),
        "plugin storage must be outside the project"
    );
    let mut create = fs::DirBuilder::new();
    create.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        create.mode(0o700);
    }
    create
        .create(data)
        .context("cannot create client plugin storage")?;
    let directory = data
        .canonicalize()
        .context("cannot resolve client plugin storage")?;
    ensure!(
        directory.is_dir() && !directory.starts_with(project),
        "plugin storage must be outside the project"
    );
    Ok(directory)
}

pub(super) fn id(value: &Value) -> Option<Uuid> {
    value
        .as_str()?
        .parse::<Uuid>()
        .ok()
        .filter(|id| !id.is_nil())
}
