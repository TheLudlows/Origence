//! Synthetic T0 SQL bridge, never linked into Origence's application.
//!
//! The separate process is deliberate crash isolation. The Python driver owns
//! lifetime/deadlines and kills this process only against its temporary fixture.
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::collections::{HashMap, hash_map::Entry};
use std::future::{Future, poll_fn};
use std::io::{BufRead, Write};
use std::path::Path;
use std::task::Poll;
use turso::{Builder, Connection, transaction::TransactionBehavior};

fn params(request: &Value) -> Result<Vec<turso::Value>> {
    request["params"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|value| {
            Ok(match value {
                Value::Null => turso::Value::Null,
                Value::Bool(v) => turso::Value::Integer(i64::from(*v)),
                Value::String(v) => turso::Value::Text(v.clone()),
                Value::Number(v) if v.is_i64() => turso::Value::Integer(v.as_i64().unwrap()),
                Value::Number(v) => turso::Value::Real(v.as_f64().context("invalid float")?),
                Value::Object(v) if v.contains_key("blob") => {
                    let hex = v["blob"].as_str().context("blob must be hex")?;
                    ensure!(hex.len() % 2 == 0 && hex.is_ascii(), "invalid blob hex");
                    let bytes = (0..hex.len())
                        .step_by(2)
                        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    turso::Value::Blob(bytes)
                }
                _ => bail!("unsupported parameter"),
            })
        })
        .collect()
}

