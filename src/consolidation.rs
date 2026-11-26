//! Long-term conclusions derived from independently stored, cited facts.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{memory_identity::MemoryIdentity, memory_semantics::Statement};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroundedFact {
    pub identity: MemoryIdentity,
    pub statement: Statement,
    pub effective_at_ms: Option<i64>,
}

fn contains_literal(
    source: &str,
    statement: &Statement,
    literal: &str,
    matches_context: impl Fn(&str, &str) -> bool,
) -> bool {
    let Some(&first) = literal.as_bytes().first() else {
        return false;
    };
    let last = literal.as_bytes()[literal.len() - 1];
    let Some(quote) = source.get(statement.byte_start..statement.byte_end) else {
        return false;
    };
    let word_char = |c: char| c.is_ascii_alphanumeric() || c.is_numeric();
    quote.match_indices(literal).any(|(offset, _)| {
        let start = statement.byte_start + offset;
        let end = start + literal.len();
        let mut prefix = source[..start].chars().rev();
        let mut suffix = source[end..].chars();
        let before = prefix.next();
        let after = suffix.next();
        let left_bound = !first.is_ascii_alphanumeric()
            || !(before.is_some_and(word_char)
                || matches!(before, Some('-' | '_' | '.'))
                    && prefix.clone().next().is_some_and(word_char));
        let right_bound = !last.is_ascii_alphanumeric()
            || !(after.is_some_and(word_char)
                || matches!(after, Some('-' | '_' | '.'))
                    && suffix.clone().next().is_some_and(word_char));
        let inside_number = first.is_ascii_digit()
            && match before {
                Some('+' | '-' | '−' | '＋' | '－') => true,
                Some('.' | ',' | '．' | '，') => prefix.next().is_some_and(char::is_numeric),
                _ => false,
            };
        let complete_number = !last.is_ascii_digit()
            || !matches!(after, Some('.' | ',' | '．' | '，'))
            || suffix.next().is_none_or(|c| !c.is_numeric());
        left_bound
            && right_bound
            && !inside_number
            && complete_number
            && before != Some('非')
            && matches_context(&quote[..offset], &quote[offset + literal.len()..])
    })
}

