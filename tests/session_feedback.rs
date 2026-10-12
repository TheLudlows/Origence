#![cfg(feature = "local-storage")]

use origence::{
    error::{AppError, Result},
    models::Models,
    service::Service,
    session::{FeedbackInput, SessionTurnInput},
    storage::{RelationalStore, Scope},
    types::AuthContext,
    worker,
};
use serde_json::{Value, json};
use uuid::Uuid;

fn id(value: &Value, field: &str) -> Uuid {
    serde_json::from_value(value[field].clone()).unwrap()
}

async fn provision(s: &Service) -> (AuthContext, String) {
    provision_scope(s, Uuid::new_v4(), Uuid::new_v4()).await
}

async fn provision_scope(
    s: &Service,
    tenant_id: Uuid,
    workspace_id: Uuid,
) -> (AuthContext, String) {
    let scope = Scope {
        tenant_id,
        workspace_id,
    };
    s.engine
        .relational()
        .create_workspace(tenant_id, workspace_id, "session regression")
        .await
        .unwrap();
    let key = s
        .engine
        .relational()
        .issue_key(scope, "admin")
        .await
        .unwrap();
    (s.auth(&key.token).await.unwrap(), key.token)
}

async fn role(s: &Service, a: &AuthContext, name: &str) -> AuthContext {
    let key = s.issue_key(a, name).await.unwrap();
    s.auth(key["token"].as_str().unwrap()).await.unwrap()
}

async fn finish(s: &Service, a: &AuthContext, receipt: &Value) {
    for _ in 0..100 {
        let job = s.job(a, id(receipt, "job_id")).await.unwrap();
        if job["state"] == "completed" {
            return;
        }
        assert_eq!(job["state"], "pending", "{job}");
        assert!(worker::process_next(s).await.unwrap());
    }
    panic!("fixture publication did not complete");
}

async fn publish(s: &Service, a: &AuthContext, key: &str, content: &str) -> Value {
    let receipt = s
        .memory(
            a,
            &Uuid::new_v4().to_string(),
            serde_json::from_value(json!({
                "fact_key":key,"content":content,"publish_if_authorized":true
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    finish(s, a, &receipt).await;
    receipt
}

async fn search(s: &Service, a: &AuthContext, query: &str) -> Value {
    s.search(
        a,
        serde_json::from_value(json!({
            "query":query,"mode":"keyword","limit":100,
            "components":{"summaries":false,"graph":false}
        }))
        .unwrap(),
    )
    .await
    .unwrap()
}

fn evidence(hit: &Value) -> Value {
    json!({
        "kind":"chunk","id":hit["chunk_id"],"asset_id":hit["asset_id"],
        "asset_version":hit["version"],"chunk_id":hit["chunk_id"]
    })
}

fn turn(turn_id: Uuid, used: Vec<Value>) -> Value {
    json!({
        "turn_id":turn_id,"question":"为何选择 Rust 🦀?\n原问题 é",
        "answer":"因为这些证据。\nKeep the caller's exact answer.","used_evidence":used
    })
}

fn feedback(feedback_id: Uuid, target: Value) -> Value {
    json!({
        "feedback_id":feedback_id,"target":target,"rating":"helpful",
        "comment":"确实有帮助 🦀 é"
    })
}

async fn archive(
    s: &Service,
    a: &AuthContext,
    key: &str,
    session: Uuid,
    input: Value,
) -> Result<Value> {
    s.archive_turn(a, key, session, serde_json::from_value(input).unwrap())
        .await
}

async fn record(
    s: &Service,
    a: &AuthContext,
    key: &str,
    session: Uuid,
    turn: Uuid,
    input: Value,
) -> Result<Value> {
    s.record_feedback(
        a,
        key,
        session,
        turn,
        serde_json::from_value(input).unwrap(),
    )
    .await
}

/// Count every relational domain table, rather than only the visible result.
/// A failed command must not leave an event, audit record, partial reference,
/// feedback row, command reservation, or queued work behind.
async fn counts(s: &Service, include_commands: bool) -> Vec<(String, i64)> {
    let pool = s.engine.relational().pool();
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' AND name LIKE 'oc_%' ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    let mut result = Vec::new();
    for name in names {
        if !include_commands && name == "oc_commands" {
            continue;
        }
        let query = format!("SELECT count(*) FROM \"{}\"", name.replace('"', "\"\""));
        let count = sqlx::query_scalar::<_, i64>(&query)
            .fetch_one(pool)
            .await
            .unwrap();
        result.push((name, count));
    }
    result
}

async fn count(s: &Service, table: &str) -> i64 {
    let query = format!("SELECT count(*) FROM {table}");
    sqlx::query_scalar(&query)
        .fetch_one(s.engine.relational().pool())
        .await
        .unwrap()
}

fn rejected(result: Result<Value>) {
    assert!(
        matches!(result, Err(AppError::NotFound | AppError::Invalid(_))),
        "expected an invalid or unavailable reference, got {result:?}"
    );
}

#[tokio::test]
async fn session_empty_evidence_is_explicit_and_archive_needs_no_models_or_worker() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s).await;
    let session = Uuid::new_v4();
    let turn_id = Uuid::new_v4();
    let input = turn(turn_id, vec![]);
    let mut missing = input.clone();
    missing.as_object_mut().unwrap().remove("used_evidence");
    assert!(serde_json::from_value::<SessionTurnInput>(missing).is_err());
    let mut null = input.clone();
    null["used_evidence"] = Value::Null;
    assert!(serde_json::from_value::<SessionTurnInput>(null).is_err());
    let mut untrusted = input.clone();
    untrusted["source_event_id"] = json!(Uuid::new_v4());
    assert!(serde_json::from_value::<SessionTurnInput>(untrusted).is_err());

    let receipt = archive(&s, &a, "empty-turn", session, input.clone())
        .await
        .unwrap();
    assert_eq!(receipt["state"], "archived");
    assert_eq!(receipt["session_id"], json!(session));
    assert_eq!(receipt["turn_id"], json!(turn_id));
    assert!(receipt.get("job_id").is_none());
    let view = s.session_turn(&a, session, turn_id).await.unwrap();
    assert_eq!(view["question"], input["question"]);
    assert_eq!(view["answer"], input["answer"]);
    assert_eq!(view["used_evidence"], json!([]));
    assert_eq!(view["feedback"], json!([]));
    assert_eq!(view["source_event_id"], receipt["source_event_id"]);
    assert!(view["created_at"].is_number());
    let source = s
        .source_event(&a, id(&receipt, "source_event_id"))
        .await
        .unwrap();
    let content: Value = serde_json::from_str(source["content"].as_str().unwrap()).unwrap();
    assert_eq!(content["question"], input["question"]);
    assert_eq!(content["answer"], input["answer"]);
    assert!(content.get("used_evidence").is_none());
    assert_eq!(count(&s, "oc_jobs").await, 0);
    assert_eq!(count(&s, "oc_assets").await, 0);
    assert_eq!(count(&s, "oc_chunks").await, 0);
    assert!(!worker::process_next(&s).await.unwrap());
}

#[tokio::test]
async fn session_archives_only_used_subset_with_server_resolved_historical_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s).await;
    let first = publish(&s, &a, "subset-one", "Atlas shared evidence first").await;
    let unused = publish(&s, &a, "subset-two", "Atlas shared evidence second").await;
    let hits = search(&s, &a, "shared").await;
    assert_eq!(hits["hits"].as_array().unwrap().len(), 2);
    let chosen = hits["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|hit| hit["asset_id"] == first["asset_id"])
        .unwrap();
    let reference = evidence(chosen);
    // A current-version replacement must not invalidate an explicitly selected
    // historical version whose own source and ownership are still active.
    let second = publish(&s, &a, "subset-one", "Atlas replacement version").await;
    assert_eq!(first["asset_id"], second["asset_id"]);
    assert_eq!(
        s.get(&a, id(&first, "asset_id"), None).await.unwrap()["version"],
        2
    );
    let session = Uuid::new_v4();
    let turn_id = Uuid::new_v4();
    let jobs = count(&s, "oc_jobs").await;
    archive(
        &s,
        &a,
        "subset",
        session,
        turn(turn_id, vec![reference.clone()]),
    )
    .await
    .unwrap();
    let view = s.session_turn(&a, session, turn_id).await.unwrap();
    let saved = view["used_evidence"].as_array().unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0]["reference"], reference);
    assert_eq!(saved[0]["source_event_id"], first["source_event_id"]);
    assert_eq!(saved[0]["source_version"], 1);
    assert_eq!(
        saved[0]["version_source_event_id"],
        first["source_event_id"]
    );
    assert_eq!(saved[0]["locator"], chosen["locator"]);
    assert!(!id(&saved[0], "evidence_ref_id").is_nil());
    assert!(
        !view
            .to_string()
            .contains(unused["asset_id"].as_str().unwrap())
    );
    assert_eq!(count(&s, "oc_jobs").await, jobs);
}

