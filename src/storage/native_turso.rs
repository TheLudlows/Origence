//! Small typed adapter over the native Rust `turso` engine, pinned to 0.8.2.
//!
//! This is not SQLx, libSQL, or SQLite FFI. It retains the domain's parameterized
//! SQL while making UUID BLOB encoding, row decoding, connection leasing and
//! cancellation cleanup explicit. A lease is never concurrently shared. Unfinished
//! transactions receive one bounded asynchronous rollback on lease drop; only
//! cleaned connections return to the pool. Shutdown waits for that cleanup.
//! Query results are fully drained before a lease is released (including RETURNING).

use std::collections::HashMap;
use std::future::Future;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;

const CONNECTIONS: u32 = 5;
type DatabaseRegistry = HashMap<(PathBuf, bool), Weak<turso::Database>>;
static DATABASES: LazyLock<tokio::sync::Mutex<DatabaseRegistry>> =
    LazyLock::new(|| tokio::sync::Mutex::new(HashMap::new()));

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Native(#[from] turso::Error),
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

struct PoolState {
    database: Mutex<Option<Arc<turso::Database>>>,
    idle: Mutex<Vec<Connection>>,
    permits: Arc<Semaphore>,
    closed: AtomicBool,
    cleanup_failed: AtomicBool,
    read_only: bool,
    lifetime_guard: Mutex<Option<Arc<dyn Send + Sync>>>,
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
        let key = (path.clone(), read_only);
        // A single native Database owns the pager/WAL for each canonical path.
        // Independently opened API/worker handles still own separate lease pools.
        let mut databases = DATABASES.lock().await;
        let database = match databases.get(&key).and_then(Weak::upgrade) {
            Some(database) => database,
            None => {
                let path = path
                    .to_str()
                    .ok_or(Error::Decode("database path is not UTF-8"))?;
                let database = Arc::new(
                    turso::Builder::new_local(path)
                        .read_only(read_only)
                        .build()
                        .await?,
                );
                databases.insert(key, Arc::downgrade(&database));
                database
            }
        };
        Ok(Self(Arc::new(PoolState {
            database: Mutex::new(Some(database)),
            idle: Mutex::new(Vec::new()),
            permits: Arc::new(Semaphore::new(CONNECTIONS as usize)),
            closed: AtomicBool::new(false),
            cleanup_failed: AtomicBool::new(false),
            read_only,
            lifetime_guard: Mutex::new(None),
        })))
    }

    /// Keep the host/path lock alive through owned transactions, cloned pools
    /// and asynchronous rollback, even when their original Store is dropped.
    pub(crate) fn hold_lifetime_guard(&self, guard: Arc<dyn Send + Sync>) {
        *self.0.lifetime_guard.lock().expect("Turso pool poisoned") = Some(guard);
    }

    pub async fn acquire(&self) -> Result<PoolConnection> {
        if self.0.closed.load(Ordering::Acquire) {
            return Err(Error::Closed);
        }
        if self.0.cleanup_failed.load(Ordering::Acquire) {
            return Err(Error::Cleanup);
        }
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
        let idle = self.0.idle.lock().expect("Turso pool poisoned").pop();
        let mut connection = match idle {
            Some(connection) => connection,
            None => {
                let raw = self
                    .0
                    .database
                    .lock()
                    .expect("Turso pool poisoned")
                    .as_ref()
                    .ok_or(Error::Closed)?
                    .connect()?;
                raw.busy_timeout(Duration::from_secs(5))?;
                if !self.0.read_only {
                    raw.execute("PRAGMA foreign_keys = ON", ()).await?;
                }
                Connection {
                    raw,
                    interrupted: false,
                    managed_transaction: false,
                }
            }
        };
        // Errors, early returns, dropped transactions and cancelled futures all
        // return through this gate. No connection with an open transaction is
        // handed to another caller. A cleanup failure drops the connection.
        if !connection.raw.is_autocommit()? {
            connection.raw.execute("ROLLBACK", ()).await?;
        }
        if connection.interrupted {
            probe_connection(&connection.raw).await?;
            connection.interrupted = false;
        }
        connection.managed_transaction = false;
        Ok(PoolConnection {
            connection: Some(connection),
            pool: self.0.clone(),
            _permit: Some(permit),
        })
    }

    pub async fn begin(&self) -> Result<Transaction> {
        self.begin_with("BEGIN DEFERRED").await
    }

    pub async fn begin_with(&self, statement: &str) -> Result<Transaction> {
        if !matches!(statement, "BEGIN" | "BEGIN DEFERRED" | "BEGIN IMMEDIATE") {
            return Err(Error::Decode("unsupported transaction mode"));
        }
        let mut connection = self.acquire().await?;
        query(statement).execute(&mut *connection).await?;
        connection.managed_transaction = true;
        Ok(Transaction { connection })
    }

    pub async fn close(&self) {
        let _ = self.shutdown().await;
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.0.closed.store(true, Ordering::Release);
        // Wait for borrowed transactions/queries, then release every native
        // handle. Committed WAL remains part of stopped-directory backups.
        let _permits = self
            .0
            .permits
            .acquire_many(CONNECTIONS)
            .await
            .map_err(|_| Error::Closed)?;
        let idle = std::mem::take(&mut *self.0.idle.lock().expect("Turso pool poisoned"));
        let mut failure = None;
        for connection in idle {
            let cleaned = async {
                if !connection.raw.is_autocommit()? {
                    connection.raw.execute("ROLLBACK", ()).await?;
                }
                Ok::<(), turso::Error>(())
            }
            .await;
            if let Err(error) = cleaned {
                failure = Some(error);
            }
        }
        self.0.database.lock().expect("Turso pool poisoned").take();
        self.0
            .lifetime_guard
            .lock()
            .expect("Turso pool poisoned")
            .take();
        match failure {
            Some(error) => Err(error.into()),
            None if self.0.cleanup_failed.load(Ordering::Acquire) => Err(Error::Cleanup),
            None => Ok(()),
        }
    }
}

