use origence::storage::{
    AuthorizedScope, DomainTx, RelationalStore, Scope, SourceVersion, SqliteStore, StorageError,
};
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Clone, Copy)]
struct EvidenceIds {
    asset: Uuid,
    root: Uuid,
    member: Uuid,
    chunk: Uuid,
    graph: Uuid,
}

impl EvidenceIds {
    fn new() -> Self {
        Self {
            asset: Uuid::new_v4(),
            root: Uuid::new_v4(),
            member: Uuid::new_v4(),
            chunk: Uuid::new_v4(),
            graph: Uuid::new_v4(),
        }
    }

    fn locator(self) -> Value {
        json!({
            "parser":"memory-semantics:v1","semantic_status":"confirmed",
            "semantic_value":"needle","source_event_id":self.member,"source_version":1,
            "support_sources":[self.root,self.member],"byte_start":0,"byte_end":6
        })
    }
}

/// Deliberately reuse every object UUID across scopes. Distinct source/member,
/// asset-version, dimension and generation values expose swapped SQL bindings.
async fn seed(store: &SqliteStore, scope: Scope, ids: EvidenceIds) -> AuthorizedScope {
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "visibility")
        .await
        .unwrap();
    let key = store.issue_key(scope, "admin").await.unwrap();
    let auth = store.authenticate(&key.token).await.unwrap();
    for source in [ids.root, ids.member] {
        sqlx::query("INSERT INTO oc_events(tenant_id,workspace_id,id,content,kind,created_by,created_at) VALUES(?,?,?,'needle','memory',?,0)")
            .bind(scope.tenant_id).bind(scope.workspace_id).bind(source).bind(auth.principal_id)
            .execute(store.pool()).await.unwrap();
    }
    sqlx::query("INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,current_version,created_at) VALUES(?,?,?,'memory','visibility',3,0)")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(ids.asset)
        .execute(store.pool()).await.unwrap();
    sqlx::query("INSERT INTO oc_versions(tenant_id,workspace_id,asset_id,version,content,content_hash,source_event_id,title,created_by,created_at) VALUES(?,?,?,3,'needle','fixture',?,'visibility',?,0)")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(ids.asset).bind(ids.root).bind(auth.principal_id)
        .execute(store.pool()).await.unwrap();
    sqlx::query("INSERT INTO oc_chunks(tenant_id,workspace_id,id,asset_id,version,ordinal,content,locator,search_terms) VALUES(?,?,?,?,3,0,?,?,'needle')")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(ids.chunk).bind(ids.asset)
        .bind(scope.workspace_id.to_string()).bind(ids.locator().to_string())
        .execute(store.pool()).await.unwrap();
    sqlx::query("INSERT INTO oc_summaries(tenant_id,workspace_id,id,chunk_id,text,search_terms) VALUES(?,?,?,?,'summary','summary')")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(Uuid::new_v4()).bind(ids.chunk)
        .execute(store.pool()).await.unwrap();
    sqlx::query("INSERT INTO oc_index_entries(tenant_id,workspace_id,artifact_id,field,model_id,dimension,generation,state,created_at) VALUES(?,?,?,'content','profile-visibility',7,11,'ready',0)")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(ids.chunk)
        .execute(store.pool()).await.unwrap();
    for (artifact, kind, surface) in [
        (ids.chunk, "chunk", "vector"),
        (ids.graph, "entity", "graph"),
    ] {
        sqlx::query("INSERT INTO oc_artifact_owners(tenant_id,workspace_id,source_id,version,artifact_type,artifact_id,chunk_id,created_at) VALUES(?,?,?,3,?,?,?,0)")
            .bind(scope.tenant_id).bind(scope.workspace_id).bind(ids.root).bind(kind).bind(artifact)
            .bind((kind == "chunk").then_some(ids.chunk)).execute(store.pool()).await.unwrap();
        sqlx::query("INSERT INTO oc_artifact_ledger(tenant_id,workspace_id,source_id,version,artifact_type,artifact_id,surface,generation,idempotency_key,state,created_at,updated_at) VALUES(?,?,?,3,?,?,?,11,?,'committed',0,0)")
            .bind(scope.tenant_id).bind(scope.workspace_id).bind(ids.root).bind(kind).bind(artifact)
            .bind(surface).bind(Uuid::new_v4().to_string()).execute(store.pool()).await.unwrap();
    }
    auth
}