#[tokio::test]
async fn session_turn_and_feedback_replays_are_immutable_and_side_effect_free() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s).await;
    publish(&s, &a, "replay", "Atlas replay evidence").await;
    let hit = &search(&s, &a, "replay").await["hits"][0];
    let session = Uuid::new_v4();
    let turn_id = Uuid::new_v4();
    let input = turn(turn_id, vec![evidence(hit)]);
    let receipt = archive(&s, &a, "archive-replay", session, input.clone())
        .await
        .unwrap();
    let before = counts(&s, true).await;
    assert_eq!(
        archive(&s, &a, "archive-replay", session, input.clone())
            .await
            .unwrap(),
        receipt
    );
    assert_eq!(counts(&s, true).await, before);
    let domain_before = counts(&s, false).await;
    assert_eq!(
        archive(&s, &a, "new-command-same-turn", session, input.clone())
            .await
            .unwrap(),
        receipt
    );
    assert_eq!(counts(&s, false).await, domain_before);
    for field in ["question", "answer", "used_evidence"] {
        let mut changed = input.clone();
        changed[field] = if field == "used_evidence" {
            json!([])
        } else {
            json!("changed")
        };
        for key in ["archive-replay", "conflicting-domain-turn"] {
            let before = counts(&s, true).await;
            assert!(matches!(
                archive(&s, &a, key, session, changed.clone()).await,
                Err(AppError::Conflict(_))
            ));
            assert_eq!(counts(&s, true).await, before);
        }
    }
    let view = s.session_turn(&a, session, turn_id).await.unwrap();
    let feedback_id = Uuid::new_v4();
    let input = feedback(
        feedback_id,
        json!({
            "type":"evidence","evidence_ref_id":view["used_evidence"][0]["evidence_ref_id"]
        }),
    );
    let receipt = record(&s, &a, "feedback-replay", session, turn_id, input.clone())
        .await
        .unwrap();
    assert_eq!(receipt["state"], "recorded");
    assert_eq!(receipt["feedback_id"], json!(feedback_id));
    let before = counts(&s, true).await;
    assert_eq!(
        record(&s, &a, "feedback-replay", session, turn_id, input.clone())
            .await
            .unwrap(),
        receipt
    );
    assert_eq!(counts(&s, true).await, before);
    let before = counts(&s, false).await;
    assert_eq!(
        record(
            &s,
            &a,
            "new-command-same-feedback",
            session,
            turn_id,
            input.clone()
        )
        .await
        .unwrap(),
        receipt
    );
    assert_eq!(counts(&s, false).await, before);
    for field in ["rating", "comment", "target"] {
        let mut changed = input.clone();
        changed[field] = match field {
            "rating" => json!("unhelpful"),
            "target" => json!({"type":"turn"}),
            _ => json!("different comment"),
        };
        for key in ["feedback-replay", "conflicting-domain-feedback"] {
            let before = counts(&s, true).await;
            assert!(matches!(
                record(&s, &a, key, session, turn_id, changed.clone()).await,
                Err(AppError::Conflict(_))
            ));
            assert_eq!(counts(&s, true).await, before);
        }
    }
    let view = s.session_turn(&a, session, turn_id).await.unwrap();
    let rows = view["feedback"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["feedback_id"], json!(feedback_id));
    assert_eq!(rows[0]["target"], input["target"]);
    assert_eq!(rows[0]["rating"], input["rating"]);
    assert_eq!(rows[0]["comment"], input["comment"]);
    assert_eq!(
        s.session_feedback(&a, session, turn_id).await.unwrap()["feedback"],
        view["feedback"]
    );
}

