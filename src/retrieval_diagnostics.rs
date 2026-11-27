//! Request-local, bounded observations. No query, vector, filter, URL or credentials.
use crate::evidence::EvidenceRoute;
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Instant,
};
use uuid::Uuid;

#[cfg(any(feature = "local-storage", test))]
use std::future::Future;

const LIMIT: usize = 128;
tokio::task_local! { static OBSERVATIONS: Arc<Mutex<Collector>>; static ROUTE: EvidenceRoute; }
#[derive(Default)]
struct Collector {
    requests: VecDeque<QueryObservation>,
    omitted: usize,
    #[cfg(any(feature = "local-storage", test))]
    context_query_embedding: Option<ContextQueryEmbedding>,
}

/// Content-free backend configuration verified at collection admission.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheckedConfig {
    pub engine_version: String,
    pub analysis_fingerprint: String,
    pub embedding_space_sha256: Option<String>,
    pub max_candidates: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_records: Option<u64>,
}

/// A backend observation; progress and results are sampled at different instants.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackendQuery {
    pub format_version: u32,
    pub request_id: String,
    pub engine_version: String,
    pub mode: String,
    pub elapsed_us: u64,
    pub analysis_fingerprint: String,
    pub index_progress: IndexProgress,
    pub embedding_space_sha256: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexProgress {
    pub committed_seq: Option<u64>,
    pub lexical_seq: u64,
    pub vector_seq: u64,
    pub index_failed: bool,
    pub recovery_required: bool,
    pub sampling: String,
}

/// One observed AnySeek query through HTTP or the embedded SDK, including preflights.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueryObservation {
    pub request_id: Uuid,
    pub route: Option<EvidenceRoute>,
    pub mode: String,
    pub elapsed_us: u64,
    pub http_status: Option<u16>,
    pub backend_request_id: Option<String>,
    pub backend_code: Option<String>,
    pub outcome: String,
    pub visible_seq: Option<u64>,
    pub returned_candidates: Option<usize>,
    pub backend: Option<BackendQuery>,
    pub checked_config: Option<CheckedConfig>,
}

/// Input usage claimed by the configured embedding endpoint for this one
/// Context query. Not an independent count, Agent usage, or cost.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextQueryEmbedding {
    pub model_config_sha256: String,
    pub provider_reported_prompt_tokens: Option<u32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RetrievalDiagnostics {
    /// Most recent requests in execution order; earlier omissions are explicit.
    pub requests: Vec<QueryObservation>,
    pub omitted_requests: usize,
    // Omit a missing observation so old prepared bundle digests still verify.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_query_embedding: Option<ContextQueryEmbedding>,
}

#[cfg(any(feature = "local-storage", test))]
pub(crate) async fn capture<F: Future>(future: F) -> (F::Output, RetrievalDiagnostics) {
    let collector = Arc::new(Mutex::new(Collector::default()));
    let result = OBSERVATIONS.scope(collector.clone(), future).await;
    let mut observations = collector.lock().unwrap_or_else(|p| p.into_inner());
    let trace = RetrievalDiagnostics {
        requests: observations.requests.drain(..).collect(),
        omitted_requests: observations.omitted,
        context_query_embedding: observations.context_query_embedding.take(),
    };
    (result, trace)
}

#[cfg(any(feature = "local-storage", test))]
pub(crate) async fn in_route<F: Future>(route: EvidenceRoute, future: F) -> F::Output {
    ROUTE.scope(route, future).await
}

#[cfg(feature = "local-storage")]
pub(crate) fn record_context_query_embedding(entry: ContextQueryEmbedding) {
    let _ = OBSERVATIONS.try_with(|collector| {
        let mut data = collector.lock().unwrap_or_else(|p| p.into_inner());
        data.context_query_embedding = Some(entry);
    });
}

pub(crate) fn record(entry: QueryObservation) {
    let _ = OBSERVATIONS.try_with(|collector| {
        let mut data = collector.lock().unwrap_or_else(|p| p.into_inner());
        if data.requests.len() == LIMIT {
            data.requests.pop_front();
            data.omitted += 1;
        }
        data.requests.push_back(entry);
    });
}

impl QueryObservation {
    pub(crate) fn new(mode: &str, config: Option<CheckedConfig>) -> Self {
        Self {
            request_id: Uuid::new_v4(),
            route: ROUTE.try_with(|v| *v).ok(),
            mode: mode.into(),
            elapsed_us: 0,
            http_status: None,
            backend_request_id: None,
            backend_code: None,
            outcome: "not_sent".into(),
            visible_seq: None,
            returned_candidates: None,
            backend: None,
            checked_config: config,
        }
    }
    pub(crate) fn elapsed(&mut self, started: Instant) {
        self.elapsed_us = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
    }
}

pub(crate) fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(crate) fn request_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(crate) fn version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-+_".contains(&b))
}
pub(crate) fn error_code(value: &str) -> Option<String> {
    [
        "busy",
        "recovery_required",
        "storage_error",
        "clock_uncertain",
        "unauthorized",
        "not_found",
        "rejected",
        "conflict",
        "shutting_down",
        "output_timeout",
        "outcome_unknown",
        "durable_pending",
        "operation_expired",
    ]
    .contains(&value)
    .then(|| value.to_owned())
}

impl BackendQuery {
    pub(crate) fn valid(
        &self,
        expected_mode: &str,
        header_id: Option<&str>,
        config: Option<&CheckedConfig>,
    ) -> bool {
        self.format_version == 2
            && request_id(&self.request_id)
            && version(&self.engine_version)
            && self.mode == expected_mode
            && header_id == Some(self.request_id.as_str())
            && digest(&self.analysis_fingerprint)
            && self.embedding_space_sha256.as_deref().is_none_or(digest)
            && self.index_progress.sampling == "after_query_not_atomic_with_result"
            && config.is_none_or(|c| {
                c.engine_version == self.engine_version
                    && c.analysis_fingerprint == self.analysis_fingerprint
                    && c.embedding_space_sha256 == self.embedding_space_sha256
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn earlier_diagnostics_serialize_identically_for_prepared_bundle_digest() {
        let earlier = r#"{"requests":[],"omitted_requests":0}"#;
        let parsed: RetrievalDiagnostics = serde_json::from_str(earlier).unwrap();
        assert!(parsed.context_query_embedding.is_none());
        assert_eq!(serde_json::to_string(&parsed).unwrap(), earlier);
    }

    #[tokio::test]
    async fn overlapping_requests_remain_isolated_and_overflow_is_explicit() {
        let one = capture(async {
            in_route(EvidenceRoute::Memory, async {
                for _ in 0..LIMIT + 3 {
                    record(QueryObservation::new("lexical", None));
                    tokio::task::yield_now().await;
                }
            })
            .await;
        });
        let two = capture(async {
            in_route(EvidenceRoute::Knowledge, async {
                record(QueryObservation::new("vector", None));
                tokio::task::yield_now().await;
            })
            .await;
        });
        let ((_, first), (_, second)) = tokio::join!(one, two);
        assert_eq!(first.requests.len(), LIMIT);
        assert_eq!(first.omitted_requests, 3);
        assert!(
            first
                .requests
                .iter()
                .all(|r| r.route == Some(EvidenceRoute::Memory) && r.mode == "lexical")
        );
        assert_eq!(second.requests.len(), 1);
        assert_eq!(second.requests[0].route, Some(EvidenceRoute::Knowledge));
        assert_eq!(second.omitted_requests, 0);
    }
}