fn row_json(row: &turso::Row) -> Result<Value> {
    let values = (0..row.column_count())
        .map(|index| {
            Ok(match row.get_value(index)? {
                turso::Value::Null => Value::Null,
                turso::Value::Integer(v) => json!(v),
                turso::Value::Real(v) => json!(v),
                turso::Value::Text(v) => json!(v),
                turso::Value::Blob(v) => {
                    let hex = v
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>();
                    json!({"blob": hex})
                }
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!(values))
}

fn error_category(error: &anyhow::Error) -> &'static str {
    match error.downcast_ref::<turso::Error>() {
        Some(turso::Error::Constraint(_)) => "constraint",
        Some(turso::Error::Busy(_) | turso::Error::BusySnapshot(_)) => "busy",
        Some(turso::Error::Interrupt(_)) => "interrupt",
        Some(_) => "engine_other",
        None => "bridge",
    }
}

async fn request(conn: &mut Connection, request: &Value) -> Result<Value> {
    let sql = request["sql"].as_str().context("missing sql")?;
    let parameters = params(request)?;
    match request["op"].as_str().unwrap_or("query") {
        "execute" => Ok(json!({"changed": conn.execute(sql, parameters).await?})),
        "query" => {
            let mut rows = conn.query(sql, parameters).await?;
            let columns = rows.column_names();
            let mut output = Vec::new();
            while let Some(row) = rows.next().await? {
                ensure!(output.len() < 10_000, "probe result row limit exceeded");
                output.push(row_json(&row)?);
            }
            Ok(json!({"columns": columns, "rows": output}))
        }
        "transaction_drop" => {
            let transaction = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .await?;
            transaction.execute(sql, parameters).await?;
            drop(transaction);
            Ok(json!({"dropped": true, "rollback": "deferred until connection reuse"}))
        }
        "returning_drop" => {
            let mut rows = conn.query(sql, parameters).await?;
            let take = request["take"].as_u64().unwrap_or(1).min(100);
            let mut output = Vec::new();
            for _ in 0..take {
                match rows.next().await? {
                    Some(row) => output.push(row_json(&row)?),
                    None => break,
                }
            }
            drop(rows);
            Ok(json!({"consumed": output, "dropped": true}))
        }
        "poll_cancel" => {
            let mut operation = Box::pin(conn.execute(sql, parameters));
            let first_poll = poll_fn(|cx| Poll::Ready(operation.as_mut().poll(cx))).await;
            drop(operation);
            match first_poll {
                Poll::Pending => Ok(json!({"interrupted_at": "pending_after_first_poll"})),
                Poll::Ready(result) => {
                    Ok(json!({"interrupted_at": "completed_before_cancel", "changed": result?}))
                }
            }
        }
        _ => bail!("unknown operation"),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    ensure!(
        args.len() == 3,
        "usage: origence-t0-turso-probe create|open PATH"
    );
    let path = Path::new(&args[2]);
    match args[1].as_str() {
        "create" => ensure!(!path.exists(), "refusing to overwrite an existing database"),
        "open" => ensure!(path.is_file(), "open requires an existing database"),
        _ => bail!("expected create or open"),
    }
    let database = Builder::new_local(&args[2])
        .experimental_index_method(true)
        .build()
        .await?;
    let mut connections = HashMap::<String, Connection>::new();
    let mut stdout = std::io::stdout().lock();
    writeln!(
        stdout,
        "{}",
        json!({"ready": true, "sdk": "0.8.2", "fts": true})
    )?;
    stdout.flush()?;
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        ensure!(line.len() <= 4 * 1024 * 1024, "request too large");
        let input: Value = serde_json::from_str(&line)?;
        let name = input["connection"].as_str().unwrap_or("default").to_owned();
        let conn = match connections.entry(name) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let conn = database.connect()?;
                conn.execute("PRAGMA foreign_keys = ON", ()).await?;
                entry.insert(conn)
            }
        };
        let response = match request(conn, &input).await {
            Ok(result) => json!({"ok": true, "result": result}),
            Err(error) => {
                json!({"ok": false, "error_category": error_category(&error), "error": format!("{error:#}")})
            }
        };
        writeln!(stdout, "{response}")?;
        stdout.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test(flavor = "current_thread")]
    async fn pending_real_job_update_drop_rollback_and_reuse() -> Result<()> {
        // The released SDK's byte-preserving memory VFS defers I/O completion.
        // This is cooperative cancellation coverage, not disk durability proof.
        // Builder requires Arc<dyn IO>; this current-thread test does not send it.
        #[allow(clippy::arc_with_non_send_sync)]
        let io = Arc::new(turso::core::MemoryYieldIO::new());
        let database = Builder::new_local("t0-cancellation.db")
            .with_io_impl(io.clone())
            .build()
            .await?;
        let conn = database.connect()?;
        conn.execute_batch(include_str!("../../../src/storage/sqlite-schema.sql"))
            .await?;
        conn.execute(
            "INSERT INTO oc_workspaces(tenant_id,id,name,created_at) VALUES(zeroblob(16),zeroblob(16),'fixture',1)",
            (),
        )
        .await?;
        conn.execute(
            "INSERT INTO oc_jobs(tenant_id,workspace_id,id,created_by,operation,payload,created_at,updated_at) VALUES(zeroblob(16),zeroblob(16),zeroblob(16),zeroblob(16),'fixture','{}',1,1)",
            (),
        )
        .await?;
        drop(conn);
        drop(database);
        // Reopen the same synthetic bytes with a cold page cache so updating
        // the real jobs table must pass through a deferred page read.
        let database = Builder::new_local("t0-cancellation.db")
            .with_io_impl(io)
            .build()
            .await?;
        let conn = database.connect()?;
        conn.execute("BEGIN IMMEDIATE", ()).await?;
        let mut operation = Box::pin(conn.execute("UPDATE oc_jobs SET generation=999", ()));
        let first = poll_fn(|cx| Poll::Ready(operation.as_mut().poll(cx))).await;
        drop(operation);
        ensure!(
            matches!(first, Poll::Pending),
            "write did not yield; cancellation was not exercised"
        );
        conn.execute("ROLLBACK", ()).await?;
        let mut rows = conn.query("SELECT generation FROM oc_jobs", ()).await?;
        let row = rows.next().await?.context("job missing after rollback")?;
        ensure!(row.get::<i64>(0)? == 1, "cancelled write escaped rollback");
        ensure!(rows.next().await?.is_none(), "unexpected extra job");
        drop(rows);
        conn.execute("BEGIN IMMEDIATE", ()).await?;
        conn.execute("UPDATE oc_jobs SET generation=2", ()).await?;
        conn.execute("COMMIT", ()).await?;
        let mut rows = conn.query("SELECT generation FROM oc_jobs", ()).await?;
        let row = rows.next().await?.context("job missing on reuse")?;
        ensure!(row.get::<i64>(0)? == 2, "connection was not reusable");
        Ok(())
    }
}