impl GroundedFact {
    pub(crate) fn validate_grounding(&self, content: &str) -> crate::error::Result<()> {
        use crate::error::AppError;
        let statement = &self.statement;
        let literal = |value: &str| contains_literal(content, statement, value, |_, _| true);
        self.identity
            .validate()
            .map_err(|_| AppError::Invalid("invalid extracted memory identity".into()))?;
        if statement.value.is_empty()
            || statement.value.len() > 256
            || statement.value.trim() != statement.value
            || statement.value.chars().any(char::is_control)
            || statement.quote.len() > 8192
            || content.get(statement.byte_start..statement.byte_end)
                != Some(statement.quote.as_str())
            || !literal(&statement.value)
            || !literal(&self.identity.subject.stable_id)
            || self
                .identity
                .context
                .values()
                .any(|condition| !literal(condition))
        {
            return Err(AppError::Invalid(
                "extracted fact is not grounded in its source span".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub source_event_id: Uuid,
    pub statement: Statement,
    pub effective_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRevision {
    pub memory_id: Uuid,
    pub version: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ConclusionState {
    Unknown,
    Conflicting,
    Current { value: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryPeriod {
    pub effective_from_ms: Option<i64>,
    pub state: ConclusionState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryView {
    pub memory_id: Uuid,
    pub version: i32,
    pub identity: MemoryIdentity,
    pub state: ConclusionState,
    pub timeline: Vec<MemoryPeriod>,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecallInput {
    pub identity: MemoryIdentity,
    pub version: Option<i32>,
    pub as_of_ms: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecallVersion {
    pub version: Option<i32>,
    pub as_of_ms: Option<i64>,
}

impl MemoryView {
    pub(crate) fn from_evidence(
        revision: MemoryRevision,
        identity: MemoryIdentity,
        evidence: Vec<Evidence>,
        as_of_ms: i64,
    ) -> Self {
        let mut by_date = BTreeMap::<_, BTreeSet<&str>>::new();
        for fact in &evidence {
            by_date
                .entry(fact.effective_at_ms)
                .or_default()
                .insert(&fact.statement.value);
        }
        let mut timeline: Vec<MemoryPeriod> = Vec::new();
        for (effective_from_ms, values) in by_date {
            let state = match values.first().filter(|_| values.len() == 1) {
                Some(value) => ConclusionState::Current {
                    value: (*value).to_owned(),
                },
                None => ConclusionState::Conflicting,
            };
            if timeline
                .last()
                .is_none_or(|previous| previous.state != state)
            {
                timeline.push(MemoryPeriod {
                    effective_from_ms,
                    state,
                });
            }
        }
        let state = timeline
            .iter()
            .rev()
            .find(|period| {
                period
                    .effective_from_ms
                    .is_none_or(|start| start <= as_of_ms)
            })
            .map_or(ConclusionState::Unknown, |period| period.state.clone());
        Self {
            memory_id: revision.memory_id,
            version: revision.version,
            identity,
            state,
            timeline,
            evidence,
        }
    }
}

#[cfg(feature = "local-storage")]
mod retain {
    use super::*;
    use crate::{
        error::{AppError, Result},
        jobs::JobPayload,
        service::Service,
        storage::{DomainTx, Permission, sqlite::now_ms},
        types::{AuthContext, CaptureInput},
    };
    use serde_json::{Value, json};
    use std::sync::LazyLock;

    const MAX_CONTENT: usize = 64 * 1024;
    const MAX_FACTS: usize = 20;
    static TIME_QUALIFIER: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
            r"(?i)[0-9]{4}[-/][0-9]{1,2}[-/][0-9]{1,2}|[0-9]{1,2}\s*月\s*[0-9]{1,2}\s*[日号]|\b(?:tomorrow|yesterday|next (?:week|month|year))\b|明天|昨天|后天|下(?:周|个月|月)|明年",
        )
        .expect("static time qualifier pattern")
    });
    static BOUNDED_POLICY: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
            r"(?i)\b(?:until|expires?|valid through|temporarily)\b|有效期|截至|截止|到期|限时|临时",
        )
        .expect("static bounded policy pattern")
    });

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Extraction {
        facts: Vec<ExtractedFact>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ExtractedFact {
        identity: MemoryIdentity,
        statement: Statement,
        effective_date: Option<String>,
    }

    fn civil_midnight_ms(literal: &str) -> Option<i64> {
        let (year, month, day) = if let Some((year, tail)) = literal.split_once('年') {
            let (month, day) = tail.strip_suffix('日')?.split_once('月')?;
            (year.trim(), month.trim(), day.trim())
        } else {
            let (year, tail) = literal.split_once('-')?;
            let (month, day) = tail.split_once('-')?;
            if month.len() != 2 || day.len() != 2 {
                return None;
            }
            (year, month, day)
        };
        let valid_digits = |part: &str, max: usize| {
            !part.is_empty()
                && part.len() <= max
                && part.bytes().all(|digit| digit.is_ascii_digit())
        };
        if year.len() != 4
            || !valid_digits(year, 4)
            || !valid_digits(month, 2)
            || !valid_digits(day, 2)
            || year == "0000"
        {
            return None;
        }
        let date = chrono::NaiveDate::from_ymd_opt(
            year.parse().ok()?,
            month.parse().ok()?,
            day.parse().ok()?,
        )?;
        Some(date.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis())
    }

    fn labels_effective_date(before: &str, after: &str) -> bool {
        let after = after.trim_start();
        let before = before.trim_end().to_ascii_lowercase();
        after.strip_prefix('起').is_some_and(|rest| {
            rest.chars().next().is_none_or(|c| {
                c.is_whitespace() || "，、。；：,:.每为按在至执适调实采用施生".contains(c)
            })
        }) || after.starts_with("生效")
            || after.starts_with("开始执行")
            || [
                "effective from",
                "effective on",
                "starting on",
                "starting",
                "effective as of",
            ]
            .iter()
            .any(|label| {
                before.strip_suffix(label).is_some_and(|prefix| {
                    prefix
                        .chars()
                        .next_back()
                        .is_none_or(|c| !c.is_ascii_alphanumeric())
                })
            })
    }

    pub(crate) fn parse_extraction(content: &str, value: Value) -> Result<Vec<GroundedFact>> {
        let extraction: Extraction = serde_json::from_value(value)
            .map_err(|_| AppError::Invalid("invalid memory extraction schema".into()))?;
        if extraction.facts.len() > MAX_FACTS {
            return Err(AppError::Invalid("too many extracted facts".into()));
        }
        let time_qualified = TIME_QUALIFIER.is_match(content);
        let bounded_policy = BOUNDED_POLICY.is_match(content);
        let mut facts = Vec::new();
        for extracted in extraction.facts {
            let ExtractedFact {
                identity,
                statement,
                effective_date,
            } = extracted;
            let effective_at_ms = effective_date
                .map(|date| {
                    civil_midnight_ms(&date)
                        .filter(|_| {
                            contains_literal(content, &statement, &date, labels_effective_date)
                        })
                        .ok_or_else(|| {
                            AppError::Invalid(
                                "invalid or unsupported effective date in source span".into(),
                            )
                        })
                })
                .transpose()?;
            let fact = GroundedFact {
                identity,
                statement,
                effective_at_ms,
            };
            fact.validate_grounding(content)?;
            // A duration or a date elsewhere in the source may qualify this
            // claim. Preserve the source without promoting an undated guess.
            if bounded_policy || (time_qualified && fact.effective_at_ms.is_none()) {
                continue;
            }
            let already_recorded = facts.iter().any(|existing: &GroundedFact| {
                existing.identity == fact.identity
                    && existing.statement.value == fact.statement.value
                    && existing.effective_at_ms == fact.effective_at_ms
            });
            if !already_recorded {
                facts.push(fact);
            }
        }
        Ok(facts)
    }

    impl Service {
        /// Accept raw content and a durable derivation job in one transaction.
        pub async fn retain_memory(
            &self,
            auth: &AuthContext,
            key: &str,
            input: CaptureInput,
        ) -> Result<Value> {
            crate::parsing::validate_text(&input.content)?;
            if input.content.len() > MAX_CONTENT {
                return Err(AppError::Invalid("memory source exceeds 64 KiB".into()));
            }
            let (mut tx, replay) = self
                .command(auth, Permission::Write, "memory_retain", key, &json!(input))
                .await?;
            if let Some(receipt) = replay {
                return Ok(receipt);
            }
            tx.require_memory_consolidation().await?;
            let extraction_profile =
                self.models
                    .consolidation_extraction_profile()
                    .ok_or_else(|| {
                        AppError::Unavailable("memory extraction model is not configured".into())
                    })?;
            let extraction_model = self
                .models
                .summary_model()
                .ok_or_else(|| {
                    AppError::Unavailable("memory extraction model is not configured".into())
                })?
                .to_owned();
            let source = tx
                .create_event("memory_retain", &input.content, None)
                .await?;
            let job = Self::enqueue(
                &mut tx,
                JobPayload::ConsolidateMemory {
                    extraction_profile,
                    extraction_model,
                },
                None,
                Some(source),
            )
            .await?;
            tx.audit("memory.retain", source, json!({"job_id":job}))
                .await?;
            Self::finish(
                tx,
                json!({"source_event_id":source,"job_id":job,"state":"accepted"}),
            )
            .await
        }

        /// Read the current conclusion or an earlier supported revision by exact identity.
        pub async fn recall_long_term_memory(
            &self,
            auth: &AuthContext,
            input: RecallInput,
        ) -> Result<MemoryView> {
            input
                .identity
                .validate()
                .map_err(|_| AppError::Invalid("invalid memory identity".into()))?;
            validate_version(input.version)?;
            let mut tx = self.read(auth, Permission::Read).await?;
            let result = tx
                .long_term_memory(
                    &input.identity,
                    input.version,
                    input.as_of_ms.unwrap_or_else(now_ms),
                )
                .await?
                .ok_or(AppError::NotFound)?;
            tx.commit().await?;
            Ok(result)
        }

        pub async fn recall_long_term_memory_by_id(
            &self,
            auth: &AuthContext,
            id: Uuid,
            query: RecallVersion,
        ) -> Result<MemoryView> {
            validate_version(query.version)?;
            let mut tx = self.read(auth, Permission::Read).await?;
            let result = tx
                .long_term_memory_by_id(id, query.version, query.as_of_ms.unwrap_or_else(now_ms))
                .await?
                .ok_or(AppError::NotFound)?;
            tx.commit().await?;
            Ok(result)
        }
    }

    fn validate_version(version: Option<i32>) -> Result<()> {
        if version.is_some_and(|version| version < 1) {
            return Err(AppError::Invalid("memory version must be positive".into()));
        }
        Ok(())
    }
}

#[cfg(feature = "local-storage")]
pub(crate) use retain::parse_extraction;