async fn probe_connection(connection: &turso::Connection) -> turso::Result<()> {
    // Native execute rejects row-producing statements; query and drain this
    // health check so cancellation cleanup cannot poison a healthy connection.
    let mut rows = connection.query("SELECT 1", ()).await?;
    while rows.next().await?.is_some() {}
    Ok(())
}

pub struct Connection {
    raw: turso::Connection,
    interrupted: bool,
    managed_transaction: bool,
}
impl Connection {
    fn check_ready(&mut self) -> Result<()> {
        // A caller may catch a query error or cancel its future while keeping
        // the lease. Neither permits the next write to escape its transaction.
        if self.interrupted {
            return Err(Error::Unfinished);
        }
        if self.managed_transaction && self.raw.is_autocommit()? {
            return Err(Error::TransactionAborted);
        }
        Ok(())
    }
}

pub struct PoolConnection {
    connection: Option<Connection>,
    pool: Arc<PoolState>,
    _permit: Option<OwnedSemaphorePermit>,
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
        if !connection.interrupted && connection.raw.is_autocommit().unwrap_or(false) {
            connection.managed_transaction = false;
            self.pool
                .idle
                .lock()
                .expect("Turso pool poisoned")
                .push(connection);
            return;
        }
        // Rust has no async Drop. This is bounded pool housekeeping, never a
        // second durable worker or a retry loop: the task retains its existing
        // permit, and shutdown waits for every such permit before releasing DB.
        // Simply deferring cleanup to this pool's next acquire would retain a
        // write lock while a different handle to the same file is waiting.
        let pool = self.pool.clone();
        let permit = self._permit.take();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let _cleanup = runtime.spawn(async move {
                let cleaned = async {
                    if !connection.raw.is_autocommit()? {
                        connection.raw.execute("ROLLBACK", ()).await?;
                    }
                    if connection.interrupted {
                        probe_connection(&connection.raw).await?;
                    }
                    Ok::<(), turso::Error>(())
                }
                .await;
                if cleaned.is_ok() {
                    connection.interrupted = false;
                    connection.managed_transaction = false;
                    pool.idle
                        .lock()
                        .expect("Turso pool poisoned")
                        .push(connection);
                } else {
                    pool.cleanup_failed.store(true, Ordering::Release);
                    // Discard before releasing its permit, so shutdown cannot
                    // finish while a failed native connection is still alive.
                    drop(connection);
                }
                drop(permit);
            });
        } else {
            // Without a runtime, never return unverified state to a pool.
            pool.cleanup_failed.store(true, Ordering::Release);
            drop(connection);
            drop(permit);
        }
    }
}

