//! Typed SQLite access with bounded leases and ordered blocking execution.
//!
//! Each connection owns a FIFO on a Tokio blocking thread. SQLite never blocks
//! an async executor. Cancelling an await does not stop running SQL: the lease is
//! poisoned and its queued rollback runs after that operation, before reuse.
//! The same FIFO drains RETURNING, closes SQLite, and only then releases the host
//! lock. These are connection workers, not additional durable job workers.

use rusqlite::OpenFlags;
pub use rusqlite::types::Value;
use std::future::Future;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use uuid::Uuid;

const CONNECTIONS: u32 = 5;

fn immutable_uri(path: &Path) -> Result<String> {
    let path = path
        .to_str()
        .ok_or(Error::Decode("database path is not UTF-8"))?;
    #[cfg(windows)]
    let path = path
        .strip_prefix(r"\\?\")
        .unwrap_or(path)
        .replace('\\', "/");
    #[cfg(windows)]
    let path = if let Some(unc) = path.strip_prefix("UNC/") {
        format!("//{unc}")
    } else {
        path
    };
    let mut uri = String::from("file:");
    if path.starts_with("//") {
        uri.push_str("//");
    } else if !path.starts_with('/') {
        uri.push('/');
    }
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-_.~:".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            uri.push('%');
            uri.push(char::from(b"0123456789ABCDEF"[usize::from(byte >> 4)]));
            uri.push(char::from(b"0123456789ABCDEF"[usize::from(byte & 15)]));
        }
    }
    uri.push_str("?immutable=1");
    Ok(uri)
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error("invalid database value: {0}")]
    Decode(&'static str),
    #[error("query returned no rows")]
    RowNotFound,
    #[error("{0}")]
    Incompatible(&'static str),
    #[error("database connection cleanup failed")]
    Cleanup,
    #[error("database transaction was aborted")]
    TransactionAborted,
    #[error("database operation failed or did not finish; rollback or release the connection")]
    Unfinished,
    #[error("database pool is closed")]
    Closed,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(all(test, feature = "local-storage"))]
#[derive(Default)]
struct TestCounts {
    transactions: std::sync::atomic::AtomicUsize,
    calls: std::sync::atomic::AtomicUsize,
}

struct PoolState {
    path: PathBuf,
    idle: Mutex<Vec<Connection>>,
    permits: Arc<Semaphore>,
    closed: AtomicBool,
    cleanup_failed: AtomicBool,
    read_only: bool,
    lifetime_guard: Mutex<Option<Arc<dyn Send + Sync>>>,
    #[cfg(all(test, feature = "local-storage"))]
    counts: Arc<TestCounts>,
}

#[derive(Clone)]
pub struct Pool(Arc<PoolState>);

impl Pool {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_mode(path.as_ref(), false).await
    }

    pub async fn open_read_only(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_mode(path.as_ref(), true).await
    }

    async fn open_mode(path: &Path, read_only: bool) -> Result<Self> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let path = if path.exists() {
            tokio::fs::canonicalize(path).await?
        } else {
            tokio::fs::canonicalize(parent)
                .await?
                .join(path.file_name().ok_or(Error::Decode("database filename"))?)
        };
        Ok(Self(Arc::new(PoolState {
            path,
            idle: Mutex::new(Vec::new()),
            permits: Arc::new(Semaphore::new(CONNECTIONS as usize)),
            closed: AtomicBool::new(false),
            cleanup_failed: AtomicBool::new(false),
            read_only,
            lifetime_guard: Mutex::new(None),
            #[cfg(all(test, feature = "local-storage"))]
            counts: Arc::default(),
        })))
    }

    /// Retained by cloned pools, leases and every live SQLite connection worker.
    pub(crate) fn hold_lifetime_guard(&self, guard: Arc<dyn Send + Sync>) {
        *self.0.lifetime_guard.lock().expect("SQLite pool poisoned") = Some(guard);
    }

    pub async fn acquire(&self) -> Result<PoolConnection> {
        let permit = self
            .0
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Error::Closed)?;
        if self.0.closed.load(Ordering::Acquire) {
            return Err(Error::Closed);
        }
        if self.0.cleanup_failed.load(Ordering::Acquire) {
            return Err(Error::Cleanup);
        }
        let idle = self.0.idle.lock().expect("SQLite pool poisoned").pop();
        let connection = match idle {
            Some(connection) => connection,
            None => return Connection::open(self.0.clone(), permit).await,
        };
        Ok(PoolConnection {
            connection: Some(connection),
            pool: self.0.clone(),
            permit: Some(permit),
        })
    }

    pub async fn begin(&self) -> Result<Transaction> {
        self.begin_with("BEGIN DEFERRED").await
    }

    /// Per-pool test evidence: acknowledged begins and client driver calls.
    /// No query, timing threshold, production telemetry or persisted counter.
    #[cfg(all(test, feature = "local-storage"))]
    pub(crate) fn test_counts(&self) -> (usize, usize) {
        (
            self.0.counts.transactions.load(Ordering::Relaxed),
            self.0.counts.calls.load(Ordering::Relaxed),
        )
    }

    pub async fn begin_with(&self, statement: &str) -> Result<Transaction> {
        if !matches!(statement, "BEGIN" | "BEGIN DEFERRED" | "BEGIN IMMEDIATE") {
            return Err(Error::Decode("unsupported transaction mode"));
        }
        let mut connection = self.acquire().await?;
        query(statement).execute(&mut *connection).await?;
        #[cfg(all(test, feature = "local-storage"))]
        self.0.counts.transactions.fetch_add(1, Ordering::Relaxed);
        connection.managed_transaction = true;
        Ok(Transaction { connection })
    }

    pub async fn close(&self) {
        let _ = self.shutdown().await;
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.0.closed.store(true, Ordering::Release);
        let _permits = self
            .0
            .permits
            .acquire_many(CONNECTIONS)
            .await
            .map_err(|_| Error::Closed)?;
        let idle = std::mem::take(&mut *self.0.idle.lock().expect("SQLite pool poisoned"));
        let mut failure = None;
        for connection in idle {
            let (reply, closed) = oneshot::channel();
            if connection.sender.send(Command::Close(reply)).is_err() || closed.await.is_err() {
                failure = Some(Error::Closed);
            }
        }
        self.0
            .lifetime_guard
            .lock()
            .expect("SQLite pool poisoned")
            .take();
        match failure {
            Some(error) => Err(error),
            None if self.0.cleanup_failed.load(Ordering::Acquire) => Err(Error::Cleanup),
            None => Ok(()),
        }
    }
}

