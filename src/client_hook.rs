//! Opt-in client command hooks and their separately run HTTP sender.
mod events;
mod queue;

use crate::{client::HostClient, error::AppError, run::RunInput};
use anyhow::{Context, ensure};
use events::HookInput;
use queue::{Queue, Record};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const HOOK_LIMIT: u64 = 1024 * 1024;
const BATCH_SIZE: usize = 16;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileFile {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    clients: Vec<Client>,
    project: Option<PathBuf>,
    server_url: Option<String>,
    logical_key_id: Option<Uuid>,
    key_env: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Client {
    Codex,
    ClaudeCode,
}

impl Client {
    fn namespace(self) -> &'static [u8] {
        match self {
            Self::Codex => b"codex",
            Self::ClaudeCode => b"claude-code",
        }
    }

    fn run_input(self, run_id: Uuid) -> RunInput {
        let summary = match self {
            Self::Codex => "Codex turn (hook metadata only)",
            Self::ClaudeCode => "Claude Code prompt (hook metadata only)",
        };
        RunInput {
            run_id,
            summary: summary.into(),
        }
    }
}

pub struct Profile {
    clients: Vec<Client>,
    project: PathBuf,
    server: reqwest::Url,
    logical_key: Uuid,
    key_env: String,
    queue: Queue,
}

impl Profile {
    /// A missing profile or a profile without an explicit opt-in is disabled.
    pub fn load(path: &Path) -> anyhow::Result<Option<Self>> {
        ensure!(path.is_absolute(), "hook profile path must be absolute");
        let path = match path.canonicalize() {
            Ok(path) => path,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => anyhow::bail!("cannot resolve hook profile"),
        };
        let metadata = path
            .symlink_metadata()
            .context("cannot inspect hook profile")?;
        ensure!(metadata.is_file(), "hook profile must be a file");
        let file = File::open(&path).context("cannot open hook profile")?;
        let config: ProfileFile = serde_json::from_slice(&read_bounded(file, 8192)?)
            .map_err(|_| anyhow::anyhow!("invalid hook profile"))?;
        if !config.enabled || config.clients.is_empty() {
            return Ok(None);
        }

        let project = config.project.context("hook profile requires project")?;
        ensure!(project.is_absolute(), "hook project must be absolute");
        let project = project
            .canonicalize()
            .context("cannot resolve hook project")?;
        ensure!(project.is_dir(), "hook project must be a directory");
        let parent = path.parent().context("hook profile needs a parent")?;
        ensure!(
            !parent.starts_with(&project),
            "hook profile must live outside its project"
        );
        require_private(&metadata)?;
        require_private(
            &parent
                .metadata()
                .context("cannot inspect hook profile directory")?,
        )?;

        let server = HostClient::parse_base(
            &config
                .server_url
                .context("hook profile requires server_url")?,
        )?;
        ensure!(server.path() == "/", "hook server URL must be an origin");
        let logical_key = config
            .logical_key_id
            .filter(|id| !id.is_nil())
            .context("hook profile requires a non-nil logical_key_id")?;
        let key_env = config.key_env.context("hook profile requires key_env")?;
        ensure!(
            valid_env_name(&key_env),
            "invalid hook credential environment variable"
        );
        let mut queue_name = path
            .file_name()
            .context("invalid hook profile path")?
            .to_os_string();
        queue_name.push(".queue");
        Ok(Some(Self {
            clients: config.clients,
            project,
            server,
            logical_key,
            key_env,
            queue: Queue::new(parent.join(queue_name)),
        }))
    }

    pub fn key_env(&self) -> &str {
        &self.key_env
    }

    pub fn enabled(&self, client: Client) -> bool {
        self.clients.contains(&client)
    }