pub struct Transaction {
    connection: PoolConnection,
}
impl Transaction {
    pub async fn commit(mut self) -> Result<()> {
        self.connection.check_ready()?;
        self.connection.interrupted = true;
        self.connection.raw.execute("COMMIT", ()).await?;
        self.connection.interrupted = false;
        self.connection.managed_transaction = false;
        Ok(())
    }
    pub async fn rollback(mut self) -> Result<()> {
        // Some native execution errors already roll the transaction back.
        // Only rollback may settle that state; commit must never claim success.
        if !self.connection.raw.is_autocommit()? {
            self.connection.interrupted = true;
            self.connection.raw.execute("ROLLBACK", ()).await?;
        }
        if self.connection.interrupted {
            probe_connection(&self.connection.raw).await?;
        }
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
    fn encode(self) -> Result<turso::Value>;
}
impl Encode for String {
    fn encode(self) -> Result<turso::Value> {
        Ok(turso::Value::Text(self))
    }
}
impl Encode for &str {
    fn encode(self) -> Result<turso::Value> {
        self.to_owned().encode()
    }
}
impl<T: Encode + Clone> Encode for &T {
    fn encode(self) -> Result<turso::Value> {
        (*self).clone().encode()
    }
}
impl Encode for Uuid {
    fn encode(self) -> Result<turso::Value> {
        Ok(turso::Value::Blob(self.as_bytes().to_vec()))
    }
}
impl Encode for Vec<u8> {
    fn encode(self) -> Result<turso::Value> {
        Ok(turso::Value::Blob(self))
    }
}
impl Encode for bool {
    fn encode(self) -> Result<turso::Value> {
        Ok(turso::Value::Integer(i64::from(self)))
    }
}
impl Encode for f64 {
    fn encode(self) -> Result<turso::Value> {
        Ok(turso::Value::Real(self))
    }
}
impl Encode for serde_json::Value {
    fn encode(self) -> Result<turso::Value> {
        self.to_string().encode()
    }
}
impl<T: Encode> Encode for Option<T> {
    fn encode(self) -> Result<turso::Value> {
        self.map_or(Ok(turso::Value::Null), Encode::encode)
    }
}
macro_rules! encode_integer { ($($ty:ty),+) => { $(impl Encode for $ty {
    fn encode(self) -> Result<turso::Value> {
        Ok(turso::Value::Integer(i64::try_from(self).map_err(|_| Error::Decode("integer overflow"))?))
    }
})+ }; }
encode_integer!(u64, usize);
impl Encode for i64 {
    fn encode(self) -> Result<turso::Value> {
        Ok(turso::Value::Integer(self))
    }
}
impl Encode for i32 {
    fn encode(self) -> Result<turso::Value> {
        i64::from(self).encode()
    }
}
impl Encode for u32 {
    fn encode(self) -> Result<turso::Value> {
        i64::from(self).encode()
    }
}

pub trait Decode<'a>: Sized {
    fn decode(value: &'a turso::Value) -> Result<Self>;
}
impl<'a> Decode<'a> for &'a str {
    fn decode(value: &'a turso::Value) -> Result<Self> {
        match value {
            turso::Value::Text(value) => Ok(value),
            _ => Err(Error::Decode("expected text")),
        }
    }
}
impl Decode<'_> for String {
    fn decode(value: &turso::Value) -> Result<Self> {
        <&str>::decode(value).map(str::to_owned)
    }
}
impl Decode<'_> for Uuid {
    fn decode(value: &turso::Value) -> Result<Self> {
        match value {
            turso::Value::Blob(value) => {
                Uuid::from_slice(value).map_err(|_| Error::Decode("expected 16-byte UUID"))
            }
            _ => Err(Error::Decode("expected UUID blob")),
        }
    }
}
impl Decode<'_> for Vec<u8> {
    fn decode(value: &turso::Value) -> Result<Self> {
        match value {
            turso::Value::Blob(value) => Ok(value.clone()),
            _ => Err(Error::Decode("expected blob")),
        }
    }
}
impl Decode<'_> for i64 {
    fn decode(value: &turso::Value) -> Result<Self> {
        match value {
            turso::Value::Integer(value) => Ok(*value),
            _ => Err(Error::Decode("expected integer")),
        }
    }
}
macro_rules! decode_integer { ($($ty:ty),+) => { $(impl Decode<'_> for $ty {
    fn decode(value: &turso::Value) -> Result<Self> {
        Self::try_from(i64::decode(value)?).map_err(|_| Error::Decode("integer overflow"))
    }
})+ }; }
decode_integer!(i32, u32, u64, usize);
impl Decode<'_> for bool {
    fn decode(value: &turso::Value) -> Result<Self> {
        match i64::decode(value)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error::Decode("expected boolean")),
        }
    }
}
impl Decode<'_> for f64 {
    fn decode(value: &turso::Value) -> Result<Self> {
        match value {
            turso::Value::Real(value) => Ok(*value),
            turso::Value::Integer(value) => Ok(*value as f64),
            _ => Err(Error::Decode("expected number")),
        }
    }
}
impl Decode<'_> for serde_json::Value {
    fn decode(value: &turso::Value) -> Result<Self> {
        serde_json::from_str(<&str>::decode(value)?).map_err(|_| Error::Decode("invalid JSON"))
    }
}
impl<'a, T: Decode<'a>> Decode<'a> for Option<T> {
    fn decode(value: &'a turso::Value) -> Result<Self> {
        match value {
            turso::Value::Null => Ok(None),
            _ => T::decode(value).map(Some),
        }
    }
}

