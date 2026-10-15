//! Structured identity persistence and legacy compatibility.
use origence::memory_identity::{MemoryIdentity, MemorySubject, SubjectKind};
use origence::storage::native_turso as db;
use origence::storage::{
    AuthorizedScope, DomainTx, Lifecycle, RelationalStore, Scope, StorageError, sqlite::SqliteStore,
};

use std::collections::BTreeMap;
use uuid::Uuid;

async fn provision(store: &SqliteStore) -> (AuthorizedScope, Scope) {
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "identity")
        .await
        .unwrap();
    let key = store.issue_key(scope, "admin").await.unwrap();
    (store.authenticate(&key.token).await.unwrap(), scope)
}

fn identity() -> MemoryIdentity {
    MemoryIdentity {
        subject: MemorySubject {
            kind: SubjectKind::Service,
            stable_id: "billing".into(),
        },
        predicate: "release.approval".into(),
        context: BTreeMap::from([("environment".into(), "production".into())]),
    }
}

#[tokio::test]
async fn reuse_publish_and_isolate_identity() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("oc.db")).await.unwrap();
    let (auth, scope) = provision(&store).await;
    let i = identity();
    let mut tx = store.begin(auth.clone()).await.unwrap();
    let (asset, version) = tx.identity_slot(&i).await.unwrap();
    assert_eq!(version, None);
    assert_eq!(tx.identity_slot(&i).await.unwrap(), (asset, None));
    let source = tx
        .create_event("identified", "approval", None)
        .await
        .unwrap();
    tx.insert_version(asset, 1, "approval", "hash", source, None, None)
        .await
        .unwrap();
    tx.update_asset_version(asset, 1, None).await.unwrap();
    tx.commit().await.unwrap();
    let mut tx = store.begin(auth).await.unwrap();
    assert_eq!(tx.identity_slot(&i).await.unwrap(), (asset, Some(1)));
    let view = tx.asset_view(asset, None).await.unwrap();
    assert_eq!(view["identity"], serde_json::to_value(&i).unwrap());
    assert!(matches!(
        tx.slot(&i.key(scope).unwrap()).await,
        Err(StorageError::Conflict(_))
    ));
    let mut different = i.clone();
    different.subject.stable_id = "shipping".into();
    assert_ne!(tx.identity_slot(&different).await.unwrap().0, asset);
    different = i.clone();
    different.subject.kind = SubjectKind::Team;
    assert_ne!(tx.identity_slot(&different).await.unwrap().0, asset);
    different = i.clone();
    different.predicate = "release.review".into();
    assert_ne!(tx.identity_slot(&different).await.unwrap().0, asset);
    different = i.clone();
    different
        .context
        .insert("environment".into(), "staging".into());
    assert_ne!(tx.identity_slot(&different).await.unwrap().0, asset);
    tx.commit().await.unwrap();
    let (other, _) = provision(&store).await;
    let mut tx = store.begin(other).await.unwrap();
    assert_ne!(tx.identity_slot(&i).await.unwrap().0, asset);
    assert!(matches!(
        tx.asset_view(asset, None).await,
        Err(StorageError::NotFound)
    ));
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn tombstone_blocks_identity_recreation() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("oc.db")).await.unwrap();
    let (auth, _) = provision(&store).await;
    let mut tx = store.begin(auth).await.unwrap();
    let i = identity();
    let (asset, _) = tx.identity_slot(&i).await.unwrap();
    tx.delete_asset(asset).await.unwrap();
    assert!(matches!(
        tx.identity_slot(&i).await,
        Err(StorageError::Conflict(_))
    ));
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn lookup_is_exact_scoped_and_reads_current_version() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("oc.db")).await.unwrap();
    let (auth, scope) = provision(&store).await;
    let i = identity();
    let mut tx = store.begin(auth).await.unwrap();
    let (asset, _) = tx.identity_slot(&i).await.unwrap();
    let source = tx.create_event("identified", "source", None).await.unwrap();
    for version in [1, 2] {
        tx.insert_version(asset, version, "approval", "hash", source, None, None)
            .await
            .unwrap();
        tx.update_asset_version(asset, version, None).await.unwrap();
    }
    tx.commit().await.unwrap();
    let key = store.issue_key(scope, "reader").await.unwrap();
    let reader = store.authenticate(&key.token).await.unwrap();
    let mut tx = store.begin(reader).await.unwrap();
    tx.check_permission(origence::storage::Permission::Read)
        .await
        .unwrap();
    let view = tx.identity_view(&i).await.unwrap();
    assert_eq!(view["asset_id"], asset.to_string());
    assert_eq!(view["version"], 2);
    let mut other = i.clone();
    other.subject.stable_id = "shipping".into();
    assert!(matches!(
        tx.identity_view(&other).await,
        Err(StorageError::NotFound)
    ));
    other = i.clone();
    other.context.insert("environment".into(), "staging".into());
    assert!(matches!(
        tx.identity_view(&other).await,
        Err(StorageError::NotFound)
    ));
    tx.commit().await.unwrap();
    let (other, _) = provision(&store).await;
    let mut tx = store.begin(other).await.unwrap();
    assert!(matches!(
        tx.identity_view(&i).await,
        Err(StorageError::NotFound)
    ));
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn missing_lookup_does_not_allocate_a_slot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oc.db");
    let store = SqliteStore::open(&path).await.unwrap();
    let (auth, _) = provision(&store).await;
    let mut tx = store.begin(auth).await.unwrap();
    assert!(matches!(
        tx.identity_view(&identity()).await,
        Err(StorageError::NotFound)
    ));
    tx.commit().await.unwrap();
    store.shutdown().await.unwrap();
    let raw_pool = db::Pool::open(&path).await.unwrap();
    let mut conn = raw_pool.acquire().await.unwrap();
    for table in ["oc_assets", "oc_events", "oc_jobs"] {
        let count: i64 = db::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(count, 0, "lookup wrote {table}");
    }
}