#[tokio::test]
async fn session_reference_validation_rejects_forged_ownership_without_partial_writes() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s).await;
    publish(&s, &a, "reference-one", "Atlas first reference").await;
    publish(&s, &a, "reference-two", "Atlas second reference").await;
    let hits = search(&s, &a, "reference").await;
    let first = evidence(&hits["hits"][0]);
    let second = evidence(&hits["hits"][1]);
    let mut bad = Vec::new();
    for field in ["id", "asset_id", "chunk_id"] {
        let mut reference = first.clone();
        reference[field] = json!(Uuid::new_v4());
        bad.push(reference);
    }
    let mut wrong_asset = first.clone();
    wrong_asset["asset_id"] = second["asset_id"].clone();
    bad.push(wrong_asset);
    let mut wrong_chunk = first.clone();
    wrong_chunk["id"] = second["chunk_id"].clone();
    wrong_chunk["chunk_id"] = second["chunk_id"].clone();
    bad.push(wrong_chunk);
    let mut wrong_version = first.clone();
    wrong_version["asset_version"] = json!(2);
    bad.push(wrong_version);
    for (index, reference) in bad.into_iter().enumerate() {
        let before = counts(&s, true).await;
        rejected(
            archive(
                &s,
                &a,
                &format!("forged-{index}"),
                Uuid::new_v4(),
                turn(Uuid::new_v4(), vec![first.clone(), reference]),
            )
            .await,
        );
        assert_eq!(counts(&s, true).await, before);
    }
    let (foreign, _) = provision_scope(&s, a.tenant_id, Uuid::new_v4()).await;
    publish(&s, &foreign, "foreign-reference", "Atlas foreign evidence").await;
    let foreign_hit = search(&s, &foreign, "foreign").await;
    let before = counts(&s, true).await;
    rejected(
        archive(
            &s,
            &a,
            "cross-workspace-evidence",
            Uuid::new_v4(),
            turn(Uuid::new_v4(), vec![evidence(&foreign_hit["hits"][0])]),
        )
        .await,
    );
    assert_eq!(counts(&s, true).await, before);
    for extra in ["source_event_id", "locator", "evidence_ref_id"] {
        let mut forged = first.clone();
        forged[extra] = json!("caller cannot supply provenance");
        assert!(
            serde_json::from_value::<SessionTurnInput>(turn(Uuid::new_v4(), vec![forged])).is_err()
        );
    }
    let mut summary = first.clone();
    summary["kind"] = json!("summary");
    assert!(
        serde_json::from_value::<SessionTurnInput>(turn(Uuid::new_v4(), vec![summary])).is_err()
    );
}

#[tokio::test]
async fn session_feedback_target_is_bound_to_archived_turn_and_principal() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s).await;
    publish(&s, &a, "target", "Atlas target evidence").await;
    let used = evidence(&search(&s, &a, "target").await["hits"][0]);
    let session = Uuid::new_v4();
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    for turn_id in [first, second] {
        archive(
            &s,
            &a,
            &turn_id.to_string(),
            session,
            turn(turn_id, vec![used.clone()]),
        )
        .await
        .unwrap();
    }
    let first_view = s.session_turn(&a, session, first).await.unwrap();
    let foreign_ref = first_view["used_evidence"][0]["evidence_ref_id"].clone();
    for evidence_ref_id in [foreign_ref, json!(Uuid::new_v4()), used["chunk_id"].clone()] {
        let before = counts(&s, true).await;
        rejected(
            record(
                &s,
                &a,
                &Uuid::new_v4().to_string(),
                session,
                second,
                feedback(
                    Uuid::new_v4(),
                    json!({"type":"evidence","evidence_ref_id":evidence_ref_id}),
                ),
            )
            .await,
        );
        assert_eq!(counts(&s, true).await, before);
    }
    let feedback_id = Uuid::new_v4();
    let input = feedback(feedback_id, json!({"type":"turn"}));
    record(&s, &a, "feedback-domain", session, first, input.clone())
        .await
        .unwrap();
    assert!(matches!(
        record(&s, &a, "feedback-moved", session, second, input.clone()).await,
        Err(AppError::Conflict(_))
    ));
    // Feedback identity is per principal. A different authorized writer can
    // independently use the same client-generated feedback UUID.
    let writer = role(&s, &a, "writer").await;
    record(&s, &writer, "feedback-domain", session, second, input)
        .await
        .unwrap();
    for (turn_id, actor) in [(first, &a), (second, &writer)] {
        let rows = s.session_feedback(actor, session, turn_id).await.unwrap();
        assert_eq!(rows["feedback"].as_array().unwrap().len(), 1);
        assert_eq!(rows["feedback"][0]["feedback_id"], json!(feedback_id));
    }
    for target in [
        json!({"type":"asset"}),
        json!({"type":"evidence"}),
        json!({"type":"turn","evidence_ref_id":Uuid::new_v4()}),
    ] {
        assert!(serde_json::from_value::<FeedbackInput>(feedback(Uuid::new_v4(), target)).is_err());
    }
    let mut invalid_rating = feedback(Uuid::new_v4(), json!({"type":"turn"}));
    invalid_rating["rating"] = json!("approved");
    assert!(serde_json::from_value::<FeedbackInput>(invalid_rating).is_err());
}