#[derive(Debug)]
pub struct Row {
    names: Arc<Vec<String>>,
    values: Vec<turso::Value>,
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
    params: Result<Vec<turso::Value>>,
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
            connection.check_ready()?;
            connection.interrupted = true;
            let result = connection.raw.execute(sql, params?).await?;
            connection.interrupted = false;
            Ok(QueryResult(result))
        })
    }
}
impl<T: FromRow + Send + 'static> Query<T> {
    pub fn fetch_all<'a>(self, executor: impl Into<Executor<'a>>) -> DriverFuture<'a, Vec<T>> {
        let executor = executor.into();
        Box::pin(async move {
            let mut connection = executor.connection().await?;
            connection.check_ready()?;
            connection.interrupted = true;
            let diagnostic = self.sql.starts_with("SELECT c.id,c.asset_id,c.version ")
                && self.sql.contains("l.surface='full_text'");
            if diagnostic {
                eprintln!("offline diagnostic: candidate raw query before");
            }
            let query_future = connection.raw.query(self.sql, self.params?);
            if diagnostic {
                eprintln!(
                    "offline diagnostic: candidate raw future constructed, bytes={}",
                    std::mem::size_of_val(&query_future)
                );
            }
            let mut rows = query_future.await?;
            if diagnostic {
                eprintln!("offline diagnostic: candidate raw query after");
            }
            let names = Arc::new(rows.column_names());
            let mut output = Vec::new();
            if diagnostic {
                eprintln!("offline diagnostic: candidate rows next before");
            }
            while let Some(row) = rows.next().await? {
                let values = (0..row.column_count())
                    .map(|index| row.get_value(index))
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                output.push(T::from_row(&Row {
                    names: names.clone(),
                    values,
                })?);
            }
            if diagnostic {
                eprintln!("offline diagnostic: candidate rows next after");
            }
            drop(rows);
            connection.interrupted = false;
            Ok(output)
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
            connection.check_ready()?;
            connection.interrupted = true;
            connection.raw.execute_batch(self.0).await?;
            connection.interrupted = false;
            Ok(QueryResult(0))
        })
    }
}

const FORMAT_SCHEMA: &str = "CREATE TABLE oc_storage_format (id INTEGER PRIMARY KEY CHECK (id=1), engine TEXT NOT NULL, role TEXT NOT NULL, schema_version INTEGER NOT NULL)";
const ENGINE_FORMAT: &str = "origence-native-turso-0.8.2";

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
            "missing or incompatible native Turso format marker",
        ));
    }
    let rows: Vec<(i64, String, String, i64)> =
        query_as("SELECT id,engine,role,schema_version FROM oc_storage_format")
            .fetch_all(conn)
            .await?;
    if rows != vec![(1, ENGINE_FORMAT.into(), role.into(), 1)] {
        return Err(Error::Incompatible(
            "incompatible native Turso format marker",
        ));
    }
    Ok(())
}

