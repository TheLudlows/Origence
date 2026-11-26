//! Scoped source facts and durable evidence revisions for derived memories.
use super::*;
use crate::{
    consolidation::{Evidence, GroundedFact, MemoryRevision, MemoryView},
    memory_identity::MemoryIdentity,
    memory_semantics::{Cardinality, MemoryProperty},
};
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

const MAX_CONSOLIDATION_PROPERTIES: usize = 128;
const MAX_ACTIVE_FACTS_PER_MEMORY: i64 = 256;
const SOURCE_KIND: &str = "memory_retain";

struct TopicSlot {
    id: Uuid,
    current_version: i32,
    identity: MemoryIdentity,
}

type IndexedFact<'a> = (i64, &'a GroundedFact);

impl SqliteTx {
    async fn has_memory_consolidation(&mut self) -> StorageResult<bool> {
        sqlite_db::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='oc_long_term_memories')",
        )
        .fetch_one(&mut *self.tx)
        .await
        .map_err(sqlite_err)
    }

    pub(crate) async fn require_memory_consolidation(&mut self) -> StorageResult<()> {
        if !self.has_memory_consolidation().await? {
            return Err(StorageError::Unavailable(
                "memory consolidation requires a new database".into(),
            ));
        }
        Ok(())
    }

    pub(crate) async fn consolidation_properties(&mut self) -> StorageResult<Vec<MemoryProperty>> {
        let definitions: Vec<String> = sqlite_db::query_scalar(
            "SELECT definition FROM oc_memory_properties
             WHERE tenant_id=? AND workspace_id=? AND json_extract(definition,'$.cardinality')='single'
             ORDER BY predicate LIMIT ?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(MAX_CONSOLIDATION_PROPERTIES as i64 + 1)
        .fetch_all(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        if definitions.len() > MAX_CONSOLIDATION_PROPERTIES {
            return Err(StorageError::InvalidArgument(
                "too many single-value properties for memory retention",
            ));
        }
        definitions
            .into_iter()
            .map(|definition| {
                serde_json::from_str(&definition)
                    .map_err(|_| StorageError::Unavailable("invalid memory property".into()))
            })
            .collect()
    }

    /// Save model decisions only while the caller holds the worker's job guard.
    pub(crate) async fn save_consolidation_plan(
        &mut self,
        job: Uuid,
        plan: &Value,
    ) -> StorageResult<()> {
        let changed = sqlite_db::query(
            "UPDATE oc_jobs
             SET payload=json_set(payload,'$.consolidation_plan',json(?)),updated_at=?
             WHERE tenant_id=? AND workspace_id=? AND id=?",
        )
        .bind(plan.to_string())
        .bind(now_ms())
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(job)
        .execute(&mut *self.tx)
        .await
        .map_err(sqlite_err)?
        .rows_affected();
        if changed != 1 {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }

    /// Publish all extracted facts and their derived revisions within the active
    /// job transaction. The caller completes the job in this same transaction.
    pub(crate) async fn consolidate_memory_facts(
        &mut self,
        source: Uuid,
        facts: &[GroundedFact],
    ) -> StorageResult<Vec<MemoryRevision>> {
        let body: String = sqlite_db::query_scalar(
            "SELECT content FROM oc_events
             WHERE tenant_id=? AND workspace_id=? AND id=? AND kind=? AND state='active' AND file_id IS NULL",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(source)
        .bind(SOURCE_KIND)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(sqlite_err)?
        .ok_or(StorageError::NotFound)?;

        let mut grouped: BTreeMap<String, Vec<IndexedFact<'_>>> = BTreeMap::new();
        let mut properties = BTreeMap::new();
        for (ordinal, fact) in facts.iter().enumerate() {
            fact.validate_grounding(&body)
                .map_err(|_| StorageError::InvalidArgument("invalid consolidated source fact"))?;
            let predicate = fact.identity.predicate.as_str();
            let property = match properties.entry(predicate) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => entry.insert(
                    self.memory_property(predicate)
                        .await?
                        .filter(|property| property.cardinality == Cardinality::Single)
                        .ok_or(StorageError::InvalidArgument(
                            "long-term memory requires a registered single-value property",
                        ))?,
                ),
            };
            property.check_identity(&fact.identity).map_err(|_| {
                StorageError::InvalidArgument("invalid long-term memory property context")
            })?;
            let key = fact
                .identity
                .key(self.scope)
                .map_err(|_| StorageError::InvalidArgument("invalid long-term memory identity"))?;
            grouped.entry(key).or_default().push((ordinal as i64, fact));
        }

        let existing: Vec<(i64, Uuid, String, Option<i64>, i32)> = sqlite_db::query_as(
            "SELECT ordinal,memory_id,statement,effective_at_ms,introduced_version
             FROM oc_long_term_memory_facts
             WHERE tenant_id=? AND workspace_id=? AND source_event_id=?
             ORDER BY ordinal LIMIT ?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(source)
        .bind(facts.len() as i64 + 1)
        .fetch_all(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        if !existing.is_empty() {
            if existing.len() != facts.len() {
                return Err(inconsistent_replay());
            }
            let mut revisions = Vec::with_capacity(grouped.len());
            for (key, entries) in grouped {
                let memory_id = self
                    .long_term_slot(&key)
                    .await?
                    .ok_or_else(inconsistent_replay)?
                    .id;
                let version = existing[entries[0].0 as usize].4;
                for (ordinal, fact) in entries {
                    let row = &existing[ordinal as usize];
                    if row.0 != ordinal
                        || row.1 != memory_id
                        || row.2 != encode(&fact.statement)?
                        || row.3 != fact.effective_at_ms
                        || row.4 != version
                    {
                        return Err(inconsistent_replay());
                    }
                }
                revisions.push(MemoryRevision { memory_id, version });
            }
            return Ok(revisions);
        }

        let mut revisions = Vec::with_capacity(grouped.len());
        for (key, entries) in grouped {
            let topic = match self.long_term_slot(&key).await? {
                Some(slot) => slot,
                None => {
                    self.create_long_term_slot(&key, &entries[0].1.identity)
                        .await?
                }
            };
            let memory_id = topic.id;
            let active_count: i64 = sqlite_db::query_scalar(
                "SELECT count(*) FROM oc_long_term_memory_facts
                 WHERE tenant_id=? AND workspace_id=? AND memory_id=? AND withdrawn_version IS NULL",
            )
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .bind(memory_id)
            .fetch_one(&mut *self.tx)
            .await
            .map_err(sqlite_err)?;
            if active_count + entries.len() as i64 > MAX_ACTIVE_FACTS_PER_MEMORY {
                return Err(StorageError::Conflict(
                    "long-term memory evidence limit reached".into(),
                ));
            }

            let version = self
                .append_long_term_revision(memory_id, topic.current_version)
                .await?;
            for (ordinal, fact) in entries {
                sqlite_db::query(
                    "INSERT INTO oc_long_term_memory_facts
                     (tenant_id,workspace_id,source_event_id,ordinal,memory_id,statement,effective_at_ms,introduced_version)
                     VALUES (?,?,?,?,?,?,?,?)",
                )
                .bind(self.scope.tenant_id)
                .bind(self.scope.workspace_id)
                .bind(source)
                .bind(ordinal)
                .bind(memory_id)
                .bind(encode(&fact.statement)?)
                .bind(fact.effective_at_ms)
                .bind(version)
                .execute(&mut *self.tx)
                .await
                .map_err(sqlite_err)?;
            }
            revisions.push(MemoryRevision { memory_id, version });
        }
        Ok(revisions)
    }

    /// Called after the original event is tombstoned, in that same transaction.
    pub(crate) async fn retract_consolidation_source(&mut self, source: Uuid) -> StorageResult<()> {
        let retained: bool = sqlite_db::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM oc_events
             WHERE tenant_id=? AND workspace_id=? AND id=? AND kind=? AND state='retracted')",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(source)
        .bind(SOURCE_KIND)
        .fetch_one(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        if !retained || !self.has_memory_consolidation().await? {
            return Ok(());
        }

        let affected: Vec<Uuid> = sqlite_db::query_scalar(
            "SELECT DISTINCT memory_id FROM oc_long_term_memory_facts
             WHERE tenant_id=? AND workspace_id=? AND source_event_id=? AND withdrawn_version IS NULL
             ORDER BY memory_id",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(source)
        .fetch_all(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        for memory_id in affected {
            let current: i32 = sqlite_db::query_scalar(
                "SELECT current_version FROM oc_long_term_memories
                 WHERE tenant_id=? AND workspace_id=? AND id=?",
            )
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .bind(memory_id)
            .fetch_one(&mut *self.tx)
            .await
            .map_err(sqlite_err)?;
            let version = self.append_long_term_revision(memory_id, current).await?;
            sqlite_db::query(
                "UPDATE oc_long_term_memory_facts SET withdrawn_version=?
                 WHERE tenant_id=? AND workspace_id=? AND source_event_id=? AND memory_id=? AND withdrawn_version IS NULL",
            )
            .bind(version)
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .bind(source)
            .bind(memory_id)
            .execute(&mut *self.tx)
            .await
            .map_err(sqlite_err)?;
        }
        Ok(())
    }

    pub(crate) async fn long_term_memory(
        &mut self,
        identity: &MemoryIdentity,
        version: Option<i32>,
        as_of_ms: i64,
    ) -> StorageResult<Option<MemoryView>> {
        self.require_memory_consolidation().await?;
        let key = identity
            .key(self.scope)
            .map_err(|_| StorageError::InvalidArgument("invalid long-term memory identity"))?;
        let Some(topic) = self.long_term_slot(&key).await? else {
            return Ok(None);
        };
        self.read_long_term_memory(topic, version, as_of_ms).await
    }

    pub(crate) async fn long_term_memory_by_id(
        &mut self,
        memory_id: Uuid,
        version: Option<i32>,
        as_of_ms: i64,
    ) -> StorageResult<Option<MemoryView>> {
        self.require_memory_consolidation().await?;
        let row: Option<(String, i32, String)> = sqlite_db::query_as(
            "SELECT identity_key,current_version,identity_json FROM oc_long_term_memories
             WHERE tenant_id=? AND workspace_id=? AND id=?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(memory_id)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        let Some((key, current, stored)) = row else {
            return Ok(None);
        };
        let topic = self.decode_long_term_slot(&key, memory_id, current, &stored)?;
        self.read_long_term_memory(topic, version, as_of_ms).await
    }

    async fn read_long_term_memory(
        &mut self,
        topic: TopicSlot,
        version: Option<i32>,
        as_of_ms: i64,
    ) -> StorageResult<Option<MemoryView>> {
        let memory_id = topic.id;
        let current = topic.current_version;
        let version = version.unwrap_or(current);
        if !(1..=current).contains(&version) {
            return Ok(None);
        }

        let rows = if version == current {
            sqlite_db::query_as(
                "SELECT source_event_id,statement,effective_at_ms FROM oc_long_term_memory_facts
                 WHERE tenant_id=? AND workspace_id=? AND memory_id=? AND withdrawn_version IS NULL
                 ORDER BY introduced_version,source_event_id,ordinal LIMIT ?",
            )
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .bind(memory_id)
            .bind(MAX_ACTIVE_FACTS_PER_MEMORY + 1)
            .fetch_all(&mut *self.tx)
            .await
        } else {
            sqlite_db::query_as(
                "SELECT source_event_id,statement,effective_at_ms FROM oc_long_term_memory_facts
                 WHERE tenant_id=? AND workspace_id=? AND memory_id=?
                   AND introduced_version<=? AND (withdrawn_version IS NULL OR withdrawn_version>?)
                 ORDER BY introduced_version,source_event_id,ordinal LIMIT ?",
            )
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .bind(memory_id)
            .bind(version)
            .bind(version)
            .bind(MAX_ACTIVE_FACTS_PER_MEMORY + 1)
            .fetch_all(&mut *self.tx)
            .await
        };
        let rows: Vec<(Uuid, String, Option<i64>)> = rows.map_err(sqlite_err)?;
        if rows.len() as i64 > MAX_ACTIVE_FACTS_PER_MEMORY {
            return Err(StorageError::Unavailable(
                "invalid long-term memory evidence count".into(),
            ));
        }
        let sources: BTreeSet<_> = rows.iter().map(|row| row.0).collect();
        if sources.is_empty() || !self.active_retain_sources(&sources).await? {
            return Ok(None);
        }
        let evidence: Vec<_> = rows
            .into_iter()
            .map(|(source_event_id, statement, effective_at_ms)| {
                Ok(Evidence {
                    source_event_id,
                    statement: serde_json::from_str(&statement).map_err(|_| {
                        StorageError::Unavailable("invalid long-term memory evidence".into())
                    })?,
                    effective_at_ms,
                })
            })
            .collect::<StorageResult<_>>()?;
        Ok(Some(MemoryView::from_evidence(
            MemoryRevision { memory_id, version },
            topic.identity,
            evidence,
            as_of_ms,
        )))
    }

    async fn active_retain_sources(&mut self, sources: &BTreeSet<Uuid>) -> StorageResult<bool> {
        let placeholders = vec!["?"; sources.len()].join(",");
        let sql = format!(
            "SELECT count(*) FROM oc_events
             WHERE tenant_id=? AND workspace_id=? AND kind=? AND state='active' AND file_id IS NULL
               AND id IN ({placeholders})"
        );
        let mut query = sqlite_db::query_scalar(&sql)
            .bind(self.scope.tenant_id)
            .bind(self.scope.workspace_id)
            .bind(SOURCE_KIND);
        for source in sources {
            query = query.bind(*source);
        }
        let visible: i64 = query.fetch_one(&mut *self.tx).await.map_err(sqlite_err)?;
        Ok(visible == sources.len() as i64)
    }

    async fn long_term_slot(&mut self, key: &str) -> StorageResult<Option<TopicSlot>> {
        let row: Option<(Uuid, i32, String)> = sqlite_db::query_as(
            "SELECT id,current_version,identity_json FROM oc_long_term_memories
             WHERE tenant_id=? AND workspace_id=? AND identity_key=?",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(key)
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        row.map(|(id, current, stored)| self.decode_long_term_slot(key, id, current, &stored))
            .transpose()
    }

    fn decode_long_term_slot(
        &self,
        key: &str,
        id: Uuid,
        current_version: i32,
        stored: &str,
    ) -> StorageResult<TopicSlot> {
        let invalid_identity =
            || StorageError::Unavailable("invalid stored long-term memory identity".into());
        let identity: MemoryIdentity =
            serde_json::from_str(stored).map_err(|_| invalid_identity())?;
        if identity.key(self.scope).map_err(|_| invalid_identity())? != key {
            return Err(invalid_identity());
        }
        Ok(TopicSlot {
            id,
            current_version,
            identity,
        })
    }

    async fn create_long_term_slot(
        &mut self,
        key: &str,
        identity: &MemoryIdentity,
    ) -> StorageResult<TopicSlot> {
        let memory_id = Uuid::new_v4();
        sqlite_db::query(
            "INSERT INTO oc_long_term_memories(tenant_id,workspace_id,id,identity_key,identity_json)
             VALUES (?,?,?,?,?)",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(memory_id)
        .bind(key)
        .bind(encode(identity)?)
        .execute(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        Ok(TopicSlot {
            id: memory_id,
            current_version: 0,
            identity: identity.clone(),
        })
    }

    async fn append_long_term_revision(
        &mut self,
        memory_id: Uuid,
        previous: i32,
    ) -> StorageResult<i32> {
        let version = previous.checked_add(1).ok_or_else(|| {
            StorageError::Conflict("long-term memory version limit reached".into())
        })?;
        sqlite_db::query(
            "INSERT INTO oc_long_term_memory_versions(tenant_id,workspace_id,memory_id,version,created_at)
             VALUES (?,?,?,?,?)",
        )
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(memory_id)
        .bind(version)
        .bind(now_ms())
        .execute(&mut *self.tx)
        .await
        .map_err(sqlite_err)?;
        let changed = sqlite_db::query(
            "UPDATE oc_long_term_memories SET current_version=?
             WHERE tenant_id=? AND workspace_id=? AND id=? AND current_version=?",
        )
        .bind(version)
        .bind(self.scope.tenant_id)
        .bind(self.scope.workspace_id)
        .bind(memory_id)
        .bind(previous)
        .execute(&mut *self.tx)
        .await
        .map_err(sqlite_err)?
        .rows_affected();
        if changed != 1 {
            return Err(StorageError::Conflict(
                "long-term memory version changed".into(),
            ));
        }
        Ok(version)
    }
}

fn encode(value: &impl serde::Serialize) -> StorageResult<String> {
    serde_json::to_string(value)
        .map_err(|_| StorageError::Unavailable("long-term memory encoding failed".into()))
}

fn inconsistent_replay() -> StorageError {
    StorageError::Conflict("retained source has a different consolidation plan".into())
}