type Operation = Box<dyn FnOnce(&mut rusqlite::Connection) + Send>;
enum Command {
    Run(Operation),
    Close(oneshot::Sender<()>),
}

pub struct Connection {
    sender: mpsc::UnboundedSender<Command>,
    interrupted: bool,
    autocommit: bool,
    managed_transaction: bool,
    #[cfg(all(test, feature = "local-storage"))]
    counts: Arc<TestCounts>,
}
impl Connection {
    async fn open(pool: Arc<PoolState>, permit: OwnedSemaphorePermit) -> Result<PoolConnection> {
        let path = pool.path.clone();
        let read_only = pool.read_only;
        let guard = pool
            .lifetime_guard
            .lock()
            .expect("SQLite pool poisoned")
            .clone();
        let (sender, mut commands) = mpsc::unbounded_channel();
        let (ready, opened) = oneshot::channel();
        // A connection stays on one blocking thread for its entire lifetime.
        // The FIFO prevents a cancelled, queued write from running after cleanup.
        tokio::task::spawn_blocking(move || {
            let guard = guard;
            let opened = (|| -> Result<rusqlite::Connection> {
                let flags = if read_only {
                    OpenFlags::SQLITE_OPEN_READ_ONLY
                } else {
                    OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
                };
                // A stopped, checkpointed database needs no WAL recovery. Use
                // immutable read-only access there so admission creates no
                // sidecars. Never use immutable when committed WAL may exist.
                let mut wal = path.as_os_str().to_os_string();
                wal.push("-wal");
                let checkpointed = if read_only {
                    match std::fs::metadata(PathBuf::from(wal)) {
                        Ok(metadata) => metadata.len() == 0,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
                        Err(error) => return Err(error.into()),
                    }
                } else {
                    false
                };
                let raw = if read_only && checkpointed {
                    rusqlite::Connection::open_with_flags(
                        immutable_uri(&path)?,
                        flags | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX,
                    )?
                } else {
                    rusqlite::Connection::open_with_flags(
                        path,
                        flags | OpenFlags::SQLITE_OPEN_NO_MUTEX,
                    )?
                };
                raw.busy_timeout(Duration::from_secs(5))?;
                raw.pragma_update(None, "foreign_keys", true)?;
                if !read_only {
                    let mode: String =
                        raw.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
                    if mode != "wal" {
                        return Err(Error::Incompatible("SQLite WAL is required"));
                    }
                    raw.pragma_update(None, "synchronous", "FULL")?;
                }
                Ok(raw)
            })();
            let mut raw = match opened {
                Ok(raw) => raw,
                Err(error) => {
                    let _ = ready.send(Err(error));
                    return;
                }
            };
            let connection = Connection {
                sender,
                interrupted: false,
                autocommit: true,
                managed_transaction: false,
                #[cfg(all(test, feature = "local-storage"))]
                counts: pool.counts.clone(),
            };
            let _ = ready.send(Ok(PoolConnection {
                connection: Some(connection),
                pool,
                permit: Some(permit),
            }));
            let mut closed = None;
            while let Some(command) = commands.blocking_recv() {
                match command {
                    Command::Run(operation) => operation(&mut raw),
                    Command::Close(reply) => {
                        closed = Some(reply);
                        break;
                    }
                }
            }
            drop(raw);
            drop(guard);
            if let Some(reply) = closed {
                let _ = reply.send(());
            }
        });
        opened.await.map_err(|_| Error::Closed)?
    }

    fn check_ready(&self) -> Result<()> {
        if self.interrupted {
            return Err(Error::Unfinished);
        }
        Ok(())
    }

    async fn call<T: Send + 'static>(
        &mut self,
        operation: impl FnOnce(&mut rusqlite::Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (reply, result) = oneshot::channel();
        #[cfg(all(test, feature = "local-storage"))]
        let counts = self.counts.clone();
        self.sender
            .send(Command::Run(Box::new(move |raw| {
                #[cfg(all(test, feature = "local-storage"))]
                counts.calls.fetch_add(1, Ordering::Relaxed);
                let result = operation(raw).map(|value| (value, raw.is_autocommit()));
                let _ = reply.send(result);
            })))
            .map_err(|_| Error::Closed)?;
        let (value, autocommit) = result.await.map_err(|_| Error::Closed)??;
        self.autocommit = autocommit;
        Ok(value)
    }

    async fn run<T: Send + 'static>(
        &mut self,
        operation: impl FnOnce(&mut rusqlite::Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        self.check_ready()?;
        let managed = self.managed_transaction;
        self.interrupted = true;
        let value = self
            .call(move |raw| {
                if managed && raw.is_autocommit() {
                    return Err(Error::TransactionAborted);
                }
                operation(raw)
            })
            .await?;
        self.interrupted = false;
        Ok(value)
    }
}

