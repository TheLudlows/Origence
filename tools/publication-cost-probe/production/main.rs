//! Executes the actual public authority receipt method from a selected Origence
//! path dependency. No copied publication algorithm, Service, model, or Lance IO.
use anyhow::{Context, Result, ensure};
use origence::messages::{AcceptedMessageSpec, IngestReceipt, PublicationStatus};
use origence::storage::native_turso as db;
use origence::storage::sqlite::TursoStore;
use origence::storage::{AuthorizedScope, DomainTx, Lifecycle, RelationalStore, Scope};
use serde_json::json;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;
use uuid::Uuid;

fn report(value: serde_json::Value) -> Result<()> {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{value}")?;
    out.flush()?;
    Ok(())
}
async fn status(store: &TursoStore, auth: &AuthorizedScope, receipt: &IngestReceipt) -> Result<PublicationStatus> {
    let mut tx = store.begin(auth.clone()).await?;
    let result = tx.message_batch_status(receipt).await;
    match result {
        Ok(status) => { tx.commit().await?; Ok(status) }
        Err(error) => { let _ = tx.rollback().await; Err(error.into()) }
    }
}

async fn seed(store: &TursoStore, count: usize) -> Result<(AuthorizedScope, IngestReceipt, usize)> {
    let scope = Scope { tenant_id: Uuid::from_u128(1), workspace_id: Uuid::from_u128(2) };
    store.create_workspace(scope.tenant_id, scope.workspace_id, "publication fixture").await?;
    let key = store.issue_key(scope, "admin").await?;
    let auth = store.authenticate(&key.token).await?;
    let source_content = tokio::task::spawn_blocking(move || {
        let messages: Vec<_> = (0..count).map(|_| json!({"role":"user","content":"a".repeat(2400)})).collect();
        json!({"batch_id":"fixture", "session_id":"fixture", "messages":messages}).to_string()
    }).await?;
    let mut tx = store.begin(auth.clone()).await?;
    let source = tx.create_event("message_batch_v1", &source_content, None).await?;
    let asset = tx.insert_knowledge_asset("publication fixture").await?;
    tx.commit().await?;
    let receipt = IngestReceipt {
        batch_id: "fixture".into(), session_id: "fixture".into(), asset_id: asset,
        source_event_id: source, job_id: Uuid::from_u128(3), target_version: 1,
        accepted_spec: AcceptedMessageSpec::raw_only("fixture"),
    };
    // Large fixture construction/serialization is outside both the read timer
    // and every database transaction, on a blocking thread.
    let spec = receipt.accepted_spec.clone();
    let payload = tokio::task::spawn_blocking(move || {
        let chunks: Vec<_> = (0..count).map(|index| json!({
            "id":Uuid::from_u128(1000 + index as u128),
            "chunk":{"content":"a".repeat(2400),"locator":{"parser":"message-batch-ranges-v1","source_event_id":source,"message_index":index,"session_id":"fixture"}},
            "embedding":vec![0.012345679_f32;4096],"summary":"","search_terms":"fixture"
        })).collect();
        json!({"schema_version":1,"expected_version":null,"accepted_spec":spec,
            "publication":{"generation":1,"profile":"fixture","summary_model":null,"graph":null,
                "memories":[{"asset":asset,"fact_key":null,"expected_version":null,"version":1,"chunks":chunks}]}
        }).to_string()
    }).await?;
    let payload_bytes = payload.len();
    let mut tx = store.pool().begin_with("BEGIN IMMEDIATE").await?;
    db::query("INSERT INTO oc_versions(tenant_id,workspace_id,asset_id,version,content,content_hash,source_event_id,title,created_by,created_at) VALUES(?,?,?,1,'fixture','fixture',?,'fixture',?,1)")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(asset).bind(source).bind(auth.principal_id)
        .execute(&mut *tx).await?;
    db::query("UPDATE oc_assets SET current_version=1 WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(asset).execute(&mut *tx).await?;
    db::query("INSERT INTO oc_jobs(tenant_id,workspace_id,id,created_by,operation,payload,state,asset_id,source_event_id,created_at,updated_at) VALUES(?,?,?,?,'message_batch_ingest',?,'completed',?,?,1,1)")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(receipt.job_id).bind(auth.principal_id)
        .bind(payload).bind(asset).bind(source).execute(&mut *tx).await?;
    for index in 0..count {
        let id = Uuid::from_u128(1000 + index as u128);
        db::query("INSERT INTO oc_chunks(tenant_id,workspace_id,id,asset_id,version,ordinal,content,locator,search_terms) VALUES(?,?,?,?,1,?,'fixture','{}','fixture')")
            .bind(scope.tenant_id).bind(scope.workspace_id).bind(id).bind(asset).bind(index)
            .execute(&mut *tx).await?;
        db::query("INSERT INTO oc_index_entries(tenant_id,workspace_id,artifact_id,field,model_id,dimension,generation,state,created_at) VALUES(?,?,?,'content','fixture',4096,1,'ready',1)")
            .bind(scope.tenant_id).bind(scope.workspace_id).bind(id).execute(&mut *tx).await?;
        for surface in ["vector", "full_text"] {
            db::query("INSERT INTO oc_artifact_ledger(tenant_id,workspace_id,source_id,version,artifact_type,artifact_id,surface,generation,idempotency_key,state,created_at,updated_at) VALUES(?,?,?,1,'chunk',?,?,1,?,'committed',1,1)")
                .bind(scope.tenant_id).bind(scope.workspace_id).bind(source).bind(id).bind(surface)
                .bind(format!("{id}:{surface}")).execute(&mut *tx).await?;
        }
    }
    tx.commit().await?;
    Ok((auth, receipt, payload_bytes))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    ensure!(args.len() == 3 || args.len() == 5, "usage: production-probe LABEL NEW-WORK-DIR [WARMUPS REPEATS]");
    let warmups: usize = args.get(3).map_or(Ok(2), |v| v.parse())?;
    let repeats: usize = args.get(4).map_or(Ok(5), |v| v.parse())?;
    ensure!((1..=10).contains(&warmups) && (3..=21).contains(&repeats), "invalid repetition bounds");
    let work = PathBuf::from(&args[2]);
    std::fs::create_dir(&work).context("fixture directory must be new")?;
    for count in [50usize, 100, 200] {
        let store = TursoStore::open(work.join(format!("authority-{count}.db"))).await?;
        let (auth, receipt, payload_bytes) = seed(&store, count).await?;
        let mut timings = Vec::new();
        for round in 0..warmups + repeats {
            let started = Instant::now();
            ensure!(status(&store, &auth, &receipt).await? == PublicationStatus::Retrievable, "valid publication not retrievable");
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            let warmup = round < warmups;
            if !warmup { timings.push(elapsed_ms); }
            report(json!({"event":"production_sample","label":args[1],"chunks":count,"dimension":4096,"payload_bytes":payload_bytes,"round":round,"warmup":warmup,"elapsed_ms":elapsed_ms,"sql_queries":null,"sql_query_note":"not instrumented; exact public method including begin/commit and permission checks","status":"retrievable"}))?;
        }
        let mut ordered = timings.clone();
        ordered.sort_by(f64::total_cmp);
        let middle = ordered.len()/2;
        let median = if ordered.len().is_multiple_of(2) { (ordered[middle-1]+ordered[middle])/2.0 } else { ordered[middle] };
        let last = Uuid::from_u128(999 + count as u128);
        db::query("UPDATE oc_artifact_ledger SET state='orphan' WHERE artifact_id=? AND surface='full_text'")
            .bind(last).execute(store.pool()).await?;
        ensure!(status(&store, &auth, &receipt).await? == PublicationStatus::Invisible, "missing full_text witness was exposed");
        db::query("UPDATE oc_artifact_ledger SET state='committed' WHERE artifact_id=? AND surface='full_text'")
            .bind(last).execute(store.pool()).await?;
        ensure!(status(&store, &auth, &receipt).await? == PublicationStatus::Retrievable, "restored witness not visible");
        db::query("UPDATE oc_events SET state='retracted' WHERE id=?")
            .bind(receipt.source_event_id).execute(store.pool()).await?;
        ensure!(status(&store, &auth, &receipt).await? == PublicationStatus::Invisible, "withdrawn source was exposed");
        report(json!({"event":"production_summary","label":args[1],"chunks":count,"payload_bytes":payload_bytes,"samples_ms":timings,"median_ms":median,"warmups":warmups,"repeats":repeats,"missing_full_text":"PASS","source_withdrawal":"PASS","scope":"actual authority receipt API, excludes Service native availability gate, models and Lance"}))?;
        store.shutdown().await?;
    }
    Ok(())
}