/// Token-preserving schema comparison. Only differences observed and verified
/// against Turso 0.8.2 are admitted: CREATE's IF NOT EXISTS display guard and
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
            return Err(Error::Incompatible("invalid embedded Turso schema"));
        }
        let name = tokens
            .get(index)
            .ok_or(Error::Incompatible("invalid embedded Turso schema"))?
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
            "incompatible native Turso schema; no migration was attempted",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let schema = include_str!("sqlite-schema.sql");
        let objects = schema_objects(schema).unwrap();
        let trigger = objects
            .iter()
            .find(|(name, _)| name == "oc_workspace_owner_immutable")
            .unwrap();
        assert!(
            trigger
                .1
                .contains(&"'workspace ownership is immutable'".into())
        );
        assert_eq!(trigger.1.last().unwrap(), "end");
        assert!(objects.iter().any(|(name, _)| name == "oc_artifact_ledger"));
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
            assert!(matches!(error, Err(Error::Native(_))));
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
    async fn pending_write_cancellation_rolls_back_before_pool_reuse() {
        use std::future::poll_fn;
        use std::task::Poll;
        fn pool(database: turso::Database) -> Pool {
            Pool(Arc::new(PoolState {
                database: Mutex::new(Some(Arc::new(database))),
                idle: Mutex::new(Vec::new()),
                permits: Arc::new(Semaphore::new(CONNECTIONS as usize)),
                closed: AtomicBool::new(false),
                cleanup_failed: AtomicBool::new(false),
                read_only: false,
                lifetime_guard: Mutex::new(None),
            }))
        }
        for explicit_rollback in [false, true] {
            // The SDK's deferred-completion VFS forces an actual Pending. It is
            // not evidence for file-backed mid-I/O cancellation or power failure.
            #[allow(clippy::arc_with_non_send_sync)]
            let io = Arc::new(turso::core::MemoryYieldIO::new());
            let database = turso::Builder::new_local("adapter-cancellation.db")
                .with_io_impl(io.clone())
                .build()
                .await
                .unwrap();
            let first = pool(database);
            raw_sql(include_str!("sqlite-schema.sql"))
                .execute(&first)
                .await
                .unwrap();
            query("INSERT INTO oc_workspaces(tenant_id,id,name,created_at) VALUES(zeroblob(16),zeroblob(16),'fixture',1)").execute(&first).await.unwrap();
            query("INSERT INTO oc_jobs(tenant_id,workspace_id,id,created_by,operation,payload,created_at,updated_at) VALUES(zeroblob(16),zeroblob(16),zeroblob(16),zeroblob(16),'fixture','{}',1,1)").execute(&first).await.unwrap();
            first.shutdown().await.unwrap();
            drop(first);
            let database = turso::Builder::new_local("adapter-cancellation.db")
                .with_io_impl(io)
                .build()
                .await
                .unwrap();
            let reopened = pool(database);
            let mut tx = reopened.begin_with("BEGIN IMMEDIATE").await.unwrap();
            let mut operation =
                Box::pin(query("UPDATE oc_jobs SET generation=999").execute(&mut *tx));
            let first_poll = poll_fn(|cx| Poll::Ready(operation.as_mut().poll(cx))).await;
            assert!(
                matches!(first_poll, Poll::Pending),
                "write must yield to exercise cancellation"
            );
            drop(operation);
            assert!(matches!(
                query("UPDATE oc_jobs SET generation=888")
                    .execute(&mut *tx)
                    .await,
                Err(Error::Unfinished)
            ));
            assert!(matches!(
                query_scalar::<i64>("SELECT generation FROM oc_jobs")
                    .fetch_one(&mut *tx)
                    .await,
                Err(Error::Unfinished)
            ));
            if explicit_rollback {
                tx.rollback().await.unwrap();
            } else {
                // Refused commit consumes the transaction, so Drop still cleans it.
                assert!(matches!(tx.commit().await, Err(Error::Unfinished)));
            }
            assert_eq!(
                query_scalar::<i64>("SELECT generation FROM oc_jobs")
                    .fetch_one(&reopened)
                    .await
                    .unwrap(),
                1
            );
            let mut tx = reopened.begin_with("BEGIN IMMEDIATE").await.unwrap();
            query("UPDATE oc_jobs SET generation=2")
                .execute(&mut *tx)
                .await
                .unwrap();
            tx.commit().await.unwrap();
            assert_eq!(
                query_scalar::<i64>("SELECT generation FROM oc_jobs")
                    .fetch_one(&reopened)
                    .await
                    .unwrap(),
                2
            );
            reopened.shutdown().await.unwrap();
        }
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
        drop(pool);
        assert!(!dropped.load(Ordering::Acquire));
        drop(tx);
        // Current-thread cleanup cannot run before this task yields.
        assert!(!dropped.load(Ordering::Acquire));
        tokio::time::timeout(Duration::from_secs(6), async {
            while !dropped.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
}