    /// The existing MCP Run tools may explicitly supply this exact Run input.
    pub fn run_identity(&self, client: Client, session: &str, turn: &str) -> Option<RunInput> {
        if !self.enabled(client) {
            return None;
        }
        let session = client_id(session)?;
        let canonical_prompt;
        let turn = match client {
            Client::Codex => client_id(turn)?,
            Client::ClaudeCode => {
                canonical_prompt = Uuid::parse_str(turn).ok().filter(|id| !id.is_nil())?;
                canonical_prompt.as_bytes()
            }
        };
        let identity = digest(
            b"origence-client-hook-run-v1",
            &[
                client.namespace(),
                self.project.as_os_str().as_encoded_bytes(),
                self.server.as_str().as_bytes(),
                self.logical_key.as_bytes(),
                session,
                turn,
            ],
        );
        Some(client.run_input(derived_uuid(identity)))
    }

    /// Reads a single bounded hook payload, then queues only the selected metadata.
    pub fn enqueue(
        &self,
        client: Client,
        token: &str,
        process_cwd: &Path,
        stdin: impl Read,
    ) -> anyhow::Result<Capture> {
        if !self.enabled(client) || !self.owns_directory(process_cwd) {
            return Ok(Capture::Ignored);
        }
        ensure!(valid_token(token), "invalid hook credential");
        let body = read_bounded(stdin, HOOK_LIMIT)?;
        let input: HookInput<'_> = serde_json::from_slice(&body)
            .map_err(|_| anyhow::anyhow!("invalid client hook input"))?;
        if !input
            .cwd
            .as_deref()
            .is_some_and(|cwd| self.owns_directory(Path::new(cwd)))
        {
            return Ok(Capture::Ignored);
        }
        let Some(observation) = input.observation(client) else {
            return Ok(Capture::Ignored);
        };
        let Some(run) = self.run_identity(
            client,
            input.session_id.as_deref().unwrap_or_default(),
            input.turn_key(client).unwrap_or_default(),
        ) else {
            return Ok(Capture::MissingIdentity);
        };
        let Some(source) = observation.source_id(&input) else {
            return Ok(Capture::MissingIdentity);
        };
        let event_id = derived_uuid(digest(
            b"origence-client-hook-event-v1",
            &[run.run_id.as_bytes(), observation.event_key(&input), source],
        ));
        let observed_at_ms =
            i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
        self.queue.enqueue(Record {
            binding: self.binding(token),
            run_id: run.run_id,
            event_id,
            observed_at_ms,
            observation,
        })
    }

    fn owns_directory(&self, path: &Path) -> bool {
        if !path.is_absolute() {
            return false;
        }
        let Ok(path) = path.canonicalize() else {
            return false;
        };
        path.is_dir()
            && path.starts_with(&self.project)
            && path.ancestors().take_while(|p| *p != self.project)
                .all(|p| matches!(p.join(".git").symlink_metadata(), Err(e) if e.kind() == std::io::ErrorKind::NotFound))
    }

    fn binding(&self, token: &str) -> Uuid {
        derived_uuid(digest(
            b"origence-client-hook-queue-binding-v1",
            &[
                self.project.as_os_str().as_encoded_bytes(),
                self.server.as_str().as_bytes(),
                self.logical_key.as_bytes(),
                self.key_env.as_bytes(),
                token.as_bytes(),
            ],
        ))
    }