#[tokio::test]
async fn lookup_hides_unpublished_deleted_and_retracted_memories() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("oc.db")).await.unwrap();
    let (auth, _) = provision(&store).await;
    let mut tx = store.begin(auth).await.unwrap();
    for deleted in [false, true] {
        let mut i = identity();
        i.subject.stable_id = format!("service-{deleted}");
        let (asset, _) = tx.identity_slot(&i).await.unwrap();
        assert!(matches!(
            tx.identity_view(&i).await,
            Err(StorageError::NotFound)
        ));
        let source = tx.create_event("identified", "source", None).await.unwrap();
        tx.insert_version(asset, 1, "approval", "hash", source, None, None)
            .await
            .unwrap();
        tx.update_asset_version(asset, 1, None).await.unwrap();
        let source = tx.create_event("identified", "update", None).await.unwrap();
        tx.insert_version(asset, 2, "approval", "hash", source, None, None)
            .await
            .unwrap();
        tx.update_asset_version(asset, 2, None).await.unwrap();
        assert!(tx.identity_view(&i).await.is_ok());
        if deleted {
            tx.delete_asset(asset).await.unwrap();
        } else {
            tx.retract_event(source).await.unwrap();
            assert!(tx.asset_view(asset, Some(1)).await.is_ok());
        }
        assert!(matches!(
            tx.identity_view(&i).await,
            Err(StorageError::NotFound)
        ));
    }
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn legacy_collision_is_not_reinterpreted() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("oc.db")).await.unwrap();
    let (auth, scope) = provision(&store).await;
    let mut tx = store.begin(auth).await.unwrap();
    let i = identity();
    let key = i.key(scope).unwrap();
    let legacy = tx.slot(&key).await.unwrap();
    assert!(matches!(
        tx.identity_slot(&i).await,
        Err(StorageError::Conflict(_))
    ));
    assert_eq!(tx.slot(&key).await.unwrap(), legacy);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn vector_candidate_ids_are_filtered_by_exact_identity_before_native_top_k() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("oc.db")).await.unwrap();
    let (auth, scope) = provision(&store).await;
    let i = identity();
    let mut tx = store.begin(auth.clone()).await.unwrap();
    let (target, _) = tx.identity_slot(&i).await.unwrap();
    let (decoy, _) = tx.slot("legacy-decoy").await.unwrap();
    let mut records = Vec::new();
    for asset in [decoy, target] {
        let source = tx.create_event("memory", "approval", None).await.unwrap();
        let chunk = Uuid::new_v4();
        tx.insert_version(asset, 1, "approval", "hash", source, None, None)
            .await
            .unwrap();
        tx.insert_chunk(
            chunk,
            asset,
            1,
            0,
            "approval",
            &serde_json::json!({}),
            "approval",
        )
        .await
        .unwrap();
        tx.update_asset_version(asset, 1, None).await.unwrap();
        tx.index_ready(chunk, "profile", 3, 1).await.unwrap();
        records.push((chunk, source));
    }
    tx.commit().await.unwrap();

    // Model the authoritative committed vector ledger without starting LanceDB:
    // vector_candidates must constrain artifact IDs before native search ranks them.
    for (chunk, source) in &records {
        db::query(
            "INSERT INTO oc_artifact_ledger
             (tenant_id,workspace_id,source_id,version,artifact_type,artifact_id,
              surface,generation,idempotency_key,state,created_at,updated_at)
             VALUES(?,?,?,1,'chunk',?,'vector',1,?,'committed',1,1)",
        )
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(source)
        .bind(chunk)
        .bind(chunk.to_string())
        .execute(store.pool())
        .await
        .unwrap();
    }
    let mut tx = store.begin(auth).await.unwrap();
    let target_asset = tx.identity_candidate_asset(&i).await.unwrap();
    assert_eq!(target_asset, Some(target));
    let unfiltered = tx.vector_candidates("profile", 3, 1, None).await.unwrap();
    assert_eq!(unfiltered.len(), 2);
    let filtered = tx
        .vector_candidates("profile", 3, 1, target_asset)
        .await
        .unwrap();
    assert_eq!(filtered, vec![records[1].0]);
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn incomplete_database_is_rejected_without_implicit_repair() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oc.db");
    let store = SqliteStore::open(&path).await.unwrap();
    let (auth, _) = provision(&store).await;
    let principal_id = auth.principal_id;
    let mut tx = store.begin(auth).await.unwrap();
    let source = tx
        .create_event("structured", "source must remain", None)
        .await
        .unwrap();
    let (asset, _) = tx.slot("legacy.fact").await.unwrap();
    tx.commit().await.unwrap();
    db::query("DROP INDEX oc_memory_identity_slot")
        .execute(store.pool())
        .await
        .unwrap();
    store.shutdown().await.unwrap();
    drop(store);
    assert!(matches!(
        SqliteStore::open(&path).await,
        Err(StorageError::Unavailable(_))
    ));
    let raw_pool = db::Pool::open_read_only(&path).await.unwrap();
    let exists: bool = db::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='oc_memory_identity_slot')",
    )
    .fetch_one(&raw_pool)
    .await
    .unwrap();
    assert!(!exists);
    let content: String = db::query_scalar("SELECT content FROM oc_events WHERE id=?")
        .bind(source)
        .fetch_one(&raw_pool)
        .await
        .unwrap();
    assert_eq!(content, "source must remain");
    let fact_key: String = db::query_scalar("SELECT fact_key FROM oc_assets WHERE id=?")
        .bind(asset)
        .fetch_one(&raw_pool)
        .await
        .unwrap();
    assert_eq!(fact_key, "legacy.fact");
    let role: String = db::query_scalar("SELECT role FROM oc_api_keys WHERE id=?")
        .bind(principal_id)
        .fetch_one(&raw_pool)
        .await
        .unwrap();
    assert_eq!(role, "admin");
    raw_pool.close().await;
}