async fn assert_visibility(
    store: &SqliteStore,
    auth: &AuthorizedScope,
    ids: EvidenceIds,
    visible: bool,
) {
    let mut tx = store.begin_read(auth.clone()).await.unwrap();
    assert_eq!(tx.visible_hit(ids.chunk).await.unwrap().is_some(), visible);
    for filter in [None, Some(ids.asset)] {
        for (terms, summaries) in [("needle", false), ("summary", true)] {
            let hits = tx.keyword_hits(terms, summaries, filter).await.unwrap();
            assert_eq!(hits.len(), usize::from(visible));
            for hit in hits {
                assert_eq!(hit.content, auth.scope.workspace_id.to_string());
                assert_eq!(hit.source_event_id, ids.member);
                assert_eq!(hit.version, 3);
            }
        }
        assert_eq!(
            tx.vector_candidates("profile-visibility", 7, 11, filter)
                .await
                .unwrap(),
            if visible { vec![ids.chunk] } else { vec![] }
        );
    }
    assert_eq!(
        tx.vector_hit(
            ids.chunk,
            SourceVersion {
                source_id: ids.root,
                version: 3,
            },
            "profile-visibility",
            11,
        )
        .await
        .unwrap()
        .is_some(),
        visible
    );
    let graph = tx.graph_evidence(ids.graph).await.unwrap();
    assert_eq!(graph.len(), usize::from(visible));
    if visible {
        assert_eq!(graph[0]["source_event_id"], json!(ids.member));
        assert_eq!(graph[0]["version"], 3);
    }
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn sqlite_visibility_binds_scope_filters_and_vector_publication_independently() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("visibility.sqlite"))
        .await
        .unwrap();
    let ids = EvidenceIds::new();
    let tenant = Uuid::new_v4();
    let mut scopes = Vec::new();
    for tenant_id in [tenant, tenant, Uuid::new_v4()] {
        scopes.push(
            seed(
                &store,
                Scope {
                    tenant_id,
                    workspace_id: Uuid::new_v4(),
                },
                ids,
            )
            .await,
        );
    }
    for auth in &scopes {
        assert_visibility(&store, auth, ids, true).await;
    }
    let auth = &scopes[0];
    let mut tx = store.begin_read(auth.clone()).await.unwrap();
    for (profile, dimension, generation, filter) in [
        ("other-profile", 7, 11, None),
        ("profile-visibility", 8, 11, None),
        ("profile-visibility", 7, 12, None),
        ("profile-visibility", 7, 11, Some(Uuid::new_v4())),
    ] {
        assert!(
            tx.vector_candidates(profile, dimension, generation, filter)
                .await
                .unwrap()
                .is_empty()
        );
    }
    for (source_id, version, profile, generation) in [
        (ids.member, 1, "profile-visibility", 11),
        (ids.root, 1, "profile-visibility", 11),
        (ids.root, 3, "other-profile", 11),
        (ids.root, 3, "profile-visibility", 12),
    ] {
        assert!(
            tx.vector_hit(
                ids.chunk,
                SourceVersion { source_id, version },
                profile,
                generation,
            )
            .await
            .unwrap()
            .is_none()
        );
    }
    assert!(
        tx.keyword_hits("needle", false, Some(Uuid::new_v4()))
            .await
            .unwrap()
            .is_empty()
    );
    tx.commit().await.unwrap();

    // Active copies of the exact same UUIDs in another workspace or tenant
    // cannot rescue a withdrawn root/member or deleted local asset.
    for source in [ids.root, ids.member] {
        sqlx::query(
            "UPDATE oc_events SET state='retracted' WHERE tenant_id=? AND workspace_id=? AND id=?",
        )
        .bind(auth.scope.tenant_id)
        .bind(auth.scope.workspace_id)
        .bind(source)
        .execute(store.pool())
        .await
        .unwrap();
        assert_visibility(&store, auth, ids, false).await;
        for other in &scopes[1..] {
            assert_visibility(&store, other, ids, true).await;
        }
        sqlx::query(
            "UPDATE oc_events SET state='active' WHERE tenant_id=? AND workspace_id=? AND id=?",
        )
        .bind(auth.scope.tenant_id)
        .bind(auth.scope.workspace_id)
        .bind(source)
        .execute(store.pool())
        .await
        .unwrap();
    }
    sqlx::query("UPDATE oc_assets SET deleted=1 WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(auth.scope.tenant_id)
        .bind(auth.scope.workspace_id)
        .bind(ids.asset)
        .execute(store.pool())
        .await
        .unwrap();
    assert_visibility(&store, auth, ids, false).await;
    for other in &scopes[1..] {
        assert_visibility(&store, other, ids, true).await;
    }
}

#[tokio::test]
async fn sqlite_visibility_preserves_invalid_support_and_malformed_locator_results() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("visibility.sqlite"))
        .await
        .unwrap();
    let ids = EvidenceIds::new();
    let auth = seed(
        &store,
        Scope {
            tenant_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
        },
        ids,
    )
    .await;
    assert_visibility(&store, &auth, ids, true).await;
    for support in [json!([ids.root, "not-a-uuid"]), json!([ids.root, 17])] {
        let mut locator = ids.locator();
        locator["support_sources"] = support;
        sqlx::query("UPDATE oc_chunks SET locator=? WHERE tenant_id=? AND workspace_id=? AND id=?")
            .bind(locator.to_string())
            .bind(auth.scope.tenant_id)
            .bind(auth.scope.workspace_id)
            .bind(ids.chunk)
            .execute(store.pool())
            .await
            .unwrap();
        assert_visibility(&store, &auth, ids, false).await;
    }
    // Invalid JSON remains a storage error, rather than becoming a visible
    // non-semantic chunk or being silently converted to an empty result.
    sqlx::query("UPDATE oc_chunks SET locator='{' WHERE tenant_id=? AND workspace_id=? AND id=?")
        .bind(auth.scope.tenant_id)
        .bind(auth.scope.workspace_id)
        .bind(ids.chunk)
        .execute(store.pool())
        .await
        .unwrap();
    let mut tx = store.begin_read(auth).await.unwrap();
    assert!(matches!(
        tx.visible_hit(ids.chunk).await,
        Err(StorageError::Backend(_))
    ));
    assert!(matches!(
        tx.keyword_hits("needle", false, None).await,
        Err(StorageError::Backend(_))
    ));
    assert!(matches!(
        tx.vector_candidates("profile-visibility", 7, 11, None)
            .await,
        Err(StorageError::Backend(_))
    ));
    assert!(matches!(
        tx.graph_evidence(ids.graph).await,
        Err(StorageError::Backend(_))
    ));
    tx.commit().await.unwrap();
}