#[tokio::test]
async fn session_scope_live_permissions_and_revoked_principals_gate_all_surfaces() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s).await;
    let writer = role(&s, &a, "writer").await;
    let reader = role(&s, &a, "reader").await;
    let (same_tenant, _) = provision_scope(&s, a.tenant_id, Uuid::new_v4()).await;
    let (other_tenant, _) = provision(&s).await;
    let session = Uuid::new_v4();
    let turn_id = Uuid::new_v4();
    let input = turn(turn_id, vec![]);
    let receipt = archive(&s, &writer, "private-turn", session, input.clone())
        .await
        .unwrap();
    let feedback = feedback(Uuid::new_v4(), json!({"type":"turn"}));
    record(
        &s,
        &writer,
        "private-feedback",
        session,
        turn_id,
        feedback.clone(),
    )
    .await
    .unwrap();
    // Turn identity is workspace/session/turn, independent of the writer key.
    let before = counts(&s, false).await;
    assert_eq!(
        archive(&s, &a, "private-turn", session, input.clone())
            .await
            .unwrap(),
        receipt
    );
    assert_eq!(counts(&s, false).await, before);
    // Another writer in the same workspace may inspect an authorized archive.
    assert_eq!(
        s.session_turn(&a, session, turn_id).await.unwrap()["source_event_id"],
        receipt["source_event_id"]
    );
    for other in [&same_tenant, &other_tenant] {
        assert!(matches!(
            s.session_turn(other, session, turn_id).await,
            Err(AppError::NotFound)
        ));
        assert!(matches!(
            s.session_feedback(other, session, turn_id).await,
            Err(AppError::NotFound)
        ));
        assert!(matches!(
            s.source_event(other, id(&receipt, "source_event_id")).await,
            Err(AppError::NotFound)
        ));
        assert!(matches!(
            record(
                &s,
                other,
                "cross-scope-feedback",
                session,
                turn_id,
                feedback.clone()
            )
            .await,
            Err(AppError::NotFound)
        ));
        // Session and turn IDs are scoped, not globally reserved by another tenant.
        archive(&s, other, "private-turn", session, input.clone())
            .await
            .unwrap();
        assert_ne!(
            s.session_turn(other, session, turn_id).await.unwrap()["source_event_id"],
            receipt["source_event_id"]
        );
    }
    for result in [
        s.session_turn(&reader, session, turn_id).await,
        s.session_feedback(&reader, session, turn_id).await,
        s.source_event(&reader, id(&receipt, "source_event_id"))
            .await,
        archive(&s, &reader, "reader-archive", session, input.clone()).await,
        record(
            &s,
            &reader,
            "reader-feedback",
            session,
            turn_id,
            feedback.clone(),
        )
        .await,
    ] {
        assert!(matches!(result, Err(AppError::Forbidden)), "{result:?}");
    }
    // A cached AuthContext must not turn stale permission into standing access.
    sqlx::query("UPDATE oc_api_keys SET role='reader' WHERE id=?")
        .bind(writer.id)
        .execute(s.engine.relational().pool())
        .await
        .unwrap();
    assert!(matches!(
        s.session_turn(&writer, session, turn_id).await,
        Err(AppError::Forbidden)
    ));
    assert!(matches!(
        archive(&s, &writer, "private-turn", session, input.clone()).await,
        Err(AppError::Forbidden)
    ));
    assert!(matches!(
        record(
            &s,
            &writer,
            "private-feedback",
            session,
            turn_id,
            feedback.clone()
        )
        .await,
        Err(AppError::Forbidden)
    ));
    s.revoke_key(&a, writer.id).await.unwrap();
    for result in [
        s.session_turn(&writer, session, turn_id).await,
        s.session_feedback(&writer, session, turn_id).await,
        s.source_event(&writer, id(&receipt, "source_event_id"))
            .await,
        archive(&s, &writer, "private-turn", session, input).await,
        record(&s, &writer, "private-feedback", session, turn_id, feedback).await,
    ] {
        assert!(matches!(result, Err(AppError::Forbidden)), "{result:?}");
    }
}

#[tokio::test]
async fn session_concurrent_repeats_and_reopen_keep_one_turn_and_one_feedback() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, token) = provision(&s).await;
    publish(&s, &a, "race", "Atlas race evidence").await;
    let hits = search(&s, &a, "race").await;
    let session = Uuid::new_v4();
    let turn_id = Uuid::new_v4();
    let input = turn(turn_id, vec![evidence(&hits["hits"][0])]);
    let before_events = count(&s, "oc_events").await;
    let before_jobs = count(&s, "oc_jobs").await;
    let (left, right) = tokio::join!(
        archive(&s, &a, "concurrent-a", session, input.clone()),
        archive(&s, &a, "concurrent-b", session, input.clone())
    );
    let receipt = left.unwrap();
    assert_eq!(right.unwrap(), receipt);
    assert_eq!(count(&s, "oc_session_turns").await, 1);
    assert_eq!(count(&s, "oc_session_turn_evidence").await, 1);
    assert_eq!(count(&s, "oc_events").await, before_events + 1);
    let saved = s.session_turn(&a, session, turn_id).await.unwrap();
    let feedback = feedback(
        Uuid::new_v4(),
        json!({
            "type":"evidence","evidence_ref_id":saved["used_evidence"][0]["evidence_ref_id"]
        }),
    );
    let (left, right) = tokio::join!(
        record(
            &s,
            &a,
            "feedback-concurrent-a",
            session,
            turn_id,
            feedback.clone()
        ),
        record(
            &s,
            &a,
            "feedback-concurrent-b",
            session,
            turn_id,
            feedback.clone()
        )
    );
    let feedback_receipt = left.unwrap();
    assert_eq!(right.unwrap(), feedback_receipt);
    assert_eq!(count(&s, "oc_feedback").await, 1);
    assert_eq!(count(&s, "oc_jobs").await, before_jobs);
    let saved = s.session_turn(&a, session, turn_id).await.unwrap();
    let before = counts(&s, true).await;
    s.engine.relational().pool().close().await;
    drop(s);
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let a = s.auth(&token).await.unwrap();
    assert_eq!(s.session_turn(&a, session, turn_id).await.unwrap(), saved);
    assert_eq!(
        archive(&s, &a, "concurrent-a", session, input)
            .await
            .unwrap(),
        receipt
    );
    assert_eq!(
        record(&s, &a, "feedback-concurrent-a", session, turn_id, feedback)
            .await
            .unwrap(),
        feedback_receipt
    );
    assert_eq!(counts(&s, true).await, before);
    assert!(!worker::process_next(&s).await.unwrap());
}

#[tokio::test]
async fn session_transaction_failure_rolls_back_mid_reference_and_feedback_audit() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s).await;
    publish(&s, &a, "rollback-one", "Atlas rollback first").await;
    publish(&s, &a, "rollback-two", "Atlas rollback second").await;
    let hits = search(&s, &a, "rollback").await;
    let session = Uuid::new_v4();
    let turn_id = Uuid::new_v4();
    let input = turn(
        turn_id,
        hits["hits"]
            .as_array()
            .unwrap()
            .iter()
            .map(evidence)
            .collect(),
    );
    assert_eq!(input["used_evidence"].as_array().unwrap().len(), 2);
    // Failure occurs after the source, turn, and first evidence insertion.
    sqlx::raw_sql("CREATE TRIGGER fail_second_session_reference BEFORE INSERT ON oc_session_turn_evidence WHEN NEW.ordinal=1 BEGIN SELECT RAISE(ABORT,'fixture reference failure'); END;")
        .execute(s.engine.relational().pool()).await.unwrap();
    let before = counts(&s, true).await;
    assert!(
        archive(&s, &a, "atomic-archive", session, input.clone())
            .await
            .is_err()
    );
    assert_eq!(counts(&s, true).await, before);
    assert!(matches!(
        s.session_turn(&a, session, turn_id).await,
        Err(AppError::NotFound)
    ));
    sqlx::raw_sql("DROP TRIGGER fail_second_session_reference;")
        .execute(s.engine.relational().pool())
        .await
        .unwrap();
    archive(&s, &a, "atomic-archive", session, input)
        .await
        .unwrap();
    assert_eq!(count(&s, "oc_session_turn_evidence").await, 2);

    // An audit failure must also roll back the feedback and command reservation.
    sqlx::raw_sql("CREATE TRIGGER fail_feedback_audit BEFORE INSERT ON oc_audit BEGIN SELECT RAISE(ABORT,'fixture audit failure'); END;")
        .execute(s.engine.relational().pool()).await.unwrap();
    let before = counts(&s, true).await;
    let input = feedback(Uuid::new_v4(), json!({"type":"turn"}));
    assert!(
        record(&s, &a, "atomic-feedback", session, turn_id, input.clone())
            .await
            .is_err()
    );
    assert_eq!(counts(&s, true).await, before);
    assert_eq!(
        s.session_feedback(&a, session, turn_id).await.unwrap()["feedback"],
        json!([])
    );
    sqlx::raw_sql("DROP TRIGGER fail_feedback_audit;")
        .execute(s.engine.relational().pool())
        .await
        .unwrap();
    record(&s, &a, "atomic-feedback", session, turn_id, input)
        .await
        .unwrap();
    assert_eq!(count(&s, "oc_feedback").await, 1);
}