#[tokio::test]
async fn identity_binding_mismatch_precedes_tombstone_rejection() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("oc.db")).await.unwrap();
    let (auth, scope) = provision(&store).await;
    let requested = identity();
    let mut tx = store.begin(auth.clone()).await.unwrap();
    let (asset, version) = tx.identity_slot(&requested).await.unwrap();
    assert_eq!(version, None);
    tx.delete_asset(asset).await.unwrap();
    tx.commit().await.unwrap();
    let mut different = requested.clone();
    different.subject.stable_id = "different".into();
    db::query("UPDATE oc_assets SET identity_json=? WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(serde_json::to_string(&different).unwrap())
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(asset)
        .execute(store.pool())
        .await
        .unwrap();
    let mut tx = store.begin(auth).await.unwrap();
    assert!(matches!(tx.identity_slot(&requested).await,
        Err(StorageError::Conflict(message)) if message == "memory identity binding mismatch"));
    tx.rollback().await.unwrap();
    let count: i64 =
        db::query_scalar("SELECT count(*) FROM oc_assets WHERE tenant_id=? AND workspace_id=?")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn non_memory_identity_binding_is_rejected_at_write() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("oc.db")).await.unwrap();
    let (auth, scope) = provision(&store).await;
    let requested = identity();
    let mut tx = store.begin(auth.clone()).await.unwrap();
    let (asset, _) = tx.identity_slot(&requested).await.unwrap();
    tx.commit().await.unwrap();
    assert!(
        db::query(
            "UPDATE oc_assets SET kind='knowledge' WHERE tenant_id=? AND workspace_id=? AND id=?",
        )
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(asset)
        .execute(store.pool())
        .await
        .is_err()
    );
    let mut tx = store.begin(auth).await.unwrap();
    assert_eq!(tx.identity_slot(&requested).await.unwrap(), (asset, None));
    tx.rollback().await.unwrap();
    let assets: i64 =
        db::query_scalar("SELECT count(*) FROM oc_assets WHERE tenant_id=? AND workspace_id=?")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(assets, 1);
    let binding: Uuid = db::query_scalar(
        "SELECT id FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND identity_key=?",
    )
    .bind(scope.tenant_id)
    .bind(scope.workspace_id)
    .bind(requested.key(scope).unwrap())
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(binding, asset);
}

