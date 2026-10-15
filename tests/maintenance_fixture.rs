//! Synthetic no-model authority and native-row oracle for maintenance interruption tests.
#![cfg(all(feature = "local-storage", target_os = "linux"))]
use arrow_array::{Array, FixedSizeListArray, Float32Array, Int32Array, Int64Array, StringArray};
use futures::TryStreamExt;
use lancedb::query::ExecutableQuery;
use origence::{
    memory_identity::MemoryIdentity,
    parsing,
    storage::{
        BlobKey, BlobStore, DomainTx, FullTextEntry, FullTextQuery, LanceDbStore, LedgerEntry,
        LedgerKey, Lifecycle, RelationalStore, Scope, SourceVersion, Surface, TursoStore,
        VectorEntry, VectorQuery, VectorStore, hash, ledger_idempotency_key,
        local_blob::LocalBlobStore, native_turso as db,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
use uuid::Uuid;

const ROWS: usize = 4160;
const BLOB: &[u8] = b"original source bytes\0\xff\nmaintenance recovery";

#[derive(Serialize, Deserialize)]
pub(super) struct Fixture {
    pub scope: Scope,
    pub profiles: Vec<String>,
    pub vectors: Vec<VectorEntry>,
    pub full_text: Vec<FullTextEntry>,
    token: String,
    other_token: String,
    asset: Uuid,
    old: Uuid,
    withdrawn: Uuid,
    current: Vec<Uuid>,
    blob: BlobKey,
    authority: Value,
    native: Value,
    native_versions: BTreeMap<String, u64>,
}

pub(super) fn table_name(profile: &str) -> String {
    format!("vec_{}", &hash(profile)[..32])
}

pub(super) async fn versions(root: &Path) -> BTreeMap<String, u64> {
    let db = lancedb::connect(root.join("vectors").to_str().unwrap())
        .execute()
        .await
        .unwrap();
    let mut result = BTreeMap::new();
    for name in db.table_names().execute().await.unwrap() {
        let table = db.open_table(&name).execute().await.unwrap();
        result.insert(name, table.version().await.unwrap());
    }
    result
}

pub(super) async fn seed(root: &Path) -> Fixture {
    tokio::fs::create_dir_all(root).await.unwrap();
    let store = TursoStore::open(root.join("context.db")).await.unwrap();
    let native = LanceDbStore::open(root.join("vectors")).await.unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "maintenance")
        .await
        .unwrap();
    let key = store.issue_key(scope, "admin").await.unwrap();
    let auth = store.authenticate(&key.token).await.unwrap();
    let other = Scope {
        tenant_id: scope.tenant_id,
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(other.tenant_id, other.workspace_id, "other scope")
        .await
        .unwrap();
    let other_token = store.issue_key(other, "reader").await.unwrap().token;
    let profiles = vec!["maintenance-a:3:v1".into(), "maintenance-b:3:v1".into()];
    let blob = BlobKey {
        scope,
        root: "originals".into(),
        path: "source.bin".into(),
    };
    let blobs = LocalBlobStore::open(root.join("blobs")).await.unwrap();
    blobs.put(&blob, BLOB.to_vec()).await.unwrap();
    blobs.shutdown().await.unwrap();
    let mut fixture = Fixture {
        scope,
        profiles,
        vectors: vec![],
        full_text: vec![],
        token: key.token,
        other_token,
        asset: Uuid::nil(),
        old: Uuid::nil(),
        withdrawn: Uuid::nil(),
        current: vec![],
        blob,
        authority: Value::Null,
        native: Value::Null,
        native_versions: BTreeMap::new(),
    };
    let mut tx = store.begin(auth.clone()).await.unwrap();
    let file = tx
        .insert_file(
            "source.bin",
            "application/octet-stream",
            &hash(BLOB),
            BLOB.len() as i64,
        )
        .await
        .unwrap();
    let mut ledger = vec![];
    let mut withdrawn_source = Uuid::nil();
    for case in 0..3 {
        let identity: MemoryIdentity = serde_json::from_value(json!({
            "subject": {"kind": "service", "stable_id": if case == 2 { "withdrawn" } else { "current" }},
            "predicate": "release.approval", "context": {"environment": "maintenance"}
        })).unwrap();
        let (asset, _) = tx.identity_slot(&identity).await.unwrap();
        // Repeated identity allocation must resolve the same slot, not add one.
        assert_eq!(tx.identity_slot(&identity).await.unwrap().0, asset);
        if case < 2 {
            fixture.asset = asset;
        }
        let version = if case == 1 { 2 } else { 1 };
        let chunk_texts: Vec<_> = (0..if case == 1 { ROWS } else { 1 })
            .map(|index| format!("needle original chunk {case} {index} 日本語"))
            .collect();
        let content = chunk_texts.join("\n");
        let source_id = tx
            .create_event("memory", &content, Some(file))
            .await
            .unwrap();
        let source = SourceVersion { source_id, version };
        if case == 2 {
            withdrawn_source = source_id;
        }
        tx.insert_version(
            asset,
            version,
            &content,
            &hash(&content),
            source_id,
            None,
            None,
        )
        .await
        .unwrap();
        let mut byte_start = 0;
        for (index, text) in chunk_texts.iter().enumerate() {
            let id = Uuid::new_v4();
            let byte_end = byte_start + text.len();
            assert_eq!(&content[byte_start..byte_end], text);
            let terms = parsing::lexical(text);
            tx.insert_chunk(id, asset, version, index as i32, text,
                &json!({"page": 1, "byte_start": byte_start, "byte_end": byte_end, "parser": "synthetic-v1"}), &terms).await.unwrap();
            tx.register_owner(source, "chunk", id, Some(id))
                .await
                .unwrap();
            for surface in [Surface::Vector, Surface::FullText] {
                let key = LedgerKey {
                    scope,
                    source,
                    artifact_id: id,
                    surface,
                    generation: 1,
                };
                tx.register_pending(LedgerEntry {
                    key,
                    artifact_type: "chunk".into(),
                    idempotency_key: ledger_idempotency_key(&key),
                })
                .await
                .unwrap();
                ledger.push(key);
            }
            for profile in &fixture.profiles {
                tx.index_ready(id, profile, 3, 1).await.unwrap();
                fixture.vectors.push(VectorEntry {
                    id,
                    source,
                    generation: 1,
                    profile: profile.clone(),
                    dimension: 3,
                    embedding: vec![
                        1.0 + (index % 67) as f32 / 67.0,
                        (index % 71) as f32 / 71.0,
                        (index % 73) as f32 / 73.0,
                    ],
                });
            }
            fixture.full_text.push(FullTextEntry {
                id,
                source,
                generation: 1,
                search_terms: terms,
            });
            match case {
                0 => fixture.old = id,
                1 => fixture.current.push(id),
                _ => fixture.withdrawn = id,
            }
            byte_start = byte_end + 1;
        }
        tx.update_asset_version(asset, version, None).await.unwrap();
    }
    tx.commit().await.unwrap();
    // Build with 4096 current rows, then append uncovered fragments. Both profiles
    // have a real IVF index; the last 64 current chunks are not in that index.
    for profile in &fixture.profiles {
        let entries: Vec<_> = fixture
            .vectors
            .iter()
            .filter(|v| &v.profile == profile)
            .cloned()
            .collect();
        native
            .upsert(scope, entries[..4097].to_vec())
            .await
            .unwrap();
        native
            .upsert(scope, entries[4097..].to_vec())
            .await
            .unwrap();
    }
    native
        .upsert_full_text(scope, fixture.full_text[..4097].to_vec())
        .await
        .unwrap();
    native
        .upsert_full_text(scope, fixture.full_text[4097..].to_vec())
        .await
        .unwrap();
    let mut tx = store.begin(auth).await.unwrap();
    for key in ledger {
        tx.confirm_committed(key).await.unwrap();
    }
    tx.retract_event(withdrawn_source).await.unwrap();
    tx.commit().await.unwrap();
    // Same artifact UUID with a different native generation must survive, yet
    // cannot acquire authority merely by appearing in a derived table.
    for profile in &fixture.profiles {
        let mut duplicate = fixture
            .vectors
            .iter()
            .find(|v| &v.profile == profile && v.id == fixture.current[0])
            .unwrap()
            .clone();
        duplicate.generation = 2;
        native.upsert(scope, vec![duplicate]).await.unwrap();
    }
    let mut duplicate = fixture.full_text[1].clone();
    duplicate.generation = 2;
    native
        .upsert_full_text(scope, vec![duplicate])
        .await
        .unwrap();
    native.shutdown().await.unwrap();
    store.shutdown().await.unwrap();
    drop(native);
    drop(store);
    fixture.authority = authority_snapshot(root).await;
    fixture.native = native_snapshot(root, None).await;
    fixture.native_versions = versions(root).await;
    check(root, &fixture).await;
    fixture
}

pub(super) async fn assert_authority(root: &Path, fixture: &Fixture) {
    assert_eq!(
        authority_snapshot(root).await,
        fixture.authority,
        "all authority fields preserved"
    );
    let blobs = LocalBlobStore::open(root.join("blobs")).await.unwrap();
    assert_eq!(blobs.get(&fixture.blob).await.unwrap(), BLOB);
    blobs.shutdown().await.unwrap();
}

pub(super) async fn assert_native(root: &Path, fixture: &Fixture) {
    assert_eq!(
        native_snapshot(root, None).await,
        fixture.native,
        "all native identities and payloads preserved before repair, including multiplicity"
    );
    assert_eq!(
        native_snapshot(root, Some(&fixture.native_versions)).await,
        fixture.native,
        "pre-maintenance native snapshots remain readable with every original payload"
    );
}

pub(super) async fn check(root: &Path, fixture: &Fixture) {
    assert_authority(root, fixture).await;
    assert_native(root, fixture).await;
    let store = TursoStore::open(root.join("context.db")).await.unwrap();
    let native = LanceDbStore::open(root.join("vectors")).await.unwrap();
    native.check().await.unwrap();
    native.check_full_text_available().await.unwrap();
    let connection = lancedb::connect(root.join("vectors").to_str().unwrap())
        .execute()
        .await
        .unwrap();
    for profile in &fixture.profiles {
        let table = connection
            .open_table(table_name(profile))
            .execute()
            .await
            .unwrap();
        assert!(
            table
                .list_indices()
                .await
                .unwrap()
                .iter()
                .any(|index| index.name == "vector_l2_ivf64_probe32_v1"),
            "ANN must be present; exact fallback is not ANN coverage"
        );
    }
    drop(connection);
    let auth = store.authenticate(&fixture.token).await.unwrap();
    let mut tx = store.begin_read(auth.clone()).await.unwrap();
    assert_eq!(
        tx.asset_view(fixture.asset, Some(1)).await.unwrap()["version"],
        1
    );
    let old_chunks = tx.chunks_for_version(fixture.asset, 1).await.unwrap();
    assert_eq!(old_chunks.len(), 1);
    assert_eq!(old_chunks[0].0, "needle original chunk 0 0 日本語");
    assert_eq!(old_chunks[0].1["parser"], "synthetic-v1");
    assert_eq!(
        tx.asset_view(fixture.asset, None).await.unwrap()["version"],
        2
    );
    let mut expected = fixture.current.clone();
    expected.sort();
    let mut candidates_by_profile = Vec::new();
    for profile in &fixture.profiles {
        let mut candidates = tx.vector_candidates(profile, 3, 1, None).await.unwrap();
        candidates.sort();
        assert_eq!(candidates, expected);
        candidates_by_profile.push((profile.clone(), candidates));
    }
    let mut text_candidates = tx.full_text_candidates(1, None).await.unwrap();
    text_candidates.sort();
    assert_eq!(text_candidates, expected);
    tx.commit().await.unwrap();

    // Independent native IO happens only after releasing the authority snapshot.
    let selected = fixture.current[ROWS - 1]; // uncovered append fragment
    let mut vector_hits = Vec::new();
    for (profile, candidates) in candidates_by_profile {
        // Exercise the ANN-eligible route without asserting unstable tie order.
        let mut ranked = native
            .search(VectorQuery {
                scope: fixture.scope,
                artifact_ids: Some(candidates),
                profile: profile.clone(),
                dimension: 3,
                generation: 1,
                embedding: vec![1.0, 0.0, 0.0],
                limit: 100,
            })
            .await
            .unwrap();
        assert_eq!(ranked.len(), 100);
        let unique: std::collections::BTreeSet<_> = ranked.iter().map(|hit| hit.id).collect();
        assert_eq!(unique.len(), 100);
        let hits = native
            .search(VectorQuery {
                scope: fixture.scope,
                artifact_ids: Some(vec![selected]),
                profile: profile.clone(),
                dimension: 3,
                generation: 1,
                embedding: vec![1.0, 0.0, 0.0],
                limit: 1,
            })
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, selected);
        ranked.extend(hits);
        vector_hits.push((profile, ranked));
    }
    assert!(text_candidates.contains(&selected));
    let text_hits = native
        .search_full_text(FullTextQuery {
            scope: fixture.scope,
            artifact_ids: Some(vec![selected]),
            generation: 1,
            search_terms: parsing::lexical("needle"),
            limit: 1,
        })
        .await
        .unwrap();
    assert_eq!(text_hits.len(), 1);
    assert_eq!(text_hits[0].id, selected);

    // Revalidate exact returned source/version/generation in a fresh snapshot.
    let mut tx = store.begin_read(auth).await.unwrap();
    for (profile, hits) in vector_hits {
        for hit in hits {
            assert!(fixture.current.contains(&hit.id));
            assert!(
                tx.vector_hit(hit.id, hit.source, &profile, 1)
                    .await
                    .unwrap()
                    .is_some()
            );
        }
        for id in [fixture.old, fixture.withdrawn] {
            let entry = fixture
                .vectors
                .iter()
                .find(|v| v.id == id && v.profile == profile)
                .unwrap();
            assert!(
                tx.vector_hit(id, entry.source, &profile, 1)
                    .await
                    .unwrap()
                    .is_none()
            );
        }
        let current = fixture
            .vectors
            .iter()
            .find(|v| v.id == fixture.current[0] && v.profile == profile)
            .unwrap();
        assert!(
            tx.vector_hit(current.id, current.source, &profile, 2)
                .await
                .unwrap()
                .is_none()
        );
    }
    assert!(
        tx.full_text_hit(text_hits[0].id, text_hits[0].source, 1)
            .await
            .unwrap()
            .is_some()
    );
    for id in [fixture.old, fixture.withdrawn] {
        let entry = fixture.full_text.iter().find(|e| e.id == id).unwrap();
        assert!(
            tx.full_text_hit(id, entry.source, 1)
                .await
                .unwrap()
                .is_none()
        );
    }
    let entry = &fixture.full_text[1];
    assert!(
        tx.full_text_hit(entry.id, entry.source, 2)
            .await
            .unwrap()
            .is_none()
    );
    tx.commit().await.unwrap();
    // A valid principal from another scope must not inherit a native candidate.
    let other_auth = store.authenticate(&fixture.other_token).await.unwrap();
    let other = other_auth.scope;
    let mut other_tx = store.begin_read(other_auth).await.unwrap();
    let entry = &fixture.vectors[2];
    assert!(
        other_tx
            .vector_hit(entry.id, entry.source, &entry.profile, 1)
            .await
            .unwrap()
            .is_none()
    );
    other_tx.commit().await.unwrap();
    assert!(
        native
            .search_full_text(FullTextQuery {
                scope: other,
                artifact_ids: None,
                generation: 1,
                search_terms: parsing::lexical("needle"),
                limit: 10
            })
            .await
            .unwrap()
            .is_empty()
    );
    native.shutdown().await.unwrap();
    store.shutdown().await.unwrap();
}

async fn native_snapshot(root: &Path, versions: Option<&BTreeMap<String, u64>>) -> Value {
    let connection = lancedb::connect(root.join("vectors").to_str().unwrap())
        .execute()
        .await
        .unwrap();
    let mut result = BTreeMap::new();
    for name in connection.table_names().execute().await.unwrap() {
        let table = connection.open_table(&name).execute().await.unwrap();
        if let Some(versions) = versions {
            table.checkout(versions[&name]).await.unwrap();
        }
        let batches: Vec<_> = table
            .query()
            .execute()
            .await
            .unwrap()
            .try_collect()
            .await
            .unwrap();
        let mut rows = Vec::new();
        for batch in batches {
            for row in 0..batch.num_rows() {
                let mut values = Vec::new();
                for column in batch.columns() {
                    let value = if column.is_null(row) {
                        json!(null)
                    } else if let Some(a) = column.as_any().downcast_ref::<StringArray>() {
                        json!(a.value(row))
                    } else if let Some(a) = column.as_any().downcast_ref::<Int32Array>() {
                        json!(a.value(row))
                    } else if let Some(a) = column.as_any().downcast_ref::<Int64Array>() {
                        json!(a.value(row))
                    } else if let Some(a) = column.as_any().downcast_ref::<FixedSizeListArray>() {
                        let vector = a.value(row);
                        let vector = vector.as_any().downcast_ref::<Float32Array>().unwrap();
                        json!(
                            vector
                                .values()
                                .iter()
                                .map(|f| f.to_bits())
                                .collect::<Vec<_>>()
                        )
                    } else {
                        panic!("unexpected native column type")
                    };
                    values.push(value);
                }
                rows.push(serde_json::to_string(&values).unwrap());
            }
        }
        rows.sort(); // Retain duplicates: a set/count-only oracle could hide corruption.
        result.insert(name, rows);
    }
    json!(result)
}

pub(super) async fn authority_snapshot(root: &Path) -> Value {
    let pool = db::Pool::open_read_only(root.join("context.db"))
        .await
        .unwrap();
    let mut result = BTreeMap::new();
    // Frozen single-relation projections include every field, NULL and BLOB,
    // rather than a row count or a visibility-only approximation.
    result.insert("oc_api_keys", db::query_scalar::<String>("SELECT json_array(quote(id),quote(tenant_id),quote(workspace_id),quote(token_hash),quote(role),quote(allow_owned_workspaces),quote(revoked),quote(created_at)) FROM oc_api_keys").fetch_all(&pool).await.unwrap());
    result.insert("oc_artifact_ledger", db::query_scalar::<String>("SELECT json_array(quote(tenant_id),quote(workspace_id),quote(source_id),quote(version),quote(artifact_type),quote(artifact_id),quote(surface),quote(generation),quote(idempotency_key),quote(state),quote(next_retry_at),quote(created_at),quote(updated_at)) FROM oc_artifact_ledger").fetch_all(&pool).await.unwrap());
    result.insert("oc_artifact_owners", db::query_scalar::<String>("SELECT json_array(quote(tenant_id),quote(workspace_id),quote(source_id),quote(version),quote(artifact_type),quote(artifact_id),quote(chunk_id),quote(created_at)) FROM oc_artifact_owners").fetch_all(&pool).await.unwrap());
    result.insert("oc_assets", db::query_scalar::<String>("SELECT json_array(quote(tenant_id),quote(workspace_id),quote(id),quote(kind),quote(title),quote(fact_key),quote(current_version),quote(deleted),quote(created_at),quote(identity_key),quote(identity_json)) FROM oc_assets").fetch_all(&pool).await.unwrap());
    result.insert("oc_audit", db::query_scalar::<String>("SELECT json_array(quote(tenant_id),quote(workspace_id),quote(id),quote(actor),quote(action),quote(target),quote(details),quote(created_at)) FROM oc_audit").fetch_all(&pool).await.unwrap());
    result.insert("oc_chunks", db::query_scalar::<String>("SELECT json_array(quote(tenant_id),quote(workspace_id),quote(id),quote(asset_id),quote(version),quote(ordinal),quote(content),quote(locator),quote(search_terms)) FROM oc_chunks").fetch_all(&pool).await.unwrap());
    result.insert("oc_commands", db::query_scalar::<String>("SELECT json_array(quote(tenant_id),quote(workspace_id),quote(principal_id),quote(dedup_scope),quote(operation),quote(key),quote(request_hash),quote(response),quote(created_at)) FROM oc_commands").fetch_all(&pool).await.unwrap());
    result.insert("oc_events", db::query_scalar::<String>("SELECT json_array(quote(tenant_id),quote(workspace_id),quote(id),quote(content),quote(kind),quote(file_id),quote(state),quote(created_by),quote(created_at)) FROM oc_events").fetch_all(&pool).await.unwrap());
    result.insert("oc_feedback", db::query_scalar::<String>("SELECT json_array(quote(tenant_id),quote(workspace_id),quote(principal_id),quote(feedback_id),quote(session_id),quote(turn_id),quote(request_hash),quote(feedback),quote(created_at),quote(feedback_sequence)) FROM oc_feedback").fetch_all(&pool).await.unwrap());
    result.insert("oc_files", db::query_scalar::<String>("SELECT json_array(quote(tenant_id),quote(workspace_id),quote(id),quote(name),quote(media_type),quote(hash),quote(size),quote(deleted),quote(created_by),quote(created_at)) FROM oc_files").fetch_all(&pool).await.unwrap());
    result.insert("oc_improve_cursors", db::query_scalar::<String>("SELECT json_array(quote(tenant_id),quote(workspace_id),quote(session_id),quote(feedback_sequence)) FROM oc_improve_cursors").fetch_all(&pool).await.unwrap());
    result.insert("oc_improve_runs", db::query_scalar::<String>("SELECT json_array(quote(tenant_id),quote(workspace_id),quote(improve_id),quote(session_id),quote(job_id),quote(request_hash),quote(request),quote(snapshot),quote(lower_sequence),quote(target_sequence),quote(proposal_plan),quote(verification_plan),quote(status),quote(created_by),quote(created_at),quote(completed_at)) FROM oc_improve_runs").fetch_all(&pool).await.unwrap());
    result.insert("oc_index_entries", db::query_scalar::<String>("SELECT json_array(quote(tenant_id),quote(workspace_id),quote(artifact_id),quote(field),quote(model_id),quote(dimension),quote(generation),quote(state),quote(created_at)) FROM oc_index_entries").fetch_all(&pool).await.unwrap());
    result.insert("oc_jobs", db::query_scalar::<String>("SELECT json_array(quote(tenant_id),quote(workspace_id),quote(id),quote(created_by),quote(operation),quote(payload),quote(state),quote(run_token),quote(generation),quote(attempt),quote(next_retry_at),quote(asset_id),quote(source_event_id),quote(cancel_requested),quote(outcome),quote(result),quote(error_code),quote(created_at),quote(updated_at)) FROM oc_jobs").fetch_all(&pool).await.unwrap());
    result.insert("oc_learning_support", db::query_scalar::<String>("SELECT json_array(quote(tenant_id),quote(workspace_id),quote(learning_id),quote(session_id),quote(turn_id),quote(support_ordinal),quote(principal_id),quote(feedback_id)) FROM oc_learning_support").fetch_all(&pool).await.unwrap());
    result.insert("oc_learnings", db::query_scalar::<String>("SELECT json_array(quote(tenant_id),quote(workspace_id),quote(id),quote(improve_id),quote(session_id),quote(candidate_index),quote(asset_id),quote(source_event_id),quote(ingest_job_id),quote(candidate),quote(fingerprint),quote(status),quote(created_by),quote(created_at),quote(published_at)) FROM oc_learnings").fetch_all(&pool).await.unwrap());
    result.insert("oc_memory_properties", db::query_scalar::<String>("SELECT json_array(quote(tenant_id),quote(workspace_id),quote(predicate),quote(definition),quote(created_by),quote(created_at)) FROM oc_memory_properties").fetch_all(&pool).await.unwrap());
    result.insert("oc_session_turn_evidence", db::query_scalar::<String>("SELECT json_array(quote(tenant_id),quote(workspace_id),quote(session_id),quote(turn_id),quote(evidence_ref_id),quote(ordinal),quote(evidence)) FROM oc_session_turn_evidence").fetch_all(&pool).await.unwrap());
    result.insert("oc_session_turns", db::query_scalar::<String>("SELECT json_array(quote(tenant_id),quote(workspace_id),quote(session_id),quote(turn_id),quote(source_event_id),quote(request_hash),quote(created_by),quote(created_at)) FROM oc_session_turns").fetch_all(&pool).await.unwrap());
    result.insert("oc_summaries", db::query_scalar::<String>("SELECT json_array(quote(tenant_id),quote(workspace_id),quote(id),quote(chunk_id),quote(text),quote(model_revision),quote(search_terms)) FROM oc_summaries").fetch_all(&pool).await.unwrap());
    result.insert("oc_versions", db::query_scalar::<String>("SELECT json_array(quote(tenant_id),quote(workspace_id),quote(asset_id),quote(version),quote(content),quote(content_hash),quote(source_event_id),quote(restored_from),quote(title),quote(created_by),quote(created_at)) FROM oc_versions").fetch_all(&pool).await.unwrap());
    result.insert("oc_workspaces", db::query_scalar::<String>("SELECT json_array(quote(tenant_id),quote(id),quote(name),quote(owner_workspace_id),quote(created_at)) FROM oc_workspaces").fetch_all(&pool).await.unwrap());
    for rows in result.values_mut() {
        rows.sort();
    }
    pool.close().await;
    json!(result)
}