async fn assert_hidden(
    s: &Service,
    a: &AuthContext,
    session: Uuid,
    input: &Value,
    receipt: &Value,
    feedback: &Value,
) {
    let turn_id = id(input, "turn_id");
    assert!(matches!(
        s.session_turn(a, session, turn_id).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        s.session_feedback(a, session, turn_id).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        s.source_event(a, id(receipt, "source_event_id")).await,
        Err(AppError::NotFound)
    ));
    let before = counts(s, true).await;
    let archive_key = format!("withdrawn-archive-{session}");
    for key in [archive_key.as_str(), "new-command-withdrawn-archive"] {
        assert!(matches!(
            archive(s, a, key, session, input.clone()).await,
            Err(AppError::NotFound)
        ));
    }
    let feedback_key = format!("withdrawn-feedback-{session}");
    for key in [feedback_key.as_str(), "new-command-withdrawn-feedback"] {
        assert!(matches!(
            record(s, a, key, session, turn_id, feedback.clone()).await,
            Err(AppError::NotFound)
        ));
    }
    assert!(matches!(
        record(
            s,
            a,
            "new-feedback-after-withdrawal",
            session,
            turn_id,
            self::feedback(Uuid::new_v4(), json!({"type":"turn"}))
        )
        .await,
        Err(AppError::NotFound)
    ));
    assert_eq!(counts(s, true).await, before);
}

#[tokio::test]
async fn session_dependency_withdrawal_hides_all_reads_and_replays_before_cleanup() {
    for target in ["event", "asset", "archive"] {
        let dir = tempfile::tempdir().unwrap();
        let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
        let (a, token) = provision(&s).await;
        let first = publish(&s, &a, "withdraw-one", "Atlas withdrawal first").await;
        publish(&s, &a, "withdraw-two", "Atlas withdrawal second").await;
        let hits = search(&s, &a, "withdrawal").await;
        let session = Uuid::new_v4();
        let turn_id = Uuid::new_v4();
        let input = turn(
            turn_id,
            hits["hits"]
                .as_array()
                .unwrap()
                .iter()
                .map(evidence)
                .collect(),
        );
        let receipt = archive(
            &s,
            &a,
            &format!("withdrawn-archive-{session}"),
            session,
            input.clone(),
        )
        .await
        .unwrap();
        let feedback = feedback(Uuid::new_v4(), json!({"type":"turn"}));
        record(
            &s,
            &a,
            &format!("withdrawn-feedback-{session}"),
            session,
            turn_id,
            feedback.clone(),
        )
        .await
        .unwrap();
        let (target_id, kind) = match target {
            "archive" => (id(&receipt, "source_event_id"), "event"),
            "asset" => (id(&first, "asset_id"), "asset"),
            _ => (id(&first, "source_event_id"), "event"),
        };
        let deleted = s
            .delete(&a, "withdraw-dependency", target_id, kind)
            .await
            .unwrap();
        assert_eq!(
            s.job(&a, id(&deleted, "cleanup_job_id")).await.unwrap()["state"],
            "pending"
        );
        assert_hidden(&s, &a, session, &input, &receipt, &feedback).await;
        if target != "archive" {
            let before = counts(&s, true).await;
            let mut new_turn = input.clone();
            new_turn["turn_id"] = json!(Uuid::new_v4());
            assert!(matches!(
                archive(
                    &s,
                    &a,
                    "new-turn-with-withdrawn-evidence",
                    session,
                    new_turn
                )
                .await,
                Err(AppError::NotFound)
            ));
            assert_eq!(counts(&s, true).await, before);
        }
        // Even though the other used reference remains active, no partial Q&A
        // or feedback list may leak through either read endpoint.
        while worker::process_next(&s).await.unwrap() {}
        assert_hidden(&s, &a, session, &input, &receipt, &feedback).await;
        let originals: i64 = sqlx::query_scalar("SELECT count(*) FROM oc_events WHERE id=?")
            .bind(id(&receipt, "source_event_id"))
            .fetch_one(s.engine.relational().pool())
            .await
            .unwrap();
        assert_eq!(
            originals, 1,
            "logical withdrawal must preserve original Q&A bytes"
        );
        s.engine.relational().pool().close().await;
        drop(s);
        let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
        let a = s.auth(&token).await.unwrap();
        assert_hidden(&s, &a, session, &input, &receipt, &feedback).await;
    }
}