#[tokio::test]
async fn merged_identity_constraints_cover_null_pairs_kinds_and_tombstones() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("oc.db")).await.unwrap();
    let (_, scope) = provision(&store).await;
    let other_scope = Scope {
        tenant_id: scope.tenant_id,
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(
            other_scope.tenant_id,
            other_scope.workspace_id,
            "other workspace",
        )
        .await
        .unwrap();
    let body = serde_json::to_string(&identity()).unwrap();
    for (kind, key, value, accepted) in [
        ("memory", None, None, true),
        ("memory", None, None, true),
        ("knowledge", None, None, true),
        ("memory", Some("key-only"), None, false),
        ("memory", None, Some(body.as_str()), false),
        (
            "knowledge",
            Some("knowledge-key"),
            Some(body.as_str()),
            false,
        ),
        ("memory", Some("reserved"), Some(body.as_str()), true),
    ] {
        let result = db::query(
            "INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,identity_key,identity_json,created_at) VALUES(?,?,?,?,?,?,?,1)",
        )
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(Uuid::new_v4())
        .bind(kind)
        .bind("constraint fixture")
        .bind(key)
        .bind(value)
        .execute(store.pool())
        .await;
        assert_eq!(result.is_ok(), accepted, "kind={kind}, key={key:?}");
    }
    for deleted in [false, true] {
        db::query("UPDATE oc_assets SET deleted=? WHERE identity_key='reserved'")
            .bind(deleted)
            .execute(store.pool())
            .await
            .unwrap();
        // Distinct fact keys (both NULL) ensure the identity index is responsible.
        assert!(db::query(
            "INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,identity_key,identity_json,created_at) VALUES(?,?,?,'memory','duplicate','reserved',?,1)",
        )
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(Uuid::new_v4())
        .bind(&body)
        .execute(store.pool())
        .await
        .is_err());
    }
    db::query(
        "INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,identity_key,identity_json,created_at) VALUES(?,?,?,'memory','other scope','reserved',?,1)",
    )
    .bind(other_scope.tenant_id)
    .bind(other_scope.workspace_id)
    .bind(Uuid::new_v4())
    .bind(&body)
    .execute(store.pool())
    .await
    .unwrap();
    let obsolete: i64 =
        db::query_scalar("SELECT count(*) FROM sqlite_master WHERE name='oc_memory_identities'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(obsolete, 0);
}

#[tokio::test]
async fn selected_identity_reads_fail_closed_on_corrupt_body_or_key() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("oc.db")).await.unwrap();
    let (auth, scope) = provision(&store).await;
    let requested = identity();
    let key = requested.key(scope).unwrap();
    let body = serde_json::to_string(&requested).unwrap();
    let mut tx = store.begin(auth.clone()).await.unwrap();
    let (asset, _) = tx.identity_slot(&requested).await.unwrap();
    let source = tx
        .create_event("identified", "approval", None)
        .await
        .unwrap();
    tx.insert_version(asset, 1, "approval", "hash", source, None, None)
        .await
        .unwrap();
    tx.update_asset_version(asset, 1, None).await.unwrap();
    tx.commit().await.unwrap();
    let mut different = requested.clone();
    different.subject.stable_id = "different".into();
    let mut invalid = requested.clone();
    invalid.subject.stable_id.clear();
    for (stored_key, stored_body) in [
        (key.as_str(), "{".to_string()),
        (key.as_str(), serde_json::to_string(&invalid).unwrap()),
        (key.as_str(), serde_json::to_string(&different).unwrap()),
        ("wrong-key", body.clone()),
    ] {
        db::query("UPDATE oc_assets SET identity_key=?,identity_json=? WHERE id=?")
            .bind(stored_key)
            .bind(&stored_body)
            .bind(asset)
            .execute(store.pool())
            .await
            .unwrap();
        let mut tx = store.begin(auth.clone()).await.unwrap();
        assert!(matches!(
            tx.asset_view(asset, None).await,
            Err(StorageError::Unavailable(_))
        ));
        if stored_key == key {
            assert!(matches!(
                tx.identity_view(&requested).await,
                Err(StorageError::Unavailable(_))
            ));
            assert!(matches!(
                tx.identity_candidate_asset(&requested).await,
                Err(StorageError::Unavailable(_))
            ));
        }
        tx.rollback().await.unwrap();
    }
    db::query("UPDATE oc_assets SET identity_key=?,identity_json='{',deleted=1 WHERE id=?")
        .bind(&key)
        .bind(asset)
        .execute(store.pool())
        .await
        .unwrap();
    let mut tx = store.begin(auth).await.unwrap();
    // Candidate filtering hides a tombstone before decoding its corrupt body.
    assert_eq!(tx.identity_candidate_asset(&requested).await.unwrap(), None);
    assert!(matches!(
        tx.identity_view(&requested).await,
        Err(StorageError::Unavailable(_))
    ));
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn same_format_backup_reopens_with_identity_history_sources_and_tombstones() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oc.db");
    let store = SqliteStore::open(&path).await.unwrap();
    let (auth, _) = provision(&store).await;
    let requested = identity();
    let mut deleted_identity = requested.clone();
    deleted_identity.subject.stable_id = "deleted".into();
    let mut tx = store.begin(auth.clone()).await.unwrap();
    let (asset, _) = tx.identity_slot(&requested).await.unwrap();
    let source = tx
        .create_event("identified", "original source", None)
        .await
        .unwrap();
    for version in [1, 2, 3] {
        let restored_from = (version == 3).then_some(1);
        tx.insert_version(
            asset,
            version,
            "approval",
            "hash",
            source,
            restored_from,
            None,
        )
        .await
        .unwrap();
        tx.update_asset_version(asset, version, None).await.unwrap();
    }
    let (deleted, _) = tx.identity_slot(&deleted_identity).await.unwrap();
    tx.delete_asset(deleted).await.unwrap();
    tx.commit().await.unwrap();
    store.shutdown().await.unwrap();
    drop(store);
    let backup = tempfile::tempdir().unwrap();
    // Copy the entire quiescent directory, including any native sidecars.
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let entry = entry.unwrap();
        assert!(entry.file_type().unwrap().is_file());
        std::fs::copy(entry.path(), backup.path().join(entry.file_name())).unwrap();
    }
    let restored_path = backup.path().join("oc.db");
    SqliteStore::check_memory_identity(&restored_path)
        .await
        .unwrap();
    let store = SqliteStore::open(&restored_path).await.unwrap();
    let mut tx = store.begin(auth).await.unwrap();
    assert_eq!(
        tx.identity_slot(&requested).await.unwrap(),
        (asset, Some(3))
    );
    for version in [1, 2, 3] {
        let view = tx.asset_view(asset, Some(version)).await.unwrap();
        assert_eq!(view["identity"], serde_json::to_value(&requested).unwrap());
        assert_eq!(view["source_event_id"], source.to_string());
    }
    assert_eq!(
        tx.event_content(source, true).await.unwrap().as_deref(),
        Some("original source")
    );
    let restore = tx.restore_source(asset, 1).await.unwrap();
    assert_eq!(restore["current_version"], 3);
    assert_eq!(restore["source_event_id"], source.to_string());
    assert!(matches!(
        tx.restore_source(deleted, 1).await,
        Err(StorageError::NotFound)
    ));
    assert!(matches!(tx.identity_slot(&deleted_identity).await,
        Err(StorageError::Conflict(message)) if message == "memory identity asset was deleted"));
    tx.rollback().await.unwrap();
    let restored_from: i32 =
        db::query_scalar("SELECT restored_from FROM oc_versions WHERE asset_id=? AND version=3")
            .bind(asset)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(restored_from, 1);
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn deleted_legacy_collision_does_not_allocate_a_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("oc.db")).await.unwrap();
    let (auth, scope) = provision(&store).await;
    let requested = identity();
    let key = requested.key(scope).unwrap();
    let mut tx = store.begin(auth.clone()).await.unwrap();
    let (asset, _) = tx.slot(&key).await.unwrap();
    tx.delete_asset(asset).await.unwrap();
    tx.commit().await.unwrap();
    let mut tx = store.begin(auth).await.unwrap();
    assert!(matches!(tx.identity_slot(&requested).await,
        Err(StorageError::Conflict(message)) if message == "memory identity conflicts with a legacy fact slot"));
    tx.rollback().await.unwrap();
    let count: i64 = db::query_scalar("SELECT count(*) FROM oc_assets")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
    let binding: Option<String> = db::query_scalar("SELECT identity_key FROM oc_assets WHERE id=?")
        .bind(asset)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(binding, None);
}
