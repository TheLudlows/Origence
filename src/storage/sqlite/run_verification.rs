//! Persistence for external assessment references and owner transcriptions.
use super::*;
use crate::run_verification::{
    RunVerificationInput, VerificationInput, WithdrawRunVerificationInput,
};

fn decode<T: serde::de::DeserializeOwned>(s: &str) -> StorageResult<T> {
    serde_json::from_str(s)
        .map_err(|_| StorageError::Unavailable("invalid verification record".into()))
}
impl SqliteTx {
    async fn check_verification_prefix(
        &mut self,
        run: Uuid,
        input: &VerificationInput,
    ) -> StorageResult<()> {
        let (_, through_sequence, prefix_digest) = input.binding();
        let digest = self
            .private_run_prefix_digest(run, through_sequence)
            .await?;
        if digest != prefix_digest {
            return Err(StorageError::Conflict("Run prefix digest mismatch".into()));
        }
        Ok(())
    }
    async fn verification_record(&mut self, run: Uuid, id: Uuid) -> StorageResult<Value> {
        self.owned_run(run).await?;
        let row: Option<(String, Option<String>)> = sqlite_db::query_as(
            "SELECT record,withdrawal FROM oc_run_verifications WHERE tenant_id=? AND workspace_id=? AND run_id=? AND verification_id=?"
        ).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(run).bind(id)
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        let (record, withdrawal) = row.ok_or(StorageError::NotFound)?;
        let mut record: Value = decode(&record)?;
        self.check_verification_prefix(run, &assessment(&record)?)
            .await?;
        record["withdrawal"] = withdrawal
            .map(|w| decode(&w))
            .transpose()?
            .unwrap_or(Value::Null);
        Ok(record)
    }
    pub async fn record_run_verification(
        &mut self,
        run: Uuid,
        input: &RunVerificationInput,
    ) -> StorageResult<Value> {
        self.record_verification(run, &VerificationInput::Transcribed(input.clone()))
            .await
    }
    pub(crate) async fn record_verification(
        &mut self,
        run: Uuid,
        input: &VerificationInput,
    ) -> StorageResult<Value> {
        self.check_permission(Permission::Write).await?;
        input.validate().map_err(StorageError::InvalidArgument)?;
        self.check_verification_prefix(run, input).await?;
        let (id, _, _) = input.binding();
        let old: Option<String> = sqlite_db::query_scalar(
            "SELECT record FROM oc_run_verifications WHERE tenant_id=? AND workspace_id=? AND run_id=? AND verification_id=?"
        ).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(run).bind(id)
            .fetch_optional(&mut *self.tx).await.map_err(sqlite_err)?;
        if let Some(old) = old {
            if assessment(&decode(&old)?)? != *input {
                return Err(StorageError::Conflict(
                    "verification identity reused with different payload".into(),
                ));
            }
            return self.verification_record(run, id).await;
        }
        let count: i64 = sqlite_db::query_scalar("SELECT COUNT(*) FROM oc_run_verifications WHERE tenant_id=? AND workspace_id=? AND run_id=?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(run).fetch_one(&mut *self.tx).await.map_err(sqlite_err)?;
        if count >= 1000 {
            return Err(StorageError::InvalidArgument(
                "Run verification budget exceeded",
            ));
        }
        let origin = match input {
            VerificationInput::Linked(_) => "owner_linked_external",
            VerificationInput::Transcribed(_) => "owner_transcribed_external",
        };
        let record = json!({"run_id":run, "assessment":input,
            "recorder_principal_id":self.principal_id,"owner_key_id":self.private_key_id().await?,
            "recorded_at_ms":now_ms(), "evidence_origin":origin,
            "verifier_authentication":"unverified_claim", "platform_verification":"not_performed",
            "withdrawal":null});
        sqlite_db::query("INSERT INTO oc_run_verifications(tenant_id,workspace_id,run_id,verification_id,record) VALUES(?,?,?,?,?)")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(run).bind(id)
            .bind(record.to_string()).execute(&mut *self.tx).await.map_err(sqlite_err)?;
        Ok(record)
    }
    pub async fn run_verifications(
        &mut self,
        run: Uuid,
        after: Uuid,
        limit: usize,
    ) -> StorageResult<Value> {
        self.owned_run(run).await?;
        if !(1..=100).contains(&limit) {
            return Err(StorageError::InvalidArgument("invalid verification page"));
        }
        let ids: Vec<Uuid> = sqlite_db::query_scalar("SELECT verification_id FROM oc_run_verifications WHERE tenant_id=? AND workspace_id=? AND run_id=? AND verification_id>? ORDER BY verification_id LIMIT ?")
            .bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(run).bind(after).bind((limit+1) as i64)
            .fetch_all(&mut *self.tx).await.map_err(sqlite_err)?;
        let more = ids.len() > limit;
        let mut cursor = after;
        let mut items = Vec::new();
        for id in ids.into_iter().take(limit) {
            items.push(self.verification_record(run, id).await?);
            cursor = id;
        }
        Ok(json!({"items":items,"next_after":more.then_some(cursor)}))
    }
    pub async fn withdraw_run_verification(
        &mut self,
        run: Uuid,
        id: Uuid,
        input: &WithdrawRunVerificationInput,
    ) -> StorageResult<Value> {
        self.check_permission(Permission::Write).await?;
        input.validate().map_err(StorageError::InvalidArgument)?;
        let mut record = self.verification_record(run, id).await?;
        if !record["withdrawal"].is_null() {
            if record["withdrawal"]["reason"] != input.reason {
                return Err(StorageError::Conflict(
                    "verification already withdrawn with different reason".into(),
                ));
            }
            return Ok(record);
        }
        let withdrawal = json!({"reason":input.reason,"recorded_at_ms":now_ms(),"recorder_principal_id":self.principal_id});
        sqlite_db::query("UPDATE oc_run_verifications SET withdrawal=? WHERE tenant_id=? AND workspace_id=? AND run_id=? AND verification_id=?")
            .bind(withdrawal.to_string()).bind(self.scope.tenant_id).bind(self.scope.workspace_id).bind(run).bind(id)
            .execute(&mut *self.tx).await.map_err(sqlite_err)?;
        record["withdrawal"] = withdrawal;
        Ok(record)
    }
}

fn assessment(record: &Value) -> StorageResult<VerificationInput> {
    decode(&record["assessment"].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::{RunEvent, RunEventInput, RunInput, RunOutcome, RunReporter};
    use crate::run_verification::VerificationConclusion;
    async fn tx(pool: &Pool, scope: Scope, principal_id: Uuid) -> SqliteTx {
        SqliteTx {
            tx: pool.begin_with("BEGIN IMMEDIATE").await.unwrap(),
            scope,
            principal_id,
            required: Vec::new(),
            command: None,
        }
    }
    #[tokio::test]
    async fn verification_retry_conflict_withdrawal_reopen_and_key_isolation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("verification.db");
        let pool = Pool::open(&path).await.unwrap();
        {
            let mut c = pool.acquire().await.unwrap();
            super::super::schema::exec_schema(&mut c).await.unwrap();
        }
        let scope = Scope {
            tenant_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
        };
        sqlite_db::query(
            "INSERT INTO oc_workspaces(tenant_id,id,name,created_at) VALUES(?,?,'synthetic',0)",
        )
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .execute(&pool)
        .await
        .unwrap();
        let owner = Uuid::new_v4();
        let other = Uuid::new_v4();
        for id in [owner, other] {
            sqlite_db::query("INSERT INTO oc_api_keys(id,logical_key_id,tenant_id,workspace_id,token_hash,role,created_at) VALUES(?,?,?,?,?,'admin',0)")
                .bind(id).bind(id).bind(scope.tenant_id).bind(scope.workspace_id).bind(id.to_string()).execute(&pool).await.unwrap();
        }
        let run = Uuid::new_v4();
        let mut t = tx(&pool, scope, owner).await;
        t.create_private_run(&RunInput {
            run_id: run,
            summary: "synthetic task".into(),
        })
        .await
        .unwrap();
        t.append_private_run_event(
            run,
            &RunEventInput {
                event_id: Uuid::new_v4(),
                reporter: RunReporter::Test,
                observed_at_ms: 1,
                event: RunEvent::ResultReported {
                    outcome: RunOutcome::Success,
                    summary: "self-reported success".into(),
                },
            },
        )
        .await
        .unwrap();
        let input = RunVerificationInput {
            verification_id: Uuid::from_u128(1),
            through_sequence: 1,
            prefix_digest: t.private_run_prefix_digest(run, 1).await.unwrap(),
            verifier_label: "external tester (claimed)".into(),
            assessed_at_ms: 2,
            conclusion: VerificationConclusion::Failed,
            basis_summary: "expected output missing".into(),
            basis_sequences: vec![1],
            reported_skill_version: None,
        };
        let receipt = t.record_run_verification(run, &input).await.unwrap();
        assert_eq!(receipt["platform_verification"], "not_performed");
        assert_eq!(receipt["assessment"]["reported_skill_version"], Value::Null);
        assert_eq!(receipt["recorder_principal_id"], json!(owner));
        assert_eq!(
            receipt,
            t.record_run_verification(run, &input).await.unwrap()
        );
        let mut changed = input.clone();
        changed.conclusion = VerificationConclusion::Passed;
        assert!(matches!(
            t.record_run_verification(run, &changed).await,
            Err(StorageError::Conflict(_))
        ));
        changed = input.clone();
        changed.verification_id = Uuid::new_v4();
        changed.prefix_digest = "0".repeat(64);
        assert!(matches!(
            t.record_run_verification(run, &changed).await,
            Err(StorageError::Conflict(_))
        ));
        let trace = t.private_run_trace(run, 0, 10).await.unwrap();
        assert_eq!(trace["events"][0]["report"]["event"]["outcome"], "success");
        assert_eq!(trace["events"][0]["evidence_origin"], "reported");
        let mut second = input.clone();
        second.verification_id = Uuid::from_u128(2);
        second.reported_skill_version = Some(crate::run_verification::ReportedSkillVersion {
            skill_id: Uuid::new_v4(),
            version: 2,
            digest: "a".repeat(64),
        });
        t.record_run_verification(run, &second).await.unwrap();
        let page = t.run_verifications(run, Uuid::nil(), 1).await.unwrap();
        assert_eq!(page["next_after"], json!(input.verification_id));
        assert_eq!(page["items"][0], receipt);
        let page = t
            .run_verifications(run, input.verification_id, 1)
            .await
            .unwrap();
        assert!(page["next_after"].is_null());
        assert_eq!(
            page["items"][0]["assessment"]["reported_skill_version"]["version"],
            2
        );
        let revoke = WithdrawRunVerificationInput {
            reason: "external evidence retracted".into(),
        };
        let withdrawn = t
            .withdraw_run_verification(run, input.verification_id, &revoke)
            .await
            .unwrap();
        assert_eq!(
            withdrawn,
            t.withdraw_run_verification(run, input.verification_id, &revoke)
                .await
                .unwrap()
        );
        assert_eq!(
            withdrawn,
            t.record_run_verification(run, &input).await.unwrap()
        );
        assert!(matches!(
            t.withdraw_run_verification(
                run,
                input.verification_id,
                &WithdrawRunVerificationInput {
                    reason: "changed".into()
                }
            )
            .await,
            Err(StorageError::Conflict(_))
        ));
        t.commit().await.unwrap();
        let mut t = tx(&pool, scope, other).await;
        assert!(matches!(
            t.run_verifications(run, Uuid::nil(), 25).await,
            Err(StorageError::NotFound)
        ));
        assert!(matches!(
            t.record_run_verification(run, &input).await,
            Err(StorageError::NotFound)
        ));
        assert!(matches!(
            t.withdraw_run_verification(run, input.verification_id, &revoke)
                .await,
            Err(StorageError::NotFound)
        ));
        t.commit().await.unwrap();
        pool.shutdown().await.unwrap();
        drop(pool);
        let pool = Pool::open(&path).await.unwrap();
        let mut t = tx(&pool, scope, owner).await;
        assert_eq!(
            t.run_verifications(run, Uuid::nil(), 1).await.unwrap()["items"][0],
            withdrawn
        );
        assert!(t.run_verifications(run, Uuid::nil(), 101).await.is_err());
        t.commit().await.unwrap();
        sqlite_db::query("UPDATE oc_api_keys SET revoked=1 WHERE id=?")
            .bind(owner)
            .execute(&pool)
            .await
            .unwrap();
        let mut t = tx(&pool, scope, owner).await;
        assert!(matches!(
            t.run_verifications(run, Uuid::nil(), 25).await,
            Err(StorageError::Forbidden)
        ));
        assert!(matches!(
            t.record_run_verification(run, &input).await,
            Err(StorageError::Forbidden)
        ));
        assert!(matches!(
            t.withdraw_run_verification(run, input.verification_id, &revoke)
                .await,
            Err(StorageError::Forbidden)
        ));
    }
}