#[tokio::test]
async fn session_deleted_file_invalidates_archived_evidence_before_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s).await;
    let upload = s
        .upload(
            &a,
            "session-upload",
            "evidence.txt",
            "text",
            "Atlas file evidence 原文 🦀".as_bytes(),
        )
        .await
        .unwrap();
    let published = s
        .knowledge(
            &a,
            "session-file-knowledge",
            serde_json::from_value(json!({
                "title":"File evidence","file_id":upload["file_id"],"format":"text"
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    finish(&s, &a, &published).await;
    let hits = search(&s, &a, "Atlas").await;
    assert_eq!(hits["hits"].as_array().unwrap().len(), 1);
    let session = Uuid::new_v4();
    let turn_id = Uuid::new_v4();
    let input = turn(turn_id, vec![evidence(&hits["hits"][0])]);
    let receipt = archive(
        &s,
        &a,
        &format!("withdrawn-archive-{session}"),
        session,
        input.clone(),
    )
    .await
    .unwrap();
    let feedback = feedback(Uuid::new_v4(), json!({"type":"turn"}));
    record(
        &s,
        &a,
        &format!("withdrawn-feedback-{session}"),
        session,
        turn_id,
        feedback.clone(),
    )
    .await
    .unwrap();
    let deleted = s
        .delete(&a, "delete-evidence-file", id(&upload, "file_id"), "file")
        .await
        .unwrap();
    assert_eq!(
        s.job(&a, id(&deleted, "cleanup_job_id")).await.unwrap()["state"],
        "pending"
    );
    assert_hidden(&s, &a, session, &input, &receipt, &feedback).await;
    while worker::process_next(&s).await.unwrap() {}
    assert_hidden(&s, &a, session, &input, &receipt, &feedback).await;
}

/// Graph support is relational source/version ownership in the current worker.
/// The worker stores graph owners with no single chunk_id; use the exact same
/// owner + committed-ledger shape on chunks published through the public API.
async fn graph_support(
    s: &Service,
    a: &AuthContext,
    source: Uuid,
    version: i32,
    kind: &str,
    artifact: Uuid,
    state: &str,
) {
    sqlx::query("INSERT INTO oc_artifact_owners(tenant_id,workspace_id,source_id,version,artifact_type,artifact_id,chunk_id,created_at) VALUES(?,?,?,?,?,?,NULL,0)")
        .bind(a.tenant_id).bind(a.workspace_id).bind(source).bind(version)
        .bind(kind).bind(artifact).execute(s.engine.relational().pool()).await.unwrap();
    sqlx::query("INSERT INTO oc_artifact_ledger(tenant_id,workspace_id,source_id,version,artifact_type,artifact_id,surface,generation,idempotency_key,state,created_at,updated_at) VALUES(?,?,?,?,?,?,'graph',1,?,?,0,0)")
        .bind(a.tenant_id).bind(a.workspace_id).bind(source).bind(version)
        .bind(kind).bind(artifact).bind(Uuid::new_v4().to_string()).bind(state)
        .execute(s.engine.relational().pool()).await.unwrap();
}

#[tokio::test]
async fn session_graph_evidence_requires_exact_support_kind_and_committed_ledger() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s).await;
    let first = publish(&s, &a, "graph-one", "Atlas graph first").await;
    let second = publish(&s, &a, "graph-two", "Atlas graph second").await;
    let hits = search(&s, &a, "graph").await;
    let first_hit = hits["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|hit| hit["asset_id"] == first["asset_id"])
        .unwrap();
    let second_hit = hits["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|hit| hit["asset_id"] == second["asset_id"])
        .unwrap();
    let entity = Uuid::new_v4();
    let relation = Uuid::new_v4();
    let pending = Uuid::new_v4();
    let other_version = Uuid::new_v4();
    for (kind, artifact, state, version) in [
        ("entity", entity, "committed", 1),
        ("relation", relation, "committed", 1),
        ("entity", pending, "pending", 1),
        ("entity", other_version, "committed", 2),
    ] {
        graph_support(
            &s,
            &a,
            id(&first, "source_event_id"),
            version,
            kind,
            artifact,
            state,
        )
        .await;
    }
    let reference = |kind: &str, artifact: Uuid, hit: &Value| {
        let mut value = evidence(hit);
        value["kind"] = json!(kind);
        value["id"] = json!(artifact);
        value
    };
    for invalid in [
        reference("entity", entity, second_hit),
        reference("relation", entity, first_hit),
        reference("entity", relation, first_hit),
        reference("entity", pending, first_hit),
        reference("entity", other_version, first_hit),
        reference("entity", Uuid::new_v4(), first_hit),
    ] {
        let before = counts(&s, true).await;
        rejected(
            archive(
                &s,
                &a,
                &Uuid::new_v4().to_string(),
                Uuid::new_v4(),
                turn(Uuid::new_v4(), vec![invalid]),
            )
            .await,
        );
        assert_eq!(counts(&s, true).await, before);
    }
    let session = Uuid::new_v4();
    let turn_id = Uuid::new_v4();
    let used = vec![
        evidence(first_hit),
        reference("entity", entity, first_hit),
        reference("relation", relation, first_hit),
    ];
    let input = turn(turn_id, used.clone());
    let receipt = archive(
        &s,
        &a,
        &format!("withdrawn-archive-{session}"),
        session,
        input.clone(),
    )
    .await
    .unwrap();
    let view = s.session_turn(&a, session, turn_id).await.unwrap();
    assert_eq!(view["used_evidence"].as_array().unwrap().len(), 3);
    for (saved, requested) in view["used_evidence"].as_array().unwrap().iter().zip(&used) {
        assert_eq!(&saved["reference"], requested);
        assert_eq!(saved["source_event_id"], first["source_event_id"]);
        assert_eq!(saved["locator"], first_hit["locator"]);
    }
    let feedback = feedback(
        Uuid::new_v4(),
        json!({
            "type":"evidence","evidence_ref_id":view["used_evidence"][1]["evidence_ref_id"]
        }),
    );
    record(
        &s,
        &a,
        &format!("withdrawn-feedback-{session}"),
        session,
        turn_id,
        feedback.clone(),
    )
    .await
    .unwrap();
    // Authoritative graph support can disappear before backend cleanup. Every
    // raw read and command replay revalidates the committed support then.
    sqlx::query("UPDATE oc_artifact_ledger SET state='orphan' WHERE artifact_id=?")
        .bind(entity)
        .execute(s.engine.relational().pool())
        .await
        .unwrap();
    assert_hidden(&s, &a, session, &input, &receipt, &feedback).await;
}

fn semantic_identity() -> Value {
    json!({"subject":{"kind":"service","stable_id":"Atlas"},"predicate":"service.languages","context":{"env":"production"}})
}

async fn semantic_member(
    s: &Service,
    a: &AuthContext,
    value: &str,
    operation: &str,
    version: i32,
) -> Value {
    let quote = format!("证据 🦀 é {value}");
    let content = format!("前言\n{quote}");
    let start = content.find(&quote).unwrap();
    let receipt = s.semantic_memory(a, &Uuid::new_v4().to_string(), serde_json::from_value(json!({
        "identity":semantic_identity(),"content":content,"operation":operation,"expected_version":version,
        "statements":[{"value":value,"quote":quote,"byte_start":start,"byte_end":start+quote.len()}]
    })).unwrap()).await.unwrap();
    finish(s, a, &receipt).await;
    receipt
}

#[tokio::test]
async fn session_s2_retains_original_member_provenance_and_checks_the_whole_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s).await;
    s.register_memory_property(
        &a,
        "session-language-property",
        serde_json::from_value(json!({
            "predicate":"service.languages","cardinality":"multi","required_context":["env"]
        }))
        .unwrap(),
    )
    .await
    .unwrap();
    let first = semantic_member(&s, &a, "Rust", "assert", 0).await;
    let second = semantic_member(&s, &a, "Python", "supplement", 1).await;
    assert_eq!(first["asset_id"], second["asset_id"]);
    let rust_hits = search(&s, &a, "Rust").await;
    let python_hits = search(&s, &a, "Python").await;
    assert_eq!(rust_hits["hits"].as_array().unwrap().len(), 1);
    assert_eq!(python_hits["hits"].as_array().unwrap().len(), 1);
    let mut archives = Vec::new();
    for hit in [&rust_hits["hits"][0], &python_hits["hits"][0]] {
        let session = Uuid::new_v4();
        let turn_id = Uuid::new_v4();
        let input = turn(turn_id, vec![evidence(hit)]);
        let receipt = archive(
            &s,
            &a,
            &format!("withdrawn-archive-{session}"),
            session,
            input.clone(),
        )
        .await
        .unwrap();
        let view = s.session_turn(&a, session, turn_id).await.unwrap();
        let saved = &view["used_evidence"][0];
        assert_eq!(saved["reference"]["asset_version"], 2);
        assert_eq!(saved["version_source_event_id"], second["source_event_id"]);
        assert_eq!(saved["source_event_id"], hit["source_event_id"]);
        assert_eq!(saved["source_version"], hit["locator"]["source_version"]);
        assert_eq!(saved["locator"], hit["locator"]);
        if hit["locator"]["semantic_value"] == "Rust" {
            assert_eq!(saved["source_event_id"], first["source_event_id"]);
            assert_eq!(saved["source_version"], 1);
            assert_ne!(saved["source_event_id"], saved["version_source_event_id"]);
        }
        let source = s
            .source_event(&a, id(saved, "source_event_id"))
            .await
            .unwrap();
        let locator = &saved["locator"];
        let content = source["content"].as_str().unwrap();
        let start = locator["byte_start"].as_u64().unwrap() as usize;
        let end = locator["byte_end"].as_u64().unwrap() as usize;
        assert_eq!(&content[start..end], hit["content"].as_str().unwrap());
        let feedback = feedback(Uuid::new_v4(), json!({"type":"turn"}));
        record(
            &s,
            &a,
            &format!("withdrawn-feedback-{session}"),
            session,
            turn_id,
            feedback.clone(),
        )
        .await
        .unwrap();
        archives.push((session, input, receipt, feedback));
    }
    let deleted = s
        .delete(
            &a,
            "withdraw-member",
            id(&first, "source_event_id"),
            "event",
        )
        .await
        .unwrap();
    assert_eq!(
        s.job(&a, id(&deleted, "cleanup_job_id")).await.unwrap()["state"],
        "pending"
    );
    // The Python-only archive is hidden too: Rust is an unselected supporting
    // member of its S2 snapshot, so the complete version is no longer valid.
    for (session, input, receipt, feedback) in &archives {
        assert_hidden(&s, &a, *session, input, receipt, feedback).await;
    }
    while worker::process_next(&s).await.unwrap() {}
    let rebuilt = semantic_member(&s, &a, "Python", "supplement", 2).await;
    assert_eq!(rebuilt["asset_id"], first["asset_id"]);
    assert_eq!(
        s.get(&a, id(&rebuilt, "asset_id"), None).await.unwrap()["version"],
        3
    );
    // Rebuilding a new valid version never rewrites the older archived version.
    for (session, input, receipt, feedback) in &archives {
        assert_hidden(&s, &a, *session, input, receipt, feedback).await;
    }
}

#[tokio::test]
async fn session_missing_schema_is_rejected_without_erasing_saved_sources() {
    for drop_all in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
        let (a, _) = provision(&s).await;
        let session = Uuid::new_v4();
        let input = turn(Uuid::new_v4(), vec![]);
        let receipt = archive(&s, &a, "schema-source", session, input.clone())
            .await
            .unwrap();
        let statement = if drop_all {
            "DROP TABLE oc_feedback; DROP TABLE oc_session_turn_evidence; DROP TABLE oc_session_turns;"
        } else {
            "DROP TABLE oc_feedback;"
        };
        sqlx::raw_sql(statement)
            .execute(s.engine.relational().pool())
            .await
            .unwrap();
        if drop_all {
            // Removing every session table must not turn an archived question
            // and answer into an unguarded legacy raw event in the live host.
            assert!(
                s.source_event(&a, id(&receipt, "source_event_id"))
                    .await
                    .is_err()
            );
        }
        s.engine.relational().pool().close().await;
        drop(s);
        assert!(Service::open(dir.path(), Models::disabled()).await.is_err());
        use sqlx::Connection;
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(dir.path().join("context.db"))
            .create_if_missing(false);
        let mut conn = sqlx::SqliteConnection::connect_with(&options)
            .await
            .unwrap();
        let source: String = sqlx::query_scalar("SELECT content FROM oc_events WHERE id=?")
            .bind(id(&receipt, "source_event_id"))
            .fetch_one(&mut conn)
            .await
            .unwrap();
        let source: Value = serde_json::from_str(&source).unwrap();
        assert_eq!(source["question"], input["question"]);
        assert_eq!(source["answer"], input["answer"]);
        let repaired: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='oc_feedback')",
        )
        .fetch_one(&mut conn)
        .await
        .unwrap();
        assert!(
            !repaired,
            "opening an incompatible database must not install missing tables"
        );
        conn.close().await.unwrap();
    }
}