pub struct PoolConnection {
    connection: Option<Connection>,
    pool: Arc<PoolState>,
    permit: Option<OwnedSemaphorePermit>,
}
impl Deref for PoolConnection {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.connection.as_ref().expect("active lease")
    }
}
impl DerefMut for PoolConnection {
    fn deref_mut(&mut self) -> &mut Connection {
        self.connection.as_mut().expect("active lease")
    }
}
impl Drop for PoolConnection {
    fn drop(&mut self) {
        let Some(mut connection) = self.connection.take() else {
            return;
        };
        // A fully acknowledged, autocommit operation has no cleanup to enqueue.
        // Its last SQLite state arrived with the result from the same FIFO.
        if !connection.interrupted && connection.autocommit {
            connection.managed_transaction = false;
            self.pool
                .idle
                .lock()
                .expect("SQLite pool poisoned")
                .push(connection);
            return;
        }
        let pool = self.pool.clone();
        let permit = self.permit.take();
        let sender = connection.sender.clone();
        let sent = sender.send(Command::Run(Box::new(move |raw| {
            let cleaned = if raw.is_autocommit() {
                Ok(())
            } else {
                raw.execute_batch("ROLLBACK")
            };
            if cleaned.is_err() {
                pool.cleanup_failed.store(true, Ordering::Release);
            }
            connection.interrupted = false;
            connection.autocommit = true;
            connection.managed_transaction = false;
            // Even a failed connection stays here for shutdown to close. The
            // cleanup_failed gate prevents it (or any other lease) being reused.
            pool.idle
                .lock()
                .expect("SQLite pool poisoned")
                .push(connection);
            drop(permit);
        })));
        if sent.is_err() {
            self.pool.cleanup_failed.store(true, Ordering::Release);
        }
    }
}

pub struct Transaction {
    connection: PoolConnection,
}
impl Transaction {
    pub async fn commit(mut self) -> Result<()> {
        self.connection
            .run(|raw| {
                raw.execute_batch("COMMIT")?;
                Ok(())
            })
            .await?;
        self.connection.managed_transaction = false;
        Ok(())
    }
    pub async fn rollback(mut self) -> Result<()> {
        self.connection.interrupted = true;
        self.connection
            .call(|raw| {
                if !raw.is_autocommit() {
                    raw.execute_batch("ROLLBACK")?;
                }
                Ok(())
            })
            .await?;
        self.connection.interrupted = false;
        self.connection.managed_transaction = false;
        Ok(())
    }
}
impl Deref for Transaction {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.connection
    }
}
impl DerefMut for Transaction {
    fn deref_mut(&mut self) -> &mut Connection {
        &mut self.connection
    }
}
// The owned variant keeps a pool lease alive through full row draining.
pub enum ConnectionAccess<'a> {
    Borrowed(&'a mut Connection),
    Pooled(PoolConnection),
}
impl Deref for ConnectionAccess<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        match self {
            Self::Borrowed(c) => c,
            Self::Pooled(c) => c,
        }
    }
}
impl DerefMut for ConnectionAccess<'_> {
    fn deref_mut(&mut self) -> &mut Connection {
        match self {
            Self::Borrowed(c) => c,
            Self::Pooled(c) => c,
        }
    }
}
/// Convert synchronously before constructing a query future. The future then
/// carries this concrete enum, not a higher-ranked generic Executor trait.
/// This keeps the Send contract usable by Tokio, Axum and MCP hosts.
pub enum Executor<'a> {
    Connection(&'a mut Connection),
    Pool(&'a Pool),
}
impl<'a> From<&'a mut Connection> for Executor<'a> {
    fn from(connection: &'a mut Connection) -> Self {
        Self::Connection(connection)
    }
}
impl<'a> From<&'a Pool> for Executor<'a> {
    fn from(pool: &'a Pool) -> Self {
        Self::Pool(pool)
    }
}
impl<'a> Executor<'a> {
    async fn connection(self) -> Result<ConnectionAccess<'a>> {
        match self {
            Self::Connection(connection) => Ok(ConnectionAccess::Borrowed(connection)),
            Self::Pool(pool) => Ok(ConnectionAccess::Pooled(pool.acquire().await?)),
        }
    }
}

pub type DriverFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

pub trait Encode {
    fn encode(self) -> Result<Value>;
}
impl Encode for String {
    fn encode(self) -> Result<Value> {
        Ok(Value::Text(self))
    }
}
impl Encode for &str {
    fn encode(self) -> Result<Value> {
        self.to_owned().encode()
    }
}
impl<T: Encode + Clone> Encode for &T {
    fn encode(self) -> Result<Value> {
        (*self).clone().encode()
    }
}
impl Encode for Uuid {
    fn encode(self) -> Result<Value> {
        Ok(Value::Blob(self.as_bytes().to_vec()))
    }
}
impl Encode for Vec<u8> {
    fn encode(self) -> Result<Value> {
        Ok(Value::Blob(self))
    }
}
impl Encode for bool {
    fn encode(self) -> Result<Value> {
        Ok(Value::Integer(i64::from(self)))
    }
}
impl Encode for f64 {
    fn encode(self) -> Result<Value> {
        Ok(Value::Real(self))
    }
}
impl Encode for serde_json::Value {
    fn encode(self) -> Result<Value> {
        self.to_string().encode()
    }
}
impl<T: Encode> Encode for Option<T> {
    fn encode(self) -> Result<Value> {
        self.map_or(Ok(Value::Null), Encode::encode)
    }
}
macro_rules! encode_integer { ($($ty:ty),+) => { $(impl Encode for $ty {
    fn encode(self) -> Result<Value> {
        Ok(Value::Integer(i64::try_from(self).map_err(|_| Error::Decode("integer overflow"))?))
    }
})+ }; }
encode_integer!(u64, usize);
impl Encode for i64 {
    fn encode(self) -> Result<Value> {
        Ok(Value::Integer(self))
    }
}
impl Encode for i32 {
    fn encode(self) -> Result<Value> {
        i64::from(self).encode()
    }
}
impl Encode for u32 {
    fn encode(self) -> Result<Value> {
        i64::from(self).encode()
    }
}

