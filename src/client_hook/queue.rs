//! A private bounded spool. Only the short local transactions take the queue lock.
use super::{Capture, Client, events::Observation, read_bounded, require_private};
use crate::run::RunEventInput;
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use uuid::Uuid;

const PENDING_LIMIT: usize = 256;
const RECEIPT_LIMIT: usize = 1024;
const STATE_LIMIT: u64 = 256 * 1024;
const LOCK_WAIT: Duration = Duration::from_millis(75);

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    pub binding: Uuid,
    pub run_id: Uuid,
    pub event_id: Uuid,
    pub observed_at_ms: i64,
    pub observation: Observation,
}

impl Record {
    pub fn report(&self) -> RunEventInput {
        self.observation.report(self.event_id, self.observed_at_ms)
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct State {
    pending: VecDeque<Record>,
    acknowledged: VecDeque<Uuid>,
}

pub(super) struct Batch {
    pub sender_lock: File,
    pub records: Vec<Record>,
    pub held: usize,
}

#[derive(Clone)]
pub(super) struct Queue(PathBuf);

impl Queue {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    pub async fn blocking<T: Send + 'static>(
        &self,
        f: impl FnOnce(Self) -> anyhow::Result<T> + Send + 'static,
    ) -> anyhow::Result<T> {
        let queue = self.clone();
        tokio::task::spawn_blocking(move || f(queue)).await?
    }

    pub fn enqueue(&self, record: Record) -> anyhow::Result<Capture> {
        self.prepare_directory()?;
        let _lock = self.lock()?;
        let mut state = self.read()?;
        if state.pending.iter().any(|r| r.event_id == record.event_id)
            || state.acknowledged.contains(&record.event_id)
        {
            return Ok(Capture::Duplicate);
        }
        if state.pending.len() == PENDING_LIMIT {
            return Ok(Capture::Full);
        }
        state.pending.push_back(record);
        self.write(&state)?;
        Ok(Capture::Queued)
    }

    pub fn batch(
        &self,
        binding: Uuid,
        clients: &[Client],
        limit: usize,
    ) -> anyhow::Result<Option<Batch>> {
        match self.0.symlink_metadata() {
            Ok(metadata) => self.check_directory(&metadata)?,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(_) => anyhow::bail!("cannot inspect hook queue"),
        }
        let sender_lock = open_private(&self.0.join("sender.lock"))?;
        match sender_lock.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => return Ok(None),
            Err(_) => anyhow::bail!("cannot lock hook sender"),
        }
        let _lock = self.lock()?;
        let state = self.read()?;
        let mut records = Vec::new();
        let mut held = 0;
        for record in state.pending {
            if record.binding != binding || !clients.contains(&record.observation.client()) {
                held += 1;
            } else if records.len() < limit {
                records.push(record);
            }
        }
        Ok(Some(Batch {
            sender_lock,
            records,
            held,
        }))
    }

    pub fn acknowledge(&self, event_id: Uuid) -> anyhow::Result<()> {
        let _lock = self.lock()?;
        let mut state = self.read()?;
        let index = state
            .pending
            .iter()
            .position(|record| record.event_id == event_id)
            .context("hook queue changed during delivery")?;
        state.pending.remove(index);
        if state.acknowledged.len() == RECEIPT_LIMIT {
            state.acknowledged.pop_front();
        }
        state.acknowledged.push_back(event_id);
        self.write(&state)
    }

    fn prepare_directory(&self) -> anyhow::Result<()> {
        #[cfg(unix)]
        let created = {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new().mode(0o700).create(&self.0)
        };
        #[cfg(not(unix))]
        let created = fs::create_dir(&self.0);
        match created {
            Ok(()) => {
                let parent = self.0.parent().context("invalid hook queue location")?;
                sync_directory(parent)?;
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(_) => anyhow::bail!("cannot create hook queue"),
        }
        self.check_directory(
            &self
                .0
                .symlink_metadata()
                .context("cannot inspect hook queue")?,
        )
    }

    fn check_directory(&self, metadata: &fs::Metadata) -> anyhow::Result<()> {
        ensure!(metadata.is_dir(), "hook queue must be a private directory");
        require_private(metadata)
    }

    fn lock(&self) -> anyhow::Result<File> {
        let lock = open_private(&self.0.join("queue.lock"))?;
        let start = Instant::now();
        loop {
            match lock.try_lock() {
                Ok(()) => return Ok(lock),
                Err(std::fs::TryLockError::WouldBlock) if start.elapsed() < LOCK_WAIT => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(std::fs::TryLockError::WouldBlock) => anyhow::bail!("hook queue busy"),
                Err(_) => anyhow::bail!("cannot lock hook queue"),
            }
        }
    }

    fn read(&self) -> anyhow::Result<State> {
        let file = match File::open(self.0.join("queue-v1.json")) {
            Ok(file) => file,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(State::default()),
            Err(_) => anyhow::bail!("cannot open hook queue"),
        };
        require_private(&file.metadata().context("cannot inspect hook queue state")?)?;
        let state: State = serde_json::from_slice(&read_bounded(file, STATE_LIMIT)?)
            .map_err(|_| anyhow::anyhow!("invalid hook queue state"))?;
        ensure!(
            state.pending.len() <= PENDING_LIMIT && state.acknowledged.len() <= RECEIPT_LIMIT,
            "hook queue size limit exceeded"
        );
        Ok(state)
    }

    fn write(&self, state: &State) -> anyhow::Result<()> {
        let data = serde_json::to_vec(state).context("cannot encode hook queue")?;
        ensure!(
            data.len() as u64 <= STATE_LIMIT,
            "hook queue size limit exceeded"
        );
        let temp = self.0.join("queue-v1.tmp");
        let mut file = open_private(&temp)?;
        file.set_len(0)
            .context("cannot reset hook queue staging file")?;
        file.write_all(&data).context("cannot stage hook queue")?;
        file.sync_all()
            .context("cannot sync hook queue staging file")?;
        drop(file);
        fs::rename(temp, self.0.join("queue-v1.json")).context("cannot publish hook queue")?;
        sync_directory(&self.0)
    }
}

fn open_private(path: &Path) -> anyhow::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path).context("cannot open hook queue file")?;
    let meta = file.metadata().context("cannot inspect hook queue file")?;
    ensure!(meta.is_file(), "hook queue entry must be a file");
    require_private(&meta)?;
    Ok(file)
}

fn sync_directory(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    File::open(path)
        .and_then(|f| f.sync_all())
        .context("cannot sync hook queue directory")?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