#[tokio::test]
async fn session_feedback_capacity_blocks_new_records_but_preserves_replays() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, _) = provision(&s).await;
    let session = Uuid::new_v4();
    let turn_id = Uuid::new_v4();
    archive(&s, &a, "capacity-turn", session, turn(turn_id, vec![]))
        .await
        .unwrap();
    let input = feedback(Uuid::new_v4(), json!({"type":"turn"}));
    let receipt = record(&s, &a, "capacity-feedback", session, turn_id, input.clone())
        .await
        .unwrap();
    // Seed the remaining valid immutable rows in a single fixture transaction;
    // exercise the public boundary rather than spending 255 writes on setup.
    let mut tx = s.engine.relational().pool().begin().await.unwrap();
    for ordinal in 1..256 {
        let feedback_id = Uuid::new_v4();
        let body = feedback(feedback_id, json!({"type":"turn"}));
        sqlx::query("INSERT INTO oc_feedback(tenant_id,workspace_id,principal_id,feedback_id,session_id,turn_id,request_hash,feedback,created_at) VALUES(?,?,?,?,?,?,'capacity-fixture',?,?)")
            .bind(a.tenant_id).bind(a.workspace_id).bind(a.id).bind(feedback_id)
            .bind(session).bind(turn_id).bind(body.to_string()).bind(i64::from(ordinal))
            .execute(&mut *tx).await.unwrap();
    }
    tx.commit().await.unwrap();
    assert_eq!(
        s.session_feedback(&a, session, turn_id).await.unwrap()["feedback"]
            .as_array()
            .unwrap()
            .len(),
        256
    );
    let before = counts(&s, true).await;
    assert_eq!(
        record(&s, &a, "capacity-feedback", session, turn_id, input.clone())
            .await
            .unwrap(),
        receipt
    );
    assert_eq!(counts(&s, true).await, before);
    let before = counts(&s, false).await;
    assert_eq!(
        record(&s, &a, "capacity-domain-replay", session, turn_id, input)
            .await
            .unwrap(),
        receipt
    );
    assert_eq!(counts(&s, false).await, before);
    let writer = role(&s, &a, "writer").await;
    for actor in [&a, &writer] {
        let before = counts(&s, true).await;
        assert!(matches!(
            record(
                &s,
                actor,
                "capacity-overflow",
                session,
                turn_id,
                feedback(Uuid::new_v4(), json!({"type":"turn"}))
            )
            .await,
            Err(AppError::Conflict(_))
        ));
        assert_eq!(counts(&s, true).await, before);
    }
    let another = Uuid::new_v4();
    archive(
        &s,
        &a,
        "capacity-other-turn",
        session,
        turn(another, vec![]),
    )
    .await
    .unwrap();
    record(
        &s,
        &a,
        "capacity-other-feedback",
        session,
        another,
        feedback(Uuid::new_v4(), json!({"type":"turn"})),
    )
    .await
    .unwrap();
    assert_eq!(
        s.session_feedback(&a, session, another).await.unwrap()["feedback"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

struct Server(tokio::task::JoinHandle<()>);

impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn serve(s: Service) -> (String, Server) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, origence::api::router(s))
            .await
            .unwrap();
    });
    (url, Server(task))
}

