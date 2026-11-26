//! Dispatch the local uninstall command to the installer that recorded the rollback journal.
use anyhow::{Context, ensure};
use serde::Deserialize;
use std::{io::ErrorKind, path::PathBuf};
use tokio::{fs::File, io::AsyncReadExt, process::Command};

const MAX_STATE_BYTES: u64 = 1024 * 1024;
const SCRIPT_FALLBACK: &str = "from the Origence checkout run node tools/install.mjs --uninstall";

#[derive(Deserialize)]
struct InstallerState {
    version: u32,
    uninstaller: Option<Uninstaller>,
}

#[derive(Deserialize)]
struct Uninstaller {
    node: PathBuf,
    script: PathBuf,
}

fn state_path() -> anyhow::Result<PathBuf> {
    let home = || std::env::home_dir().context("Cannot locate the current user's home directory");
    let directory = if cfg!(windows) {
        let local = std::env::var_os("LOCALAPPDATA").filter(|path| !path.is_empty());
        let local = match local {
            Some(path) => PathBuf::from(path),
            None => home()?.join("AppData").join("Local"),
        };
        local.join("Origence")
    } else {
        home()?.join(".origence")
    };
    ensure!(
        directory.is_absolute(),
        "The installer state needs an absolute user directory"
    );
    Ok(directory.join("installer").join("state.json"))
}

pub async fn uninstall() -> anyhow::Result<()> {
    let file = match File::open(state_path()?).await {
        Err(error) if error.kind() == ErrorKind::NotFound => {
            println!(
                "{}",
                serde_json::json!({
                    "step": "uninstall",
                    "status": "skipped",
                    "detail": "No installer settings were found."
                })
            );
            return Ok(());
        }
        result => result.context("Cannot open the local Origence installer state")?,
    };
    let mut bytes = Vec::new();
    file.take(MAX_STATE_BYTES + 1)
        .read_to_end(&mut bytes)
        .await
        .context("Cannot read the local Origence installer state")?;
    ensure!(
        bytes.len() as u64 <= MAX_STATE_BYTES,
        "The installer state exceeds its size limit"
    );
    let state: InstallerState =
        serde_json::from_slice(&bytes).context("Invalid local Origence installer state")?;
    ensure!(state.version == 1, "Unsupported Origence installer state");
    let uninstaller = state
        .uninstaller
        .with_context(|| format!("This installation predates the shortcut; {SCRIPT_FALLBACK}"))?;
    ensure!(
        uninstaller.node.is_absolute() && uninstaller.script.is_absolute(),
        "Invalid uninstall command in the local installer state"
    );
    ensure!(
        tokio::fs::metadata(&uninstaller.script)
            .await
            .is_ok_and(|file| file.is_file()),
        "The recorded installer script is unavailable; {SCRIPT_FALLBACK}"
    );
    // Node owns platform edits, rollback, and service shutdown. Run that same
    // entry point with its absolute paths instead of looking in the caller's cwd.
    let status = Command::new(uninstaller.node)
        .arg(uninstaller.script)
        .arg("--uninstall")
        .env_remove("OC_API_KEY")
        .env_remove("ORIGENCE_DEMO_KEY")
        .status()
        .await
        .with_context(|| format!("Cannot start the recorded Node runtime; {SCRIPT_FALLBACK}"))?;
    ensure!(
        status.success(),
        "Origence uninstallation failed ({status})"
    );
    Ok(())
}