pub trait Decode<'a>: Sized {
    fn decode(value: &'a Value) -> Result<Self>;
}
impl<'a> Decode<'a> for &'a str {
    fn decode(value: &'a Value) -> Result<Self> {
        match value {
            Value::Text(value) => Ok(value),
            _ => Err(Error::Decode("expected text")),
        }
    }
}
impl Decode<'_> for String {
    fn decode(value: &Value) -> Result<Self> {
        <&str>::decode(value).map(str::to_owned)
    }
}
impl Decode<'_> for Uuid {
    fn decode(value: &Value) -> Result<Self> {
        match value {
            Value::Blob(value) => {
                Uuid::from_slice(value).map_err(|_| Error::Decode("expected 16-byte UUID"))
            }
            _ => Err(Error::Decode("expected UUID blob")),
        }
    }
}
impl Decode<'_> for Vec<u8> {
    fn decode(value: &Value) -> Result<Self> {
        match value {
            Value::Blob(value) => Ok(value.clone()),
            _ => Err(Error::Decode("expected blob")),
        }
    }
}
impl Decode<'_> for i64 {
    fn decode(value: &Value) -> Result<Self> {
        match value {
            Value::Integer(value) => Ok(*value),
            _ => Err(Error::Decode("expected integer")),
        }
    }
}
macro_rules! decode_integer { ($($ty:ty),+) => { $(impl Decode<'_> for $ty {
    fn decode(value: &Value) -> Result<Self> {
        Self::try_from(i64::decode(value)?).map_err(|_| Error::Decode("integer overflow"))
    }
})+ }; }
decode_integer!(i32, u32, u64, usize);
impl Decode<'_> for bool {
    fn decode(value: &Value) -> Result<Self> {
        match i64::decode(value)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error::Decode("expected boolean")),
        }
    }
}
impl Decode<'_> for f64 {
    fn decode(value: &Value) -> Result<Self> {
        match value {
            Value::Real(value) => Ok(*value),
            Value::Integer(value) => Ok(*value as f64),
            _ => Err(Error::Decode("expected number")),
        }
    }
}
impl Decode<'_> for serde_json::Value {
    fn decode(value: &Value) -> Result<Self> {
        serde_json::from_str(<&str>::decode(value)?).map_err(|_| Error::Decode("invalid JSON"))
    }
}
impl<'a, T: Decode<'a>> Decode<'a> for Option<T> {
    fn decode(value: &'a Value) -> Result<Self> {
        match value {
            Value::Null => Ok(None),
            _ => T::decode(value).map(Some),
        }
    }
}

#[derive(Debug)]
pub struct Row {
    names: Arc<Vec<String>>,
    values: Vec<Value>,
}
pub trait ColumnIndex {
    fn index(&self, names: &[String]) -> Result<usize>;
}
impl ColumnIndex for usize {
    fn index(&self, _names: &[String]) -> Result<usize> {
        Ok(*self)
    }
}
impl ColumnIndex for &str {
    fn index(&self, names: &[String]) -> Result<usize> {
        names
            .iter()
            .position(|name| name.as_str() == *self)
            .ok_or(Error::Decode("column not found"))
    }
}
impl Row {
    pub fn try_get<'a, T: Decode<'a>>(&'a self, index: impl ColumnIndex) -> Result<T> {
        let index = index.index(&self.names)?;
        T::decode(
            self.values
                .get(index)
                .ok_or(Error::Decode("column out of bounds"))?,
        )
    }
    pub fn get<'a, T: Decode<'a>>(&'a self, index: impl ColumnIndex) -> T {
        self.try_get(index).expect("validated database row")
    }
}
pub trait FromRow: Sized {
    fn from_row(row: &Row) -> Result<Self>;
}
impl FromRow for Row {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(Row {
            names: row.names.clone(),
            values: row.values.clone(),
        })
    }
}
macro_rules! tuple_row { ($($ty:ident:$index:tt),+) => {
    impl<$($ty: for<'a> Decode<'a>),+> FromRow for ($($ty,)+) {
        fn from_row(row: &Row) -> Result<Self> { Ok(($(row.try_get::<$ty>($index)?,)+)) }
    }
}; }
tuple_row!(A:0);
tuple_row!(A:0,B:1);
tuple_row!(A:0,B:1,C:2);
tuple_row!(A:0,B:1,C:2,D:3);
tuple_row!(A:0,B:1,C:2,D:3,E:4);
tuple_row!(A:0,B:1,C:2,D:3,E:4,F:5);
tuple_row!(A:0,B:1,C:2,D:3,E:4,F:5,G:6);
tuple_row!(A:0,B:1,C:2,D:3,E:4,F:5,G:6,H:7);
tuple_row!(A:0,B:1,C:2,D:3,E:4,F:5,G:6,H:7,I:8);
tuple_row!(A:0,B:1,C:2,D:3,E:4,F:5,G:6,H:7,I:8,J:9);
tuple_row!(A:0,B:1,C:2,D:3,E:4,F:5,G:6,H:7,I:8,J:9,K:10);
tuple_row!(A:0,B:1,C:2,D:3,E:4,F:5,G:6,H:7,I:8,J:9,K:10,L:11);
tuple_row!(A:0,B:1,C:2,D:3,E:4,F:5,G:6,H:7,I:8,J:9,K:10,L:11,M:12);
tuple_row!(A:0,B:1,C:2,D:3,E:4,F:5,G:6,H:7,I:8,J:9,K:10,L:11,M:12,N:13);
tuple_row!(A:0,B:1,C:2,D:3,E:4,F:5,G:6,H:7,I:8,J:9,K:10,L:11,M:12,N:13,O:14);
tuple_row!(A:0,B:1,C:2,D:3,E:4,F:5,G:6,H:7,I:8,J:9,K:10,L:11,M:12,N:13,O:14,P:15);