#[tokio::test]
async fn session_http_contract_validates_fields_permissions_and_idempotency() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let (a, token) = provision(&s).await;
    let (_, other_token) = provision(&s).await;
    let reader = s.issue_key(&a, "reader").await.unwrap();
    let reader_token = reader["token"].as_str().unwrap();
    let (base, _server) = serve(s.clone()).await;
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();
    let session = Uuid::new_v4();
    let turn_id = Uuid::new_v4();
    let turns_url = format!("{base}/v1/sessions/{session}/turns");
    let turn_url = format!("{turns_url}/{turn_id}");
    let feedback_url = format!("{turn_url}/feedback");
    let input = turn(turn_id, vec![]);
    let before = counts(&s, true).await;
    for field in ["turn_id", "question", "answer", "used_evidence"] {
        let mut missing = input.clone();
        missing.as_object_mut().unwrap().remove(field);
        let response = http
            .post(&turns_url)
            .bearer_auth(&token)
            .header("Idempotency-Key", "invalid-turn")
            .json(&missing)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 422, "missing {field}");
    }
    let response = http
        .post(&turns_url)
        .bearer_auth(&token)
        .json(&input)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 422);
    let response = http
        .post(&turns_url)
        .header("Idempotency-Key", "unauthenticated")
        .json(&input)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    let response = http
        .post(&turns_url)
        .bearer_auth(reader_token)
        .header("Idempotency-Key", "reader-turn")
        .json(&input)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
    assert_eq!(counts(&s, true).await, before);
    let response = http
        .post(&turns_url)
        .bearer_auth(&token)
        .header("Idempotency-Key", "http-turn")
        .json(&input)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let receipt: Value = response.json().await.unwrap();
    assert_eq!(receipt["state"], "archived");
    let response = http
        .post(&turns_url)
        .bearer_auth(&token)
        .header("Idempotency-Key", "http-turn")
        .json(&input)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.json::<Value>().await.unwrap(), receipt);
    let mut changed = input.clone();
    changed["answer"] = json!("different answer");
    let response = http
        .post(&turns_url)
        .bearer_auth(&token)
        .header("Idempotency-Key", "http-turn")
        .json(&changed)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 409);
    let body = feedback(Uuid::new_v4(), json!({"type":"turn"}));
    let mut invalid = body.clone();
    invalid["rating"] = json!("good");
    let response = http
        .post(&feedback_url)
        .bearer_auth(&token)
        .header("Idempotency-Key", "invalid-rating")
        .json(&invalid)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 422);
    let response = http
        .post(&feedback_url)
        .bearer_auth(reader_token)
        .header("Idempotency-Key", "reader-feedback")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
    let response = http
        .post(&feedback_url)
        .bearer_auth(&token)
        .header("Idempotency-Key", "http-feedback")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.json::<Value>().await.unwrap()["state"], "recorded");
    for url in [&turn_url, &feedback_url] {
        assert_eq!(
            http.get(url)
                .bearer_auth(reader_token)
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        assert_eq!(
            http.get(url)
                .bearer_auth(&other_token)
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
        assert_eq!(http.get(url).send().await.unwrap().status(), 401);
        let response = http.get(url).bearer_auth(&token).send().await.unwrap();
        assert_eq!(response.status(), 200);
        let value: Value = response.json().await.unwrap();
        assert_eq!(value["feedback"].as_array().unwrap().len(), 1);
    }
    let raw_url = format!("{base}/v1/events/{}", id(&receipt, "source_event_id"));
    assert_eq!(
        http.get(&raw_url)
            .bearer_auth(reader_token)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let deleted = s
        .delete(
            &a,
            "http-withdraw",
            id(&receipt, "source_event_id"),
            "event",
        )
        .await
        .unwrap();
    assert_eq!(
        s.job(&a, id(&deleted, "cleanup_job_id")).await.unwrap()["state"],
        "pending"
    );
    for url in [&turn_url, &feedback_url, &raw_url] {
        let response = http.get(url).bearer_auth(&token).send().await.unwrap();
        assert_eq!(response.status(), 404);
        let value: Value = response.json().await.unwrap();
        assert_eq!(value["error"]["code"], "NOT_FOUND");
        assert!(
            !value
                .to_string()
                .contains(input["answer"].as_str().unwrap())
        );
    }
    assert_eq!(
        http.post(&turns_url)
            .bearer_auth(&token)
            .header("Idempotency-Key", "http-turn")
            .json(&input)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        http.post(&feedback_url)
            .bearer_auth(&token)
            .header("Idempotency-Key", "http-feedback")
            .json(&body)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
}