    /// A sender pass keeps one exclusive sender lock and releases the queue lock
    /// before each network request. An empty or mismatched queue sends nothing.
    pub async fn flush(self, token: String) -> anyhow::Result<Flush> {
        ensure!(valid_token(&token), "invalid hook credential");
        let binding = self.binding(&token);
        let queue = self.queue;
        let clients = self.clients;
        let Some(batch) = queue
            .blocking(move |queue| queue.batch(binding, &clients, BATCH_SIZE))
            .await?
        else {
            return Ok(Flush::default());
        };
        let _sender_lock = batch.sender_lock;
        let mut report = Flush {
            held: batch.held,
            ..Flush::default()
        };
        if batch.records.is_empty() {
            return Ok(report);
        }

        let client = HostClient::with_timeout(self.server, token, Duration::from_secs(5))?;
        // The credential-bound fingerprint pins the queued token. Recheck its
        // authoritative logical owner before submitting any Run or event.
        let session = client
            .call(reqwest::Method::GET, "/v1/session", None, None)
            .await;
        match session {
            Ok(value)
                if value["key_id"]
                    .as_str()
                    .and_then(|id| Uuid::parse_str(id).ok())
                    == Some(self.logical_key) => {}
            Ok(_) => {
                report.blocked = Some("logical_key_mismatch");
                return Ok(report);
            }
            Err(e) => {
                report.blocked = Some(retry_code(&e));
                return Ok(report);
            }
        }

        let mut created = HashSet::new();
        for record in batch.records {
            let run = record.observation.client().run_input(record.run_id);
            let result = send_record(&client, &mut created, &run, &record).await;
            match result {
                Ok(_) => report.sent += 1,
                Err(AppError::Conflict(_) | AppError::Invalid(_) | AppError::NotFound) => {
                    report.discarded += 1;
                }
                Err(e) => {
                    report.blocked = Some(retry_code(&e));
                    break;
                }
            }
            queue
                .blocking(move |queue| queue.acknowledge(record.event_id))
                .await?;
        }
        Ok(report)
    }
}

async fn send_record(
    client: &HostClient,
    created: &mut HashSet<Uuid>,
    run: &RunInput,
    record: &Record,
) -> crate::error::Result<()> {
    if !created.contains(&run.run_id) {
        let receipt = client.create_run(run).await?;
        if !receipt_id(&receipt["run_id"], run.run_id) {
            return Err(AppError::Unavailable("invalid Run creation receipt".into()));
        }
        created.insert(run.run_id);
    }
    let receipt = client
        .append_run_event(run.run_id, &record.report())
        .await?;
    if !receipt_id(&receipt["run_id"], run.run_id)
        || !receipt_id(&receipt["event_id"], record.event_id)
        || !receipt["sequence"].as_u64().is_some_and(|n| n > 0)
    {
        return Err(AppError::Unavailable("invalid Run append receipt".into()));
    }
    Ok(())
}

fn receipt_id(value: &serde_json::Value, expected: Uuid) -> bool {
    value.as_str().and_then(|s| Uuid::parse_str(s).ok()) == Some(expected)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Capture {
    Ignored,
    MissingIdentity,
    Duplicate,
    Queued,
    Full,
}

#[derive(Default, Debug, PartialEq, Eq, Serialize)]
pub struct Flush {
    pub sent: usize,
    pub discarded: usize,
    pub held: usize,
    pub blocked: Option<&'static str>,
}

fn retry_code(error: &AppError) -> &'static str {
    match error {
        AppError::Unauthorized | AppError::Forbidden => "authorization_denied",
        _ => "host_unavailable",
    }
}

fn client_id(id: &str) -> Option<&[u8]> {
    // Codex documents strings, including opaque tool IDs. Reject empty,
    // oversized, path-shaped and whitespace-containing values without logging them.
    (!id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.:".contains(&b)))
    .then_some(id.as_bytes())
}

fn valid_env_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_uppercase())
        && name.len() <= 128
        && bytes.all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

fn valid_token(token: &str) -> bool {
    !token.is_empty() && token.len() <= 1024 && token.bytes().all(|b| b.is_ascii_graphic())
}

fn digest(domain: &[u8], values: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    for value in values {
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value);
    }
    hasher.finalize().into()
}

fn derived_uuid(digest: [u8; 32]) -> Uuid {
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    uuid::Builder::from_custom_bytes(bytes).into_uuid()
}

fn read_bounded(input: impl Read, limit: u64) -> anyhow::Result<Vec<u8>> {
    let mut body = Vec::new();
    input
        .take(limit + 1)
        .read_to_end(&mut body)
        .context("cannot read hook data")?;
    ensure!(
        body.len() as u64 <= limit,
        "hook input or queue size limit exceeded"
    );
    Ok(body)
}

fn require_private(metadata: &fs::Metadata) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            metadata.mode() & 0o077 == 0,
            "hook profile and queue require private permissions"
        );
    }
    #[cfg(not(unix))]
    let _ = metadata; // Windows uses the caller-selected private directory's ACL.
    Ok(())
}