pub struct Query<T = Row> {
    sql: String,
    params: Result<Vec<Value>>,
    _row: PhantomData<fn() -> T>,
}
pub fn query(sql: &str) -> Query {
    query_as(sql)
}
pub fn query_as<T: FromRow>(sql: &str) -> Query<T> {
    Query {
        sql: sql.to_owned(),
        params: Ok(Vec::new()),
        _row: PhantomData,
    }
}
impl<T> Query<T> {
    pub fn bind(mut self, value: impl Encode) -> Self {
        self.params = self.params.and_then(|mut values| {
            values.push(value.encode()?);
            Ok(values)
        });
        self
    }
    pub fn execute<'a>(self, executor: impl Into<Executor<'a>>) -> DriverFuture<'a, QueryResult> {
        let executor = executor.into();
        let Query { sql, params, .. } = self;
        Box::pin(async move {
            let mut connection = executor.connection().await?;
            let params = params?;
            connection
                .run(move |raw| {
                    Ok(QueryResult(
                        raw.execute(&sql, rusqlite::params_from_iter(params))? as u64,
                    ))
                })
                .await
        })
    }
}
impl<T: FromRow + Send + 'static> Query<T> {
    pub fn fetch_all<'a>(self, executor: impl Into<Executor<'a>>) -> DriverFuture<'a, Vec<T>> {
        let executor = executor.into();
        Box::pin(async move {
            let mut connection = executor.connection().await?;
            let params = self.params?;
            connection
                .run(move |raw| {
                    let mut statement = raw.prepare(&self.sql)?;
                    let names = Arc::new(
                        statement
                            .column_names()
                            .map(str::to_owned)
                            .collect::<Vec<_>>(),
                    );
                    let mut rows = statement.query(rusqlite::params_from_iter(params))?;
                    let mut output = Vec::new();
                    while let Some(row) = rows.next()? {
                        let values = (0..names.len())
                            .map(|index| row.get(index))
                            .collect::<rusqlite::Result<Vec<Value>>>()?;
                        output.push(T::from_row(&Row {
                            names: names.clone(),
                            values,
                        })?);
                    }
                    Ok(output)
                })
                .await
        })
    }
    pub fn fetch_optional<'a>(
        self,
        executor: impl Into<Executor<'a>>,
    ) -> DriverFuture<'a, Option<T>> {
        let rows = self.fetch_all(executor);
        Box::pin(async move { Ok(rows.await?.into_iter().next()) })
    }
    pub fn fetch_one<'a>(self, executor: impl Into<Executor<'a>>) -> DriverFuture<'a, T> {
        let row = self.fetch_optional(executor);
        Box::pin(async move { row.await?.ok_or(Error::RowNotFound) })
    }
}
pub struct ScalarQuery<T>(Query<(T,)>);
pub fn query_scalar<T: for<'a> Decode<'a>>(sql: &str) -> ScalarQuery<T> {
    ScalarQuery(query_as(sql))
}
impl<T: for<'r> Decode<'r> + Send + 'static> ScalarQuery<T> {
    pub fn bind(self, value: impl Encode) -> Self {
        Self(self.0.bind(value))
    }
    pub fn fetch_all<'a>(self, executor: impl Into<Executor<'a>>) -> DriverFuture<'a, Vec<T>> {
        let rows = self.0.fetch_all(executor);
        Box::pin(async move { Ok(rows.await?.into_iter().map(|row| row.0).collect()) })
    }
    pub fn fetch_optional<'a>(
        self,
        executor: impl Into<Executor<'a>>,
    ) -> DriverFuture<'a, Option<T>> {
        let row = self.0.fetch_optional(executor);
        Box::pin(async move { Ok(row.await?.map(|row| row.0)) })
    }
    pub fn fetch_one<'a>(self, executor: impl Into<Executor<'a>>) -> DriverFuture<'a, T> {
        let row = self.0.fetch_one(executor);
        Box::pin(async move { Ok(row.await?.0) })
    }
}
pub struct QueryResult(u64);
impl QueryResult {
    pub fn rows_affected(&self) -> u64 {
        self.0
    }
}
pub struct RawSql(String);
pub fn raw_sql(sql: &str) -> RawSql {
    RawSql(sql.to_owned())
}
impl RawSql {
    pub fn execute<'a>(self, executor: impl Into<Executor<'a>>) -> DriverFuture<'a, QueryResult> {
        let executor = executor.into();
        Box::pin(async move {
            let mut connection = executor.connection().await?;
            connection
                .run(move |raw| {
                    raw.execute_batch(&self.0)?;
                    Ok(QueryResult(0))
                })
                .await
        })
    }
}

const FORMAT_SCHEMA: &str = "CREATE TABLE oc_storage_format (id INTEGER PRIMARY KEY CHECK (id=1), engine TEXT NOT NULL, role TEXT NOT NULL, schema_version INTEGER NOT NULL)";
const ENGINE_FORMAT: &str = "origence-sqlite-v2";

pub(crate) async fn install_format(conn: &mut Connection, role: &str) -> Result<()> {
    raw_sql(FORMAT_SCHEMA).execute(&mut *conn).await?;
    query("INSERT INTO oc_storage_format(id,engine,role,schema_version) VALUES(1,?,?,1)")
        .bind(ENGINE_FORMAT)
        .bind(role)
        .execute(conn)
        .await?;
    Ok(())
}

pub(crate) async fn check_format(conn: &mut Connection, role: &str) -> Result<()> {
    let ddl: Option<String> = query_scalar(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='oc_storage_format'",
    )
    .fetch_optional(&mut *conn)
    .await?;
    if ddl
        .as_deref()
        .map(|sql| canonical_ddl(sql, "oc_storage_format"))
        != Some(canonical_ddl(FORMAT_SCHEMA, "oc_storage_format"))
    {
        return Err(Error::Incompatible(
            "missing or incompatible SQLite format marker",
        ));
    }
    let rows: Vec<(i64, String, String, i64)> =
        query_as("SELECT id,engine,role,schema_version FROM oc_storage_format")
            .fetch_all(conn)
            .await?;
    if rows != vec![(1, ENGINE_FORMAT.into(), role.into(), 1)] {
        return Err(Error::Incompatible("incompatible SQLite format marker"));
    }
    Ok(())
}

