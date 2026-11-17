//! Real SQLite transactions through public library entry points, no query doubles.
use super::*;
use crate::skill::*;
use std::collections::BTreeMap;

struct Fixture {
    dir: tempfile::TempDir,
    store: SqliteStore,
    owner: AuthorizedScope,
    reviewer: AuthorizedScope,
    other: AuthorizedScope,
    source: Uuid,
    input: SkillCandidateInput,
}
impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(dir.path().join("skills.db"))
            .await
            .unwrap();
        let scope = Scope {
            tenant_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
        };
        store
            .create_workspace(scope.tenant_id, scope.workspace_id, "skills")
            .await
            .unwrap();
        let mut keys = Vec::new();
        for role in ["writer", "reviewer", "admin"] {
            let key = store.issue_key(scope, role).await.unwrap();
            keys.push(store.authenticate(&key.token).await.unwrap());
        }
        let owner = keys.remove(0);
        let reviewer = keys.remove(0);
        let other = keys.remove(0);
        let source = Uuid::new_v4();
        let asset = Uuid::new_v4();
        let mut tx = store.begin(owner.clone()).await.unwrap();
        sqlite_db::query("INSERT INTO oc_assets(tenant_id,workspace_id,id,kind,title,current_version,created_at) VALUES(?,?,?,'knowledge','source',1,0)")
            .bind(scope.tenant_id).bind(scope.workspace_id).bind(asset).execute(&mut *tx.tx).await.unwrap();
        sqlite_db::query("INSERT INTO oc_events(tenant_id,workspace_id,id,content,kind,created_by,created_at) VALUES(?,?,?,'basis','knowledge',?,0)")
            .bind(scope.tenant_id).bind(scope.workspace_id).bind(source).bind(owner.principal_id).execute(&mut *tx.tx).await.unwrap();
        tx.insert_version_inner(asset, 1, "basis", source, None, Some("source"))
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let input = SkillCandidateInput {
            draft: SkillDraft {
                name: "reviewed-method".into(),
                description: "Reusable verification".into(),
                body: "Verify the result independently.".into(),
                references: BTreeMap::new(),
            },
            sources: vec![SkillSource::Asset {
                asset_id: asset,
                version: 1,
                content_sha256: hash("basis"),
            }],
            private_notes: "PRIVATE raw reasoning".into(),
        };
        Self {
            dir,
            store,
            owner,
            reviewer,
            other,
            source,
            input,
        }
    }
    fn review(&self, revision: u32, skill: Uuid) -> SkillReviewInput {
        SkillReviewInput {
            revision,
            reviewer_id: self.reviewer.principal_id,
            target: SkillTarget {
                tenant_id: self.owner.scope.tenant_id,
                workspace_id: self.owner.scope.workspace_id,
                skill_id: skill,
                version: 1,
            },
            basis: "Selected redacted basis, not proof of general reliability".into(),
        }
    }
}

#[tokio::test]
async fn skill_exact_approval_private_review_restart_and_withdrawal() {
    let mut f = Fixture::new().await;
    let candidate = Uuid::new_v4();
    let review = Uuid::new_v4();
    let skill = Uuid::new_v4();
    let mut tx = f.store.begin(f.owner.clone()).await.unwrap();
    assert_eq!(
        tx.save_skill_candidate(candidate, 0, &f.input)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        tx.save_skill_candidate(candidate, 0, &f.input)
            .await
            .unwrap(),
        1
    );
    let packet = tx
        .submit_skill_review(candidate, review, &f.review(1, skill))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut tx = f.store.begin(f.other.clone()).await.unwrap();
    assert!(matches!(
        tx.skill_candidate(candidate).await,
        Err(StorageError::NotFound)
    ));
    assert!(matches!(
        tx.skill_review(review).await,
        Err(StorageError::NotFound)
    ));
    assert!(
        tx.decide_skill_review(review, &packet.digest, true)
            .await
            .is_err()
    );
    assert!(tx.publish_skill(review, &packet.digest).await.is_err());
    tx.rollback().await.unwrap();
    let mut tx = f.store.begin(f.reviewer.clone()).await.unwrap();
    let selected = tx.skill_review(review).await.unwrap();
    assert!(!selected.to_string().contains("PRIVATE"));
    assert!(!selected.to_string().contains(&f.source.to_string()));
    assert!(tx.decide_skill_review(review, "wrong", true).await.is_err());
    tx.decide_skill_review(review, &packet.digest, true)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut tx = f.store.begin(f.owner.clone()).await.unwrap();
    assert!(tx.publish_skill(review, &packet.digest).await.is_err());
    tx.rollback().await.unwrap();
    let mut tx = f.store.begin(f.reviewer.clone()).await.unwrap();
    let package = tx.publish_skill(review, &packet.digest).await.unwrap();
    assert_eq!(
        package,
        tx.publish_skill(review, &packet.digest).await.unwrap()
    );
    package.validate().unwrap();
    tx.commit().await.unwrap();
    f.store.shutdown().await.unwrap();
    drop(f.store);
    f.store = SqliteStore::open(f.dir.path().join("skills.db"))
        .await
        .unwrap();
    let mut tx = f.store.begin(f.other.clone()).await.unwrap();
    assert_eq!(package, tx.skill_package(skill, 1).await.unwrap());
    tx.withdraw_skill(skill, 1).await.unwrap();
    assert!(matches!(
        tx.skill_package(skill, 1).await,
        Err(StorageError::NotFound)
    ));
    assert!(tx.publish_skill(review, &packet.digest).await.is_err());
    tx.commit().await.unwrap();
    f.store.shutdown().await.unwrap();
}

