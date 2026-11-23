use crate::storage::{
    anyseek::{AnySeekConfig, AnySeekFullTextStore},
    retrieval_store::binding,
};
use anyhow::{Context, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    fs,
    net::TcpListener,
    process::{Child, Command},
    time,
};
use uuid::Uuid;

/// Configuration for the local-disk AnySeek process. Its endpoint stays stable
/// across restarts so Origence can enforce its existing backend binding.
pub struct LocalAnySeek {
    pub binary: PathBuf,
    pub data_dir: PathBuf,
    pub port: u16,
}

impl LocalAnySeek {
    pub(super) async fn start(self, origence_dir: &Path) -> anyhow::Result<(Child, AnySeekConfig)> {
        ensure!(
            self.port != 0,
            "local AnySeek requires a stable, nonzero port"
        );
        let address = (std::net::Ipv4Addr::LOCALHOST, self.port);
        // Never attach to a pre-existing listener. The one-time key also
        // prevents another service from satisfying the authenticated probe.
        drop(
            TcpListener::bind(address)
                .await
                .context("local AnySeek loopback port is unavailable")?,
        );
        fs::create_dir_all(origence_dir)
            .await
            .context("creating the Origence data directory")?;
        let index_path = std::path::absolute(self.data_dir)?;
        match fs::canonicalize(&index_path).await {
            Ok(existing) => ensure!(
                existing != fs::canonicalize(origence_dir).await?,
                "AnySeek and Origence require distinct data directories"
            ),
            Err(error) if error.kind() == ErrorKind::NotFound => ensure!(
                !fs::try_exists(origence_dir.join(binding::FILE)).await?,
                "local AnySeek data directory is missing for the existing Origence backend binding"
            ),
            Err(error) => return Err(error).context("opening the local AnySeek data directory"),
        }
        let path = index_path
            .to_str()
            .filter(|path| path.trim() == *path)
            .context("local AnySeek data path requires UTF-8 and no trailing whitespace")?;
        let secret = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let key = STANDARD.encode(Sha256::digest(secret.as_bytes()));
        let config = AnySeekConfig::new(&format!("http://127.0.0.1:{}", self.port), &key)?;

        let mut command = Command::new(self.binary);
        command
            .arg("local_ip=127.0.0.1")
            .arg(format!("local_port={}", self.port))
            .arg(format!("index_path={path}"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        for (name, _) in std::env::vars_os() {
            let name_upper = name.to_string_lossy().to_ascii_uppercase();
            if ["OC_", "ORIGENCE_", "ANYSEEK_"]
                .iter()
                .any(|prefix| name_upper.starts_with(prefix))
            {
                command.env_remove(name);
            }
        }
        command
            .env("MASTER_KEY_SECRET", secret)
            .env("ANYSEEK_STORAGE_BACKEND", "local");
        // Keep a terminal interrupt from stopping AnySeek before Origence has
        // finished its current publication and stopped the worker.
        #[cfg(unix)]
        command.process_group(0);
        #[cfg(windows)]
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP);

        let child = command
            .spawn()
            .context("starting local AnySeek; check --anyseek-bin or put anyseek_server on PATH")?;
        Ok((child, config))
    }
}

pub(super) async fn wait_ready(config: AnySeekConfig) -> anyhow::Result<()> {
    let store = AnySeekFullTextStore::new(config)?;
    time::timeout(Duration::from_secs(60), async {
        loop {
            if matches!(
                time::timeout(Duration::from_secs(2), store.check()).await,
                Ok(Ok(()))
            ) {
                return;
            }
            time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .context("local AnySeek authenticated readiness timed out")
}

pub(super) async fn stop(child: &mut Child) -> anyhow::Result<()> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;

        let pid = child.id().context("local AnySeek process ID unavailable")? as libc::pid_t;
        // SAFETY: this PID belongs to our unreaped child, which cannot be reused.
        if unsafe { libc::kill(pid, libc::SIGTERM) } == 0
            && let Ok(status) = time::timeout(Duration::from_secs(30), child.wait()).await
        {
            let status = status?;
            // A startup cancellation can arrive before AnySeek installs its handler.
            ensure!(
                status.success() || status.signal() == Some(libc::SIGTERM),
                "local AnySeek failed to shut down cleanly"
            );
            return Ok(());
        }
        tracing::warn!("local AnySeek did not exit after SIGTERM; terminating the owned child");
    }
    // The pinned AnySeek Windows binary handles console-wide Ctrl+C only.
    // Terminate the owned process after the Origence worker has drained.
    child.kill().await.context("terminating local AnySeek")
}