/// Token-preserving schema comparison. Only differences observed and verified
/// for embedded schemas are admitted: CREATE's IF NOT EXISTS display guard and
/// the engine's quoting of `key` in oc_commands / `action` in oc_audit.
/// String literals and all other quoted identifiers retain their exact bytes.
fn ddl_tokens(sql: &str) -> Vec<String> {
    let chars: Vec<char> = sql.chars().collect();
    let mut output = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        if chars[i] == '-' && chars.get(i + 1) == Some(&'-') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i = (i + 2).min(chars.len());
            continue;
        }
        let start = i;
        if matches!(chars[i], '\'' | '"' | '`' | '[') {
            let end = if chars[i] == '[' { ']' } else { chars[i] };
            i += 1;
            while i < chars.len() {
                if chars[i] == end {
                    i += 1;
                    if end != ']' && chars.get(i) == Some(&end) {
                        i += 1;
                    } else {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            output.push(chars[start..i].iter().collect());
        } else {
            i += 1;
            if chars[start].is_ascii_alphanumeric() || chars[start] == '_' {
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
            }
            output.push(
                chars[start..i]
                    .iter()
                    .collect::<String>()
                    .to_ascii_lowercase(),
            );
        }
    }
    if output.last().is_some_and(|token| token == ";") {
        output.pop();
    }
    output
}

pub(crate) fn canonical_ddl(sql: &str, object: &str) -> Vec<String> {
    let mut tokens = ddl_tokens(sql);
    let end = if tokens.get(1).is_some_and(|t| t == "unique") {
        3
    } else {
        2
    };
    if tokens.first().is_some_and(|t| t == "create")
        && tokens
            .get(end..end + 3)
            .is_some_and(|t| t == ["if", "not", "exists"])
    {
        tokens.drain(end..end + 3);
    }
    let quoted = match object {
        "oc_commands" => Some(("\"key\"", "key")),
        "oc_audit" => Some(("\"action\"", "action")),
        _ => None,
    };
    if let Some((from, to)) = quoted {
        for token in &mut tokens {
            if token == from {
                *token = to.into();
            }
        }
    }
    tokens
}

fn schema_objects(schema: &str) -> Result<Vec<(String, Vec<String>)>> {
    // Embedded schema files have one CREATE per line start. Do not split on
    // semicolons: trigger bodies contain complete SQL statements themselves.
    let mut statements = Vec::new();
    let mut statement = String::new();
    for line in schema.lines() {
        if line.trim_start().starts_with("CREATE ") && !statement.trim().is_empty() {
            statements.push(std::mem::take(&mut statement));
        }
        statement.push_str(line);
        statement.push('\n');
    }
    if !statement.trim().is_empty() {
        statements.push(statement);
    }
    let mut output = Vec::new();
    for statement in statements {
        let tokens = canonical_ddl(&statement, "");
        if tokens.is_empty() {
            continue;
        }
        let index = if tokens.get(1).is_some_and(|t| t == "unique") {
            3
        } else {
            2
        };
        if tokens.first().is_none_or(|t| t != "create") {
            return Err(Error::Incompatible("invalid embedded SQLite schema"));
        }
        let name = tokens
            .get(index)
            .ok_or(Error::Incompatible("invalid embedded SQLite schema"))?
            .clone();
        output.push((name.clone(), canonical_ddl(&statement, &name)));
    }
    Ok(output)
}

pub(crate) async fn verify_schema(conn: &mut Connection, schemas: &[&str]) -> Result<()> {
    let mut expected = schema_objects(FORMAT_SCHEMA)?;
    for schema in schemas {
        expected.extend(schema_objects(schema)?);
    }
    expected.sort();
    let rows: Vec<(String, String)> = query_as(
        "SELECT name,sql FROM sqlite_master WHERE name NOT GLOB 'sqlite_*' AND sql IS NOT NULL",
    )
    .fetch_all(conn)
    .await?;
    let mut actual = rows
        .into_iter()
        .map(|(name, sql)| {
            let tokens = canonical_ddl(&sql, &name);
            (name, tokens)
        })
        .collect::<Vec<_>>();
    actual.sort();
    if actual != expected {
        return Err(Error::Incompatible(
            "incompatible SQLite schema; no migration was attempted",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn every_connection_enforces_wal_full_foreign_keys_and_lease_limit() {
        let dir = tempfile::tempdir().unwrap();
        let pool = Pool::open(dir.path().join("pragmas.db")).await.unwrap();
        let mut leases = Vec::new();
        for _ in 0..CONNECTIONS {
            let mut connection = pool.acquire().await.unwrap();
            for (pragma, expected) in [
                ("foreign_keys", 1),
                ("synchronous", 2),
                ("busy_timeout", 5000),
            ] {
                assert_eq!(
                    query_scalar::<i64>(&format!("PRAGMA {pragma}"))
                        .fetch_one(&mut *connection)
                        .await
                        .unwrap(),
                    expected
                );
            }
            assert_eq!(
                query_scalar::<String>("PRAGMA journal_mode")
                    .fetch_one(&mut *connection)
                    .await
                    .unwrap(),
                "wal"
            );
            leases.push(connection);
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(20), pool.acquire())
                .await
                .is_err()
        );
        drop(leases);
        pool.shutdown().await.unwrap();
        assert!(matches!(pool.acquire().await, Err(Error::Closed)));
    }

    #[tokio::test]
    async fn read_only_admission_reads_committed_wal_and_checkpointed_unicode_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("元数据 % #.db");
        let pool = Pool::open(&path).await.unwrap();
        raw_sql("CREATE TABLE t(value TEXT); INSERT INTO t VALUES('原文 🌍')")
            .execute(&pool)
            .await
            .unwrap();
        let mut wal = path.as_os_str().to_os_string();
        wal.push("-wal");
        let wal = PathBuf::from(wal);
        let before = (std::fs::read(&path).unwrap(), std::fs::read(&wal).unwrap());
        assert!(!before.1.is_empty());
        let reader = Pool::open_read_only(&path).await.unwrap();
        assert_eq!(
            query_scalar::<String>("SELECT value FROM t")
                .fetch_one(&reader)
                .await
                .unwrap(),
            "原文 🌍"
        );
        reader.shutdown().await.unwrap();
        assert_eq!(
            (std::fs::read(&path).unwrap(), std::fs::read(&wal).unwrap()),
            before
        );
        pool.shutdown().await.unwrap();
        let names = || {
            let mut names = std::fs::read_dir(dir.path())
                .unwrap()
                .map(|p| p.unwrap().file_name())
                .collect::<Vec<_>>();
            names.sort();
            names
        };
        let before = (std::fs::read(&path).unwrap(), names());
        let reader = Pool::open_read_only(&path).await.unwrap();
        assert_eq!(
            query_scalar::<String>("SELECT value FROM t")
                .fetch_one(&reader)
                .await
                .unwrap(),
            "原文 🌍"
        );
        reader.shutdown().await.unwrap();
        assert_eq!((std::fs::read(&path).unwrap(), names()), before);
    }

    #[test]
    fn ddl_normalization_is_narrow_and_preserves_literals() {
        assert_eq!(
            canonical_ddl(
                "CREATE TABLE oc_commands (key TEXT DEFAULT 'pending');",
                "oc_commands"
            ),
            canonical_ddl(
                "create table if not exists oc_commands (\"key\" TEXT DEFAULT 'pending')",
                "oc_commands"
            )
        );
        assert_ne!(
            canonical_ddl("CREATE TABLE t (v TEXT DEFAULT 'pending')", "t"),
            canonical_ddl("CREATE TABLE t (v TEXT DEFAULT 'PENDING')", "t")
        );
        assert_ne!(
            canonical_ddl("CREATE TABLE t (v TEXT DEFAULT 'two words')", "t"),
            canonical_ddl("CREATE TABLE t (v TEXT DEFAULT 'twowords')", "t")
        );
        assert_ne!(
            canonical_ddl("CREATE TABLE t (key TEXT)", "t"),
            canonical_ddl("CREATE TABLE t (\"key\" TEXT)", "t")
        );
        assert_ne!(
            canonical_ddl("CREATE TABLE t (v TEXT DEFAULT 'if not exists')", "t"),
            canonical_ddl("CREATE TABLE t (v TEXT DEFAULT '')", "t")
        );
    }

    #[test]
    fn trigger_bodies_are_not_split_at_semicolons() {
        let schema = "CREATE TABLE fixture (value TEXT NOT NULL);
            CREATE TRIGGER fixture_immutable BEFORE UPDATE ON fixture BEGIN
                SELECT RAISE(ABORT, 'fixture value is immutable');
            END;
            CREATE TABLE following (id INTEGER PRIMARY KEY);";
        let objects = schema_objects(schema).unwrap();
        let trigger = objects
            .iter()
            .find(|(name, _)| name == "fixture_immutable")
            .unwrap();
        assert!(trigger.1.contains(&"'fixture value is immutable'".into()));
        assert_eq!(trigger.1.last().unwrap(), "end");
        assert!(objects.iter().any(|(name, _)| name == "following"));
    }

    #[tokio::test]
    async fn native_uuid_nullable_rows_and_transaction_drop() {
        let dir = tempfile::tempdir().unwrap();
        let pool = Pool::open(dir.path().join("values.db")).await.unwrap();
        raw_sql("CREATE TABLE values_test(id BLOB PRIMARY KEY, value TEXT, flag INTEGER NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        let id = Uuid::new_v4();
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        query("INSERT INTO values_test VALUES(?,?,?)")
            .bind(id)
            .bind(None::<String>)
            .bind(true)
            .execute(&mut *tx)
            .await
            .unwrap();
        let row: (Uuid, Option<String>, bool) = query_as("SELECT id,value,flag FROM values_test")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(row, (id, None, true));
        drop(tx);
        assert_eq!(
            query_scalar::<i64>("SELECT count(*) FROM values_test")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        query("INSERT INTO values_test VALUES(?,?,?)")
            .bind(id)
            .bind("你好 café")
            .bind(false)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let row: (String, i64, String) =
            query_as("SELECT typeof(id),length(id),value FROM values_test")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row, ("blob".into(), 16, "你好 café".into()));
        pool.shutdown().await.unwrap();
        let reopened = Pool::open(dir.path().join("values.db")).await.unwrap();
        assert_eq!(
            query_scalar::<Uuid>("SELECT id FROM values_test")
                .fetch_one(&reopened)
                .await
                .unwrap(),
            id
        );
        reopened.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn returning_is_drained_and_failure_does_not_escape_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let pool = Pool::open(dir.path().join("returning.db")).await.unwrap();
        raw_sql("CREATE TABLE t(id INTEGER PRIMARY KEY, value INTEGER NOT NULL); INSERT INTO t VALUES(1,0),(2,0),(3,0);").execute(&pool).await.unwrap();
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        assert!(
            query_scalar::<i64>("UPDATE t SET value=7 RETURNING id")
                .fetch_optional(&mut *tx)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            query("INSERT INTO t VALUES(1,99)")
                .execute(&mut *tx)
                .await
                .is_err()
        );
        drop(tx);
        assert_eq!(
            query_scalar::<i64>("SELECT sum(value) FROM t")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        query("UPDATE t SET value=1")
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(
            query_scalar::<i64>("SELECT sum(value) FROM t")
                .fetch_one(&pool)
                .await
                .unwrap(),
            3
        );
        pool.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn aborted_transaction_cannot_autocommit_following_writes() {
        let dir = tempfile::tempdir().unwrap();
        let pool = Pool::open(dir.path().join("aborted.db")).await.unwrap();
        raw_sql("CREATE TABLE t(value INTEGER); INSERT INTO t VALUES(0)")
            .execute(&pool)
            .await
            .unwrap();
        for entry in ["fetch", "execute", "batch"] {
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
            query("UPDATE t SET value=10")
                .execute(&mut *tx)
                .await
                .unwrap();
            let error = match entry {
                "fetch" => query("SELECT json_extract('{', '$.value')")
                    .fetch_all(&mut *tx)
                    .await
                    .map(|_| ()),
                "execute" => query("UPDATE t SET value=json_extract('{', '$.value')")
                    .execute(&mut *tx)
                    .await
                    .map(|_| ()),
                _ => raw_sql("UPDATE t SET value=json_extract('{', '$.value')")
                    .execute(&mut *tx)
                    .await
                    .map(|_| ()),
            };
            assert!(matches!(error, Err(Error::Sqlite(_))));
            // All entry points must refuse the retained transaction. In
            // particular, none may silently run in the engine's autocommit.
            assert!(matches!(
                query("UPDATE t SET value=20").execute(&mut *tx).await,
                Err(Error::Unfinished | Error::TransactionAborted)
            ));
            assert!(matches!(
                query("UPDATE t SET value=30 RETURNING value")
                    .fetch_all(&mut *tx)
                    .await,
                Err(Error::Unfinished | Error::TransactionAborted)
            ));
            assert!(matches!(
                raw_sql("UPDATE t SET value=40").execute(&mut *tx).await,
                Err(Error::Unfinished | Error::TransactionAborted)
            ));
            assert!(matches!(
                tx.commit().await,
                Err(Error::Unfinished | Error::TransactionAborted)
            ));
            // A new writer also waits for any asynchronous rollback cleanup.
            let mut next = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
            assert_eq!(
                query_scalar::<i64>("SELECT value FROM t")
                    .fetch_one(&mut *next)
                    .await
                    .unwrap(),
                0
            );
            query("UPDATE t SET value=1")
                .execute(&mut *next)
                .await
                .unwrap();
            next.rollback().await.unwrap();
        }
        pool.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn concurrent_reader_retains_snapshot_until_commit() {
        let dir = tempfile::tempdir().unwrap();
        let pool = Pool::open(dir.path().join("snapshot.db")).await.unwrap();
        raw_sql("CREATE TABLE t(value INTEGER); INSERT INTO t VALUES(1)")
            .execute(&pool)
            .await
            .unwrap();
        let mut reader = pool.begin().await.unwrap();
        assert_eq!(
            query_scalar::<i64>("SELECT value FROM t")
                .fetch_one(&mut *reader)
                .await
                .unwrap(),
            1
        );
        let mut writer = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        query("UPDATE t SET value=2")
            .execute(&mut *writer)
            .await
            .unwrap();
        writer.commit().await.unwrap();
        assert_eq!(
            query_scalar::<i64>("SELECT value FROM t")
                .fetch_one(&mut *reader)
                .await
                .unwrap(),
            1
        );
        reader.commit().await.unwrap();
        assert_eq!(
            query_scalar::<i64>("SELECT value FROM t")
                .fetch_one(&pool)
                .await
                .unwrap(),
            2
        );
        pool.shutdown().await.unwrap();
    }
    #[tokio::test(flavor = "current_thread")]
    async fn queued_write_cancellation_rolls_back_before_pool_reuse() {
        use std::future::poll_fn;
        use std::task::Poll;
        for explicit_rollback in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let pool = Pool::open(dir.path().join("cancellation.db"))
                .await
                .unwrap();
            raw_sql("CREATE TABLE t(value INTEGER); INSERT INTO t VALUES(1)")
                .execute(&pool)
                .await
                .unwrap();
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
            let release = block_worker(&tx).await;
            let mut operation = Box::pin(query("UPDATE t SET value=999").execute(&mut *tx));
            assert!(matches!(
                poll_fn(|cx| Poll::Ready(operation.as_mut().poll(cx))).await,
                Poll::Pending
            ));
            drop(operation);
            assert!(matches!(
                query("UPDATE t SET value=888").execute(&mut *tx).await,
                Err(Error::Unfinished)
            ));
            release.send(()).unwrap();
            if explicit_rollback {
                tx.rollback().await.unwrap();
            } else {
                assert!(matches!(tx.commit().await, Err(Error::Unfinished)));
            }
            assert_eq!(
                query_scalar::<i64>("SELECT value FROM t")
                    .fetch_one(&pool)
                    .await
                    .unwrap(),
                1
            );
            query("UPDATE t SET value=2").execute(&pool).await.unwrap();
            pool.shutdown().await.unwrap();
            // Independently open the file with SQLite after every worker closes.
            let raw = rusqlite::Connection::open(dir.path().join("cancellation.db")).unwrap();
            assert_eq!(
                raw.query_row("SELECT value FROM t", [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                2
            );
        }
    }

    // Hold the actual file-backed connection worker before a queued operation.
    // This exercises FIFO cleanup, not hard interruption of running SQLite IO.
    async fn block_worker(connection: &Connection) -> std::sync::mpsc::Sender<()> {
        let (started, ready) = oneshot::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        connection
            .sender
            .send(Command::Run(Box::new(move |_| {
                let _ = started.send(());
                let _ = blocked.recv();
            })))
            .unwrap();
        ready.await.unwrap();
        release
    }

    #[tokio::test]
    async fn dropped_writer_releases_other_pool_handle() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shared.db");
        let first = Pool::open(&path).await.unwrap();
        let second = Pool::open(&path).await.unwrap();
        raw_sql("CREATE TABLE t(value INTEGER); INSERT INTO t VALUES(1)")
            .execute(&first)
            .await
            .unwrap();
        let mut tx = first.begin_with("BEGIN IMMEDIATE").await.unwrap();
        query("UPDATE t SET value=999")
            .execute(&mut *tx)
            .await
            .unwrap();
        drop(tx);
        // Do not touch `first` again: another pool must not wait forever for
        // its dropped transaction to be cleaned on a future acquire.
        let mut writer =
            tokio::time::timeout(Duration::from_secs(6), second.begin_with("BEGIN IMMEDIATE"))
                .await
                .unwrap()
                .unwrap();
        assert_eq!(
            query_scalar::<i64>("SELECT value FROM t")
                .fetch_one(&mut *writer)
                .await
                .unwrap(),
            1
        );
        query("UPDATE t SET value=2")
            .execute(&mut *writer)
            .await
            .unwrap();
        writer.commit().await.unwrap();
        first.shutdown().await.unwrap();
        second.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn lifetime_guard_survives_store_and_lease_drop_until_cleanup() {
        struct Guard(Arc<AtomicBool>);
        impl Drop for Guard {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let pool = Pool::open(dir.path().join("guard.db")).await.unwrap();
        let dropped = Arc::new(AtomicBool::new(false));
        pool.hold_lifetime_guard(Arc::new(Guard(dropped.clone())));
        query("CREATE TABLE t(value INTEGER)")
            .execute(&pool)
            .await
            .unwrap();
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        query("INSERT INTO t VALUES(1)")
            .execute(&mut *tx)
            .await
            .unwrap();
        let release = block_worker(&tx).await;
        drop(pool);
        assert!(!dropped.load(Ordering::Acquire));
        drop(tx);
        assert!(!dropped.load(Ordering::Acquire));
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(6), async {
            while !dropped.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
}