#[tokio::test]
async fn skill_edits_targets_sources_and_reviewer_revocation_invalidate_publication() {
    let f = Fixture::new().await;
    let candidate = Uuid::new_v4();
    let review = Uuid::new_v4();
    let skill = Uuid::new_v4();
    let mut tx = f.store.begin(f.owner.clone()).await.unwrap();
    tx.save_skill_candidate(candidate, 0, &f.input)
        .await
        .unwrap();
    let mut wrong = f.review(1, skill);
    wrong.target.workspace_id = Uuid::new_v4();
    assert!(
        tx.submit_skill_review(candidate, review, &wrong)
            .await
            .is_err()
    );
    let packet = tx
        .submit_skill_review(candidate, review, &f.review(1, skill))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut tx = f.store.begin(f.reviewer.clone()).await.unwrap();
    tx.decide_skill_review(review, &packet.digest, true)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut edited = f.input.clone();
    edited.draft.body.push_str(" Then check again.");
    let mut tx = f.store.begin(f.owner.clone()).await.unwrap();
    tx.save_skill_candidate(candidate, 1, &edited)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut tx = f.store.begin(f.reviewer.clone()).await.unwrap();
    assert!(tx.publish_skill(review, &packet.digest).await.is_err());
    tx.rollback().await.unwrap();
    let mut tx = f.store.begin(f.owner.clone()).await.unwrap();
    let next = Uuid::new_v4();
    let packet = tx
        .submit_skill_review(candidate, next, &f.review(2, skill))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut tx = f.store.begin(f.reviewer.clone()).await.unwrap();
    tx.decide_skill_review(next, &packet.digest, true)
        .await
        .unwrap();
    tx.publish_skill(next, &packet.digest).await.unwrap();
    tx.commit().await.unwrap();
    let mut tx = f.store.begin(f.other.clone()).await.unwrap();
    sqlite_db::query(
        "UPDATE oc_events SET state='retracted' WHERE tenant_id=? AND workspace_id=? AND id=?",
    )
    .bind(f.owner.scope.tenant_id)
    .bind(f.owner.scope.workspace_id)
    .bind(f.source)
    .execute(&mut *tx.tx)
    .await
    .unwrap();
    assert!(matches!(
        tx.skill_package(skill, 1).await,
        Err(StorageError::NotFound)
    ));
    assert!(tx.publish_skill(next, &packet.digest).await.is_err());
    tx.commit().await.unwrap();
    f.store.shutdown().await.unwrap();
}

#[tokio::test]
async fn skill_private_run_basis_and_revoked_approver_fail_closed() {
    let f = Fixture::new().await;
    let run = Uuid::new_v4();
    let candidate = Uuid::new_v4();
    let review = Uuid::new_v4();
    let skill = Uuid::new_v4();
    let mut tx = f.store.begin(f.owner.clone()).await.unwrap();
    tx.create_private_run(&crate::run::RunInput {
        run_id: run,
        summary: "PRIVATE original task".into(),
    })
    .await
    .unwrap();
    let digest = tx.private_run_prefix_digest(run, 0).await.unwrap();
    let mut input = f.input.clone();
    input.sources = vec![SkillSource::Run {
        run_id: run,
        through_sequence: 0,
        digest,
    }];
    tx.save_skill_candidate(candidate, 0, &input).await.unwrap();
    let packet = tx
        .submit_skill_review(candidate, review, &f.review(1, skill))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut tx = f.store.begin(f.other.clone()).await.unwrap();
    assert!(matches!(
        tx.save_skill_candidate(Uuid::new_v4(), 0, &input).await,
        Err(StorageError::NotFound)
    ));
    tx.rollback().await.unwrap();
    let mut tx = f.store.begin(f.reviewer.clone()).await.unwrap();
    assert!(
        !tx.skill_review(review)
            .await
            .unwrap()
            .to_string()
            .contains("PRIVATE")
    );
    assert!(tx.private_run_trace(run, 0, 10).await.is_err());
    tx.decide_skill_review(review, &packet.digest, true)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    f.store.revoke_key(f.reviewer.principal_id).await.unwrap();
    let mut tx = f.store.begin(f.other.clone()).await.unwrap();
    assert!(matches!(
        tx.publish_skill(review, &packet.digest).await,
        Err(StorageError::Forbidden)
    ));
    tx.rollback().await.unwrap();
    let foreign = Scope {
        workspace_id: Uuid::new_v4(),
        ..f.owner.scope
    };
    f.store
        .create_workspace(foreign.tenant_id, foreign.workspace_id, "foreign")
        .await
        .unwrap();
    let key = f.store.issue_key(foreign, "admin").await.unwrap();
    let auth = f.store.authenticate(&key.token).await.unwrap();
    let mut tx = f.store.begin(auth).await.unwrap();
    assert!(matches!(
        tx.skill_candidate(candidate).await,
        Err(StorageError::NotFound)
    ));
    assert!(matches!(
        tx.skill_review(review).await,
        Err(StorageError::NotFound)
    ));
    assert!(matches!(
        tx.skill_package(skill, 1).await,
        Err(StorageError::NotFound)
    ));
    tx.rollback().await.unwrap();
    f.store.shutdown().await.unwrap();
}
