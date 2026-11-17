//! Immutable chunk content with unified lexical/vector indexes over HTTP.
//! Origence remains the visibility authority.
//!
//! A publication is immutable and uses one bounded Record group. Its UTC retry
//! epoch is saved in the ordinary publication plan before network IO. Each
//! application token is encoded as one SHA-256 word so engine query syntax and
//! a second tokenizer cannot reinterpret Origence's conjunctive lexical input.

use super::{
    FullTextEntry, FullTextHit, FullTextQuery, Scope, SourceVersion, StorageError, StorageResult,
    VectorEntry, VectorHit, VectorQuery, hash,
};
use reqwest::{Method, StatusCode, Url, header::HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

mod graph;
pub use graph::{GraphEdgeCandidate, GraphEntityCandidate, GraphPublication};

const FORMAT: &str = "origence-content-sha256-v1";
pub(crate) const MAX_GROUP: usize = 64;
const MAX_RESPONSE: usize = 8 * 1024 * 1024;
const MAX_REQUEST: usize = 2 * 1024 * 1024;
const MAX_CANDIDATES: usize = 10_000;
const META: &[&str] = &[
    "format",
    "kind",
    "tenant",
    "workspace",
    "chunk_id",
    "source_id",
    "source_version",
    "generation",
    "publication",
    "profile",
    "dimension",
];

/// Server-owned connection details. Credentials are deliberately not Debug.
pub struct AnySeekConfig {
    base: Url,
    key: HeaderValue,
    embedding: Option<(String, usize)>,
}

impl AnySeekConfig {
    #[cfg(feature = "local-storage")]
    pub(crate) fn binding_identity(&self) -> Value {
        json!({"format":"origence-content-backend-v2","backend":"anyseek","endpoint":self.base.as_str(),"routing":"tenant-uuid/workspace-ws-simple-uuid-v1","embedding":self.embedding})
    }

    /// CLI configuration. Libraries pass an explicit config instead of reading
    /// process environment; AnySeek connection details are required.
    pub fn from_env() -> StorageResult<Self> {
        match std::env::var("OC_FULL_TEXT_BACKEND").as_deref() {
            Err(std::env::VarError::NotPresent) | Ok("anyseek") => {
                let get = |name| {
                    std::env::var(name)
                        .map_err(|_| invalid("AnySeek URL and service key are required"))
                };
                Self::new(&get("OC_ANYSEEK_URL")?, &get("OC_ANYSEEK_API_KEY")?)
            }
            _ => Err(invalid("only the AnySeek retrieval backend is supported")),
        }
    }

    /// Pin the externally generated embedding space and dimension for this collection.
    pub fn with_embedding(mut self, profile: String, dimension: usize) -> StorageResult<Self> {
        if profile.is_empty() || !(1..=4096).contains(&dimension) {
            return Err(invalid("invalid embedding profile or dimension"));
        }
        if self
            .embedding
            .as_ref()
            .is_some_and(|old| old != &(profile.clone(), dimension))
        {
            return Err(invalid("embedding profile is already pinned"));
        }
        self.embedding = Some((profile, dimension));
        Ok(self)
    }

    /// Connect with a server-owned service credential; Scope determines routing.
    /// Require HTTPS except on loopback; do not accept URL credentials or paths.
    pub fn new(base: &str, key: &str) -> StorageResult<Self> {
        let base = Url::parse(base).map_err(|_| invalid("invalid AnySeek URL"))?;
        let local = matches!(
            base.host_str(),
            Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
        );
        if base.host_str().is_none()
            || (base.scheme() != "https" && !(local && base.scheme() == "http"))
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || base.path() != "/"
        {
            return Err(invalid(
                "AnySeek URL requires HTTPS (HTTP on loopback) and an origin without credentials",
            ));
        }
        if key.is_empty() {
            return Err(invalid("AnySeek service key is required"));
        }
        let mut key =
            HeaderValue::from_str(key).map_err(|_| invalid("invalid AnySeek service key"))?;
        key.set_sensitive(true);
        Ok(Self {
            base,
            key,
            embedding: None,
        })
    }
}

/// Immutable lexical/vector publications stored behind a trusted service key.
pub struct AnySeekFullTextStore {
    config: AnySeekConfig,
    http: reqwest::Client,
    checked_config: OnceLock<crate::retrieval_diagnostics::CheckedConfig>,
    checked_scopes: tokio::sync::Mutex<HashSet<(Uuid, Uuid)>>,
    #[cfg(feature = "local-storage")]
    binding: Option<super::retrieval_store::binding::Binding>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Document {
    format: String,
    kind: String,
    tenant: Uuid,
    workspace: Uuid,
    chunk_id: Uuid,
    source_id: Uuid,
    source_version: String,
    generation: String,
    publication: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    payload: Option<super::ChunkPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    terms: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dimension: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vector: Option<Vec<f32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    graph: Option<graph::Projection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    graph_keys: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    endpoint_ids: Option<Vec<String>>,
}

fn invalid(message: &'static str) -> StorageError {
    StorageError::InvalidArgument(message)
}
fn unavailable() -> StorageError {
    StorageError::Unavailable("AnySeek retrieval unavailable".into())
}
fn transient() -> StorageError {
    StorageError::TransientUnavailable("AnySeek transport unavailable".into())
}
fn inconsistent() -> StorageError {
    StorageError::Conflict("AnySeek publication identity mismatch".into())
}

/// Current UTC day, used once when preparing a durable publication operation.
pub fn operation_epoch() -> StorageResult<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| unavailable())?
        .as_secs()
        / 86400)
}

fn encode_terms(terms: &str, budget: usize) -> StorageResult<String> {
    let mut encoded = String::new();
    for word in terms.split_whitespace() {
        if encoded.len().saturating_add(66) > budget {
            return Err(invalid("AnySeek encoded terms exceed request budget"));
        }
        if !encoded.is_empty() {
            encoded.push(' ');
        }
        encoded.push('t');
        encoded.push_str(&hash(word));
    }
    Ok(encoded)
}

fn filter(field: &str, values: impl IntoIterator<Item = String>) -> Value {
    json!({"String":{"field":field,"filter":values.into_iter().collect::<Vec<_>>()}})
}

fn scope_filters(scope: Scope) -> Vec<Value> {
    vec![
        filter("format", [FORMAT.into()]),
        filter("tenant", [scope.tenant_id.to_string()]),
        filter("workspace", [scope.workspace_id.to_string()]),
    ]
}

fn record_id(doc: &Document) -> String {
    format!(
        "c{}",
        hash(format!(
            "{}:{}:{}:{}:{}:{}",
            doc.tenant,
            doc.workspace,
            doc.chunk_id,
            doc.source_id,
            doc.source_version,
            doc.generation
        ))
    )
}

fn decode_record(value: &Value, scope: Scope) -> StorageResult<(String, Document)> {
    let doc: Document =
        serde_json::from_value(value["document"].clone()).map_err(|_| inconsistent())?;
    if doc.format != FORMAT
        || doc.tenant != scope.tenant_id
        || doc.workspace != scope.workspace_id
        || value["record_id"].as_str() != Some(record_id(&doc).as_str())
        || doc
            .source_version
            .parse::<i32>()
            .ok()
            .filter(|v| *v > 0)
            .is_none()
        || doc
            .generation
            .parse::<i64>()
            .ok()
            .filter(|v| *v > 0)
            .is_none()
    {
        return Err(inconsistent());
    }
    graph::validate_document(&doc)?;
    Ok((record_id(&doc), doc))
}

impl AnySeekFullTextStore {
    /// Read immutable payloads by scoped chunk identity. Residual generations
    /// may repeat identical bytes; conflicting payloads never select a winner.
    /// Callers must verify these bytes against the authority's content proof.
    pub async fn read_payloads(
        &self,
        scope: Scope,
        ids: &[Uuid],
    ) -> StorageResult<HashMap<Uuid, super::ChunkPayload>> {
        let mut ids = ids.to_vec();
        ids.sort_unstable();
        ids.dedup();
        if ids.len() > MAX_CANDIDATES {
            return Err(invalid("AnySeek content read exceeds candidate budget"));
        }
        let mut payloads = HashMap::new();
        for batch in ids.chunks(MAX_GROUP) {
            let mut filters = scope_filters(scope);
            filters.push(filter("kind", ["chunk".into()]));
            filters.push(filter("chunk_id", batch.iter().map(Uuid::to_string)));
            let result = self
                .query(scope, filters, "", MAX_CANDIDATES, 0, true)
                .await?;
            let hits = result["hits"].as_array().ok_or_else(unavailable)?;
            if result["count_total"].as_u64() != Some(hits.len() as u64) {
                return Err(unavailable());
            }
            let mut seen = HashSet::new();
            for hit in hits {
                let (_, doc) = decode_record(&hit["record"], scope)?;
                if doc.kind != "chunk" || batch.binary_search(&doc.chunk_id).is_err() {
                    return Err(inconsistent());
                }
                let payload = doc.payload.ok_or_else(inconsistent)?;
                seen.insert(doc.chunk_id);
                match payloads.entry(doc.chunk_id) {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(payload);
                    }
                    std::collections::hash_map::Entry::Occupied(entry)
                        if entry.get() != &payload =>
                    {
                        return Err(inconsistent());
                    }
                    _ => {}
                }
            }
            if seen.len() != batch.len() {
                return Err(unavailable());
            }
        }
        Ok(payloads)
    }

    /// Construct a bounded client; collections follow authenticated workspace scopes.
    pub fn new(config: AnySeekConfig) -> StorageResult<Self> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| unavailable())?;
        Ok(Self {
            config,
            http,
            checked_config: OnceLock::new(),
            checked_scopes: tokio::sync::Mutex::new(HashSet::new()),
            #[cfg(feature = "local-storage")]
            binding: None,
        })
    }

    #[cfg(feature = "local-storage")]
    pub(crate) fn with_binding(
        mut self,
        binding: super::retrieval_store::binding::Binding,
    ) -> Self {
        self.binding = Some(binding);
        self
    }

    fn collection(scope: Scope) -> String {
        format!("ws_{}", scope.workspace_id.simple())
    }

    /// Exact collection configuration for a fresh dedicated lexical projection.
    pub fn collection_config() -> Value {
        let mut schema: Vec<Value> = META.iter().map(|field| json!({"field":field,"field_type":"String","store":true,"index_lexical":false,"index_vector":false,"filterable":true,"boost":1.0})).collect();
        schema.push(json!({"field":"terms","field_type":"Text","store":true,"index_lexical":true,"index_vector":false,"filterable":false,"boost":1.0}));
        schema.push(json!({"field":"payload","field_type":"Json","store":true,"index_lexical":false,"index_vector":false,"filterable":false,"boost":1.0}));
        schema.push(json!({"field":"graph","field_type":"Json","store":true,"index_lexical":false,"index_vector":false,"filterable":false,"boost":1.0}));
        schema.push(json!({"field":"endpoint_ids","field_type":"StringSet","store":true,"index_lexical":false,"index_vector":false,"filterable":true,"boost":1.0}));
        schema.push(json!({"field":"graph_keys","field_type":"StringSet","store":true,"index_lexical":false,"index_vector":false,"filterable":true,"boost":1.0}));
        json!({"index_name":FORMAT,"tokenizer":"TantivySimpleV1","text_normalization":"None","stop_words":"None","schema":schema})
    }

    /// Unified collection schema, with vectors generated by the caller's fixed profile.
    pub fn vector_collection_config(dimension: usize) -> StorageResult<Value> {
        if !(1..=4096).contains(&dimension) {
            return Err(invalid("invalid embedding dimension"));
        }
        let mut config = Self::collection_config();
        config["schema"].as_array_mut().unwrap().push(json!({"field":"vector","field_type":"Json","store":true,"index_lexical":false,"index_vector":true,"filterable":false,"boost":1.0}));
        config["vector"] = json!({"dimensions":dimension});
        Ok(config)
    }
    pub async fn check_vector(&self, profile: &str, dimension: usize) -> StorageResult<()> {
        if self.config.embedding.as_ref() != Some(&(profile.to_owned(), dimension)) {
            return Err(inconsistent());
        }
        self.check().await
    }

    async fn request(
        &self,
        scope: Scope,
        method: Method,
        suffix: &str,
        body: Option<Value>,
    ) -> StorageResult<(StatusCode, Value)> {
        self.ensure_scope(scope).await?;
        let mode = body
            .as_ref()
            .map(|body| {
                if body.get("query_vector").is_some() {
                    "vector"
                } else if body["query"].as_str().is_some_and(|s| !s.is_empty()) {
                    "lexical"
                } else {
                    "browse"
                }
            })
            .unwrap_or("browse");
        let mut observation = (suffix == "/query").then(|| {
            crate::retrieval_diagnostics::QueryObservation::new(
                mode,
                self.checked_config.get().cloned(),
            )
        });
        let started = std::time::Instant::now();
        let result = self
            .request_inner(
                Some(scope),
                method,
                &format!("/api/v1/index/{}{}", Self::collection(scope), suffix),
                body,
                &mut observation,
            )
            .await;
        if let Some(mut entry) = observation {
            entry.elapsed(started);
            if let Err(error) = &result
                && entry.outcome == "received"
            {
                entry.outcome = match error {
                    StorageError::TransientUnavailable(_) => "transport_unavailable",
                    StorageError::Conflict(_) => "invalid_backend_diagnostics",
                    _ => "invalid_response",
                }
                .into();
            }
            crate::retrieval_diagnostics::record(entry);
        }
        result
    }

    async fn request_inner(
        &self,
        scope: Option<Scope>,
        method: Method,
        path: &str,
        body: Option<Value>,
        observation: &mut Option<crate::retrieval_diagnostics::QueryObservation>,
    ) -> StorageResult<(StatusCode, Value)> {
        let url = self.config.base.join(path).map_err(|_| unavailable())?;
        let mut request = self
            .http
            .request(method, url)
            .header("apikey", self.config.key.clone());
        if let Some(scope) = scope {
            request = request.header("X-AnySeek-Tenant", scope.tenant_id.to_string());
        }
        if let Some(body) = body {
            let bytes = tokio::task::spawn_blocking(move || serde_json::to_vec(&body))
                .await
                .map_err(|_| unavailable())?
                .map_err(|_| unavailable())?;
            if bytes.len() > MAX_REQUEST {
                return Err(invalid("AnySeek request exceeds 2 MiB"));
            }
            request = request
                .header("Content-Type", "application/json")
                .body(bytes);
        }
        if let Some(entry) = observation {
            entry.outcome = "transport_unavailable".into();
        }
        let mut response = request.send().await.map_err(|_| transient())?;
        let status = response.status();
        if let Some(entry) = observation {
            entry.http_status = Some(status.as_u16());
            entry.backend_request_id = response
                .headers()
                .get("x-anyseek-request-id")
                .and_then(|s| s.to_str().ok())
                .filter(|s| crate::retrieval_diagnostics::request_id(s))
                .map(str::to_owned);
            entry.backend_code = response
                .headers()
                .get("x-anyseek-error-code")
                .and_then(|s| s.to_str().ok())
                .and_then(crate::retrieval_diagnostics::error_code);
            entry.outcome = if status.is_success() {
                "received"
            } else {
                "http_error"
            }
            .into();
        }
        if status.is_server_error()
            || status == StatusCode::TOO_MANY_REQUESTS
            || status == StatusCode::REQUEST_TIMEOUT
        {
            return Err(transient());
        }
        // Never expose a backend body, URL, key, or parser diagnostic.
        if !status.is_success() {
            return Ok((status, Value::Null));
        }
        let mut bytes = Vec::new();
        while let Some(part) = response.chunk().await.map_err(|_| transient())? {
            if part.len() > MAX_RESPONSE.saturating_sub(bytes.len()) {
                return Err(unavailable());
            }
            bytes.extend_from_slice(&part);
        }
        let value: Value = tokio::task::spawn_blocking(move || serde_json::from_slice(&bytes))
            .await
            .map_err(|_| unavailable())?
            .map_err(|_| unavailable())?;
        if let Some(entry) = observation {
            let backend: crate::retrieval_diagnostics::BackendQuery =
                serde_json::from_value(value["diagnostics"].clone()).map_err(|_| unavailable())?;
            if !backend.valid(
                &entry.mode,
                entry.backend_request_id.as_deref(),
                entry.checked_config.as_ref(),
            ) {
                entry.outcome = "invalid_backend_diagnostics".into();
                return Err(unavailable());
            }
            entry.visible_seq = value["visible_seq"].as_u64();
            entry.returned_candidates = value["hits"].as_array().map(Vec::len);
            if entry.visible_seq.is_none() {
                return Err(unavailable());
            }
            entry.backend = Some(backend);
            entry.outcome = "response_received_not_authority_verified".into();
        }
        Ok((status, value))
    }

    /// Service health needs no configured tenant or placeholder collection.
    pub async fn check(&self) -> StorageResult<()> {
        let (status, version) = self
            .request_inner(None, Method::GET, "/api/v1/live", None, &mut None)
            .await?;
        if !status.is_success()
            || version
                .as_str()
                .is_none_or(|s| !crate::retrieval_diagnostics::version(s))
        {
            return Err(unavailable());
        }
        let scopes: Vec<_> = self.checked_scopes.lock().await.iter().copied().collect();
        for (tenant_id, workspace_id) in scopes {
            self.check_scope(
                Scope {
                    tenant_id,
                    workspace_id,
                },
                false,
            )
            .await?;
        }
        Ok(())
    }

    async fn ensure_scope(&self, scope: Scope) -> StorageResult<()> {
        if !self
            .checked_scopes
            .lock()
            .await
            .contains(&(scope.tenant_id, scope.workspace_id))
        {
            self.check_scope(scope, true).await?;
        }
        Ok(())
    }

    /// Existing committed scopes pass create=false: missing data is never repaired.
    pub async fn check_scope(&self, scope: Scope, create: bool) -> StorageResult<()> {
        let mut scopes = self.checked_scopes.lock().await;
        let path = format!("/api/v1/index/{}", Self::collection(scope));
        let (mut status, mut info) = self
            .request_inner(Some(scope), Method::GET, &path, None, &mut None)
            .await?;
        let uninitialized = create && !scopes.contains(&(scope.tenant_id, scope.workspace_id));
        // AnySeek hides unknown scoped IDs behind 403. Confirm absence through
        // the authenticated namespace listing; a bare authorization error never
        // grants permission to provision or reuse a collection.
        let absent = if uninitialized && status == StatusCode::FORBIDDEN {
            let (listed, collections) = self
                .request_inner(Some(scope), Method::GET, "/api/v1/index", None, &mut None)
                .await?;
            if !listed.is_success() {
                return Err(unavailable());
            }
            let collections = collections.as_array().ok_or_else(unavailable)?;
            if collections.iter().any(|item| item["id"].as_str().is_none()) {
                return Err(unavailable());
            }
            !collections
                .iter()
                .any(|item| item["id"] == Self::collection(scope))
        } else {
            status == StatusCode::NOT_FOUND
        };
        if uninitialized && absent {
            let config = match &self.config.embedding {
                Some((_, d)) => Self::vector_collection_config(*d)?,
                None => Self::collection_config(),
            };
            let epoch = operation_epoch()?;
            let nonce = format!(
                "oc-init-{}",
                hash(
                    serde_json::to_vec(&(
                        scope.tenant_id,
                        scope.workspace_id,
                        &config,
                        &self.config.embedding
                    ))
                    .map_err(|_| inconsistent())?
                )
            );
            let body = json!({"operation_id":{"epoch":epoch,"nonce":nonce},"collection_id":Self::collection(scope),"config":config,"embedding_space_id":self.config.embedding.as_ref().map(|(p,_)|p)});
            // A race or lost create response is settled only by a strict read.
            let created = self
                .request_inner(
                    Some(scope),
                    Method::POST,
                    "/api/v1/index",
                    Some(body),
                    &mut None,
                )
                .await;
            (status, info) = self
                .request_inner(Some(scope), Method::GET, &path, None, &mut None)
                .await?;
            if !status.is_success() {
                created?;
                return Err(unavailable());
            }
        }
        if !status.is_success() {
            return Err(unavailable());
        }
        let expected = match &self.config.embedding {
            Some((_, d)) => Self::vector_collection_config(*d)?,
            None => Self::collection_config(),
        };
        if info["schema"] != expected["schema"]
            || info["name"] != FORMAT
            || info["text_normalization"] != "None"
            || info["vector_dimensions"]
                != self
                    .config
                    .embedding
                    .as_ref()
                    .map_or(Value::Null, |(_, d)| json!(d))
            || info["analysis"]["format"] != "native-package-analysis:v1"
            || info["analysis"]["tokenizer"] != "TantivySimpleV1"
            || info["analysis"]["tokenizer_identity"]
                != "tantivy-simple-v1;tantivy=0.27.0;lowercase;no-stopwords;no-length-filter"
            || info["analysis"]["stop_words"] != "None"
            || info["analysis"]["stop_words_fingerprint"] != hash(b"\"None\"")
            || info["analysis"]["fingerprint"]
                .as_str()
                .is_none_or(|s| !crate::retrieval_diagnostics::digest(s))
            || info["version"]
                .as_str()
                .is_none_or(|s| !crate::retrieval_diagnostics::version(s))
            || info["limits"]["max_candidates"]
                .as_u64()
                .is_none_or(|n| n == 0)
            || info["limits"]["max_records"]
                .as_u64()
                .is_none_or(|n| n == 0)
            || info["embedding_space_id"]
                != self
                    .config
                    .embedding
                    .as_ref()
                    .map_or(Value::Null, |(p, _)| json!(p))
        {
            return Err(StorageError::Conflict(
                "incompatible AnySeek retrieval collection".into(),
            ));
        }
        let checked = crate::retrieval_diagnostics::CheckedConfig {
            engine_version: info["version"].as_str().unwrap().to_owned(),
            analysis_fingerprint: info["analysis"]["fingerprint"].as_str().unwrap().to_owned(),
            embedding_space_sha256: self.config.embedding.as_ref().map(|(p, _)| hash(p)),
            max_candidates: info["limits"]["max_candidates"].as_u64().unwrap(),
            max_records: info["limits"]["max_records"].as_u64().unwrap(),
        };
        #[cfg(feature = "local-storage")]
        if let Some(binding) = &self.binding {
            binding
                .check_analysis(&checked.analysis_fingerprint)
                .await?;
        }
        if self.checked_config.get_or_init(|| checked.clone()) != &checked {
            return Err(inconsistent());
        }
        scopes.insert((scope.tenant_id, scope.workspace_id));
        Ok(())
    }

    async fn query(
        &self,
        scope: Scope,
        filters: Vec<Value>,
        query: &str,
        length: usize,
        offset: usize,
        all_fields: bool,
    ) -> StorageResult<Value> {
        let fields: Vec<&str> = if all_fields {
            Vec::new()
        } else {
            META.to_vec()
        };
        let (status, result) = self.request(scope, Method::POST, "/query", Some(json!({"query":query,"operator":"and","search_fields":["terms"],"filter":filters,"fields":fields,"length":length,"offset":offset,"count":true}))).await?;
        if !status.is_success() {
            return Err(unavailable());
        }
        if !result["hits"].is_array() || result["count_total"].as_u64().is_none() {
            return Err(unavailable());
        }
        Ok(result)
    }

    async fn group_version(&self, scope: Scope, group: &str) -> StorageResult<Option<u64>> {
        // Group IDs are generated lowercase hex, never caller-provided paths.
        if group.len() != 65
            || !group.starts_with('p')
            || !group[1..].bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(inconsistent());
        }
        let (status, result) = self
            .request(
                scope,
                Method::GET,
                &format!("/group?group_id={group}"),
                None,
            )
            .await?;
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() || result["group_id"] != group {
            return Err(unavailable());
        }
        result["group_version"]
            .as_u64()
            .filter(|v| *v > 0)
            .map(Some)
            .ok_or_else(unavailable)
    }

    async fn group_records(&self, scope: Scope, group: &str) -> StorageResult<Vec<Value>> {
        let mut filters = scope_filters(scope);
        filters.push(filter("publication", [group.into()]));
        let result = self
            .query(scope, filters, "", MAX_GROUP + 1, 0, true)
            .await?;
        if result["count_total"].as_u64().unwrap() > MAX_GROUP as u64 {
            return Err(inconsistent());
        }
        let hits = result["hits"].as_array().unwrap();
        if result["count_total"].as_u64() != Some(hits.len() as u64) {
            return Err(unavailable());
        }
        let mut rows = Vec::new();
        let mut seen = HashSet::new();
        for hit in hits {
            let (id, doc) = decode_record(&hit["record"], scope)?;
            if doc.publication != group
                || (doc.kind == "chunk" && doc.terms.is_none())
                || !seen.insert(id.clone())
            {
                return Err(inconsistent());
            }
            rows.push(json!({"record_id":id,"document":doc}));
        }
        let anchors: Vec<_> = rows
            .iter()
            .filter(|r| !r["document"]["graph"].is_null())
            .collect();
        if anchors.len() > 1 {
            return Err(inconsistent());
        }
        if let Some(anchor) = anchors.first() {
            let d = &anchor["document"];
            if rows.iter().any(|r| {
                r["document"]["source_id"] != d["source_id"]
                    || r["document"]["source_version"] != d["source_version"]
                    || r["document"]["generation"] != d["generation"]
                    || r["document"]["chunk_id"].as_str() < d["chunk_id"].as_str()
            }) {
                return Err(inconsistent());
            }
        }
        rows.sort_by(|a, b| a["record_id"].as_str().cmp(&b["record_id"].as_str()));
        Ok(rows)
    }

    /// Publish one immutable <=64-chunk group using the saved operation epoch.
    pub async fn upsert(
        &self,
        scope: Scope,
        entries: Vec<FullTextEntry>,
        epoch: u64,
    ) -> StorageResult<()> {
        self.upsert_combined(scope, entries, Vec::new(), epoch)
            .await
    }

    /// Publish lexical and optional vector data together under the durable retry identity.
    pub async fn upsert_combined(
        &self,
        scope: Scope,
        entries: Vec<FullTextEntry>,
        vectors: Vec<VectorEntry>,
        epoch: u64,
    ) -> StorageResult<()> {
        self.upsert_with_graph(scope, entries, vectors, None, epoch)
            .await
    }

    /// Publish chunks and owner-bound graph support in one bounded atomic group.
    pub async fn upsert_with_graph(
        &self,
        scope: Scope,
        entries: Vec<FullTextEntry>,
        vectors: Vec<VectorEntry>,
        graph: Option<GraphPublication>,
        epoch: u64,
    ) -> StorageResult<()> {
        if entries.len() > MAX_GROUP || (entries.is_empty() && graph.is_some()) {
            return Err(invalid("invalid AnySeek publication chunk count"));
        }
        let mut vector_map = HashMap::new();
        for vector in vectors {
            if self.config.embedding.as_ref() != Some(&(vector.profile.clone(), vector.dimension))
                || vector.embedding.len() != vector.dimension
                || vector.embedding.iter().any(|v| !v.is_finite())
                || vector.embedding.iter().all(|v| *v == 0.0)
                || vector_map.insert(vector.id, vector).is_some()
            {
                return Err(invalid("invalid publication embedding"));
            }
        }
        if entries.is_empty() {
            return if vector_map.is_empty() {
                Ok(())
            } else {
                Err(inconsistent())
            };
        }
        let (group, records, digest) = tokio::task::spawn_blocking(move || -> StorageResult<_> {
            let graph = graph::prepare(graph, &entries)?;
            let mut docs = BTreeMap::new();
            let mut remaining = MAX_REQUEST;
            for entry in entries {
                if entry.source.version <= 0 || entry.generation <= 0 {
                    return Err(invalid("invalid full-text publication version"));
                }
                let terms = encode_terms(&entry.search_terms, remaining)?;
                remaining -= terms.len();
                let vector = vector_map.remove(&entry.id);
                if vector
                    .as_ref()
                    .is_some_and(|v| v.source != entry.source || v.generation != entry.generation)
                {
                    return Err(inconsistent());
                }
                let doc = Document {
                    format: FORMAT.into(),
                    kind: "chunk".into(),
                    tenant: scope.tenant_id,
                    workspace: scope.workspace_id,
                    chunk_id: entry.id,
                    source_id: entry.source.source_id,
                    source_version: entry.source.version.to_string(),
                    generation: entry.generation.to_string(),
                    publication: String::new(),
                    payload: Some(entry.payload),
                    terms: Some(terms),
                    profile: vector.as_ref().map(|v| v.profile.clone()),
                    dimension: vector.as_ref().map(|v| v.dimension.to_string()),
                    vector: vector.map(|v| v.embedding),
                    graph: None,
                    graph_keys: None,
                    endpoint_ids: None,
                };
                let id = record_id(&doc);
                if let Some(old) = docs.insert(id, doc.clone())
                    && old != doc
                {
                    return Err(inconsistent());
                }
            }
            if !vector_map.is_empty() {
                return Err(inconsistent());
            }
            graph::attach(&mut docs, graph)?;
            let group = format!(
                "p{}",
                hash(docs.keys().cloned().collect::<Vec<_>>().join(":"))
            );
            let records: Vec<Value> = docs
                .into_iter()
                .map(|(id, mut doc)| {
                    doc.publication = group.clone();
                    json!({"record_id":id,"document":doc})
                })
                .collect();
            let digest = hash(serde_json::to_vec(&records).map_err(|_| unavailable())?);
            Ok((group, records, digest))
        })
        .await
        .map_err(|_| unavailable())??;
        // A matching group settles a lost response without a new write. Startup
        // cleanup may have cleared an uncommitted projection; its active job
        // can rebuild the empty group under a new CAS and the saved epoch.
        let expected_version = if let Some(version) = self.group_version(scope, &group).await? {
            let current = self.group_records(scope, &group).await?;
            if self.group_version(scope, &group).await? != Some(version) {
                return Err(unavailable());
            }
            if current == records {
                return Ok(());
            }
            if !current.is_empty() {
                return Err(inconsistent());
            }
            version
        } else {
            0
        };
        let next_version = expected_version.checked_add(1).ok_or_else(inconsistent)?;
        let nonce = format!("origence:{}", hash(format!("{expected_version}:{digest}")));
        let request = json!({"operation_id":{"epoch":epoch,"nonce":nonce},"group_id":group,"expected_group_version":expected_version,"records":records});
        let (status, result) = self
            .request(scope, Method::POST, "/group/replace", Some(request))
            .await?;
        if !status.is_success() {
            return Err(unavailable());
        }
        if result["atomic"] != true
            || result["operation_id"]["epoch"] != epoch
            || result["operation_id"]["nonce"] != nonce
            || result["receipt"]["group_id"] != group
            || result["receipt"]["group_version"].as_u64() != Some(next_version)
            || result["receipt"]["record_count"].as_u64() != Some(records.len() as u64)
        {
            return Err(inconsistent());
        }
        Ok(())
    }

    /// Preflight every eligible identity before ranking, then recheck each hit.
    /// The caller must still perform final live authority/permission checks.
    pub async fn search(
        &self,
        mut query: FullTextQuery,
        expected: &HashMap<Uuid, SourceVersion>,
    ) -> StorageResult<Vec<FullTextHit>> {
        let ids = query
            .artifact_ids
            .as_mut()
            .ok_or_else(|| invalid("AnySeek requires authorized candidate IDs"))?;
        ids.sort();
        ids.dedup();
        if ids.len() > MAX_CANDIDATES || query.limit > 100 {
            return Err(invalid("AnySeek candidate budget exceeded"));
        }
        if ids.is_empty() || query.limit == 0 || query.search_terms.trim().is_empty() {
            return Ok(Vec::new());
        }
        for batch in ids.chunks(MAX_GROUP) {
            let mut filters = scope_filters(query.scope);
            filters.push(filter("kind", ["chunk".into()]));
            filters.push(filter("generation", [query.generation.to_string()]));
            filters.push(filter("chunk_id", batch.iter().map(Uuid::to_string)));
            let result = self
                .query(query.scope, filters, "", batch.len() + 1, 0, false)
                .await?;
            let hits = result["hits"].as_array().unwrap();
            if result["count_total"].as_u64() != Some(batch.len() as u64)
                || hits.len() != batch.len()
            {
                return Err(unavailable());
            }
            let mut seen = HashSet::new();
            for hit in hits {
                let (_, doc) = decode_record(&hit["record"], query.scope)?;
                if doc.kind != "chunk"
                    || batch.binary_search(&doc.chunk_id).is_err()
                    || !seen.insert(doc.chunk_id)
                    || doc.generation != query.generation.to_string()
                    || expected.get(&doc.chunk_id)
                        != Some(&SourceVersion {
                            source_id: doc.source_id,
                            version: doc.source_version.parse().map_err(|_| inconsistent())?,
                        })
                {
                    return Err(inconsistent());
                }
            }
        }
        let mut filters = scope_filters(query.scope);
        filters.push(filter("kind", ["chunk".into()]));
        filters.push(filter("generation", [query.generation.to_string()]));
        filters.push(filter("chunk_id", ids.iter().map(Uuid::to_string)));
        let result = self
            .query(
                query.scope,
                filters,
                &encode_terms(&query.search_terms, MAX_REQUEST)?,
                query.limit,
                0,
                false,
            )
            .await?;
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let hits = result["hits"].as_array().unwrap();
        if hits.len() > query.limit {
            return Err(inconsistent());
        }
        for hit in hits {
            let (_, doc) = decode_record(&hit["record"], query.scope)?;
            let source = SourceVersion {
                source_id: doc.source_id,
                version: doc.source_version.parse().map_err(|_| inconsistent())?,
            };
            let score = hit["score"]
                .as_f64()
                .map(|s| s as f32)
                .filter(|s| s.is_finite())
                .ok_or_else(inconsistent)?;
            if doc.kind != "chunk"
                || ids.binary_search(&doc.chunk_id).is_err()
                || !seen.insert(doc.chunk_id)
                || doc.generation != query.generation.to_string()
                || expected.get(&doc.chunk_id) != Some(&source)
            {
                return Err(inconsistent());
            }
            out.push(FullTextHit {
                id: doc.chunk_id,
                source,
                score,
            });
        }
        Ok(out)
    }

    /// Vector candidates from the same scoped, versioned projection as lexical search.
    pub async fn search_vector(
        &self,
        mut query: VectorQuery,
        expected: &HashMap<Uuid, SourceVersion>,
    ) -> StorageResult<Vec<VectorHit>> {
        if self.config.embedding.as_ref() != Some(&(query.profile.clone(), query.dimension))
            || query.embedding.len() != query.dimension
            || query.embedding.iter().any(|v| !v.is_finite())
            || query.embedding.iter().all(|v| *v == 0.0)
        {
            return Err(invalid("invalid query embedding"));
        }
        let ids = query
            .artifact_ids
            .as_mut()
            .ok_or_else(|| invalid("AnySeek requires authorized candidate IDs"))?;
        ids.sort();
        ids.dedup();
        if ids.len() > MAX_CANDIDATES || query.limit > 100 {
            return Err(invalid("AnySeek candidate budget exceeded"));
        }
        if ids.is_empty() || query.limit == 0 {
            return Ok(Vec::new());
        }
        for batch in ids.chunks(MAX_GROUP) {
            let mut filters = scope_filters(query.scope);
            filters.push(filter("kind", ["chunk".into()]));
            filters.push(filter("generation", [query.generation.to_string()]));
            filters.push(filter("profile", [query.profile.clone()]));
            filters.push(filter("dimension", [query.dimension.to_string()]));
            filters.push(filter("chunk_id", batch.iter().map(Uuid::to_string)));
            let result = self
                .query(query.scope, filters, "", batch.len() + 1, 0, false)
                .await?;
            let hits = result["hits"].as_array().unwrap();
            if result["count_total"].as_u64() != Some(batch.len() as u64)
                || hits.len() != batch.len()
            {
                return Err(unavailable());
            }
            let mut seen = HashSet::new();
            for hit in hits {
                let (_, doc) = decode_record(&hit["record"], query.scope)?;
                if doc.kind != "chunk"
                    || batch.binary_search(&doc.chunk_id).is_err()
                    || !seen.insert(doc.chunk_id)
                    || doc.generation != query.generation.to_string()
                    || doc.profile.as_deref() != Some(query.profile.as_str())
                    || doc.dimension.as_deref() != Some(query.dimension.to_string().as_str())
                    || expected.get(&doc.chunk_id)
                        != Some(&SourceVersion {
                            source_id: doc.source_id,
                            version: doc.source_version.parse().map_err(|_| inconsistent())?,
                        })
                {
                    return Err(inconsistent());
                }
            }
        }
        let mut filters = scope_filters(query.scope);
        filters.push(filter("kind", ["chunk".into()]));
        filters.push(filter("generation", [query.generation.to_string()]));
        filters.push(filter("profile", [query.profile.clone()]));
        filters.push(filter("dimension", [query.dimension.to_string()]));
        filters.push(filter("chunk_id", ids.iter().map(Uuid::to_string)));
        let (status, result) = self.request(query.scope, Method::POST, "/query", Some(json!({"query_vector":query.embedding,"all":true,"filter":filters,"fields":META,"length":query.limit,"count":true}))).await?;
        if !status.is_success() || !result["hits"].is_array() {
            return Err(unavailable());
        }
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let hits = result["hits"].as_array().unwrap();
        if hits.len() > query.limit {
            return Err(inconsistent());
        }
        for hit in hits {
            let (_, doc) = decode_record(&hit["record"], query.scope)?;
            let source = SourceVersion {
                source_id: doc.source_id,
                version: doc.source_version.parse().map_err(|_| inconsistent())?,
            };
            let score = hit["score"]
                .as_f64()
                .map(|s| s as f32)
                .filter(|s| s.is_finite())
                .ok_or_else(inconsistent)?;
            if doc.kind != "chunk"
                || ids.binary_search(&doc.chunk_id).is_err()
                || !seen.insert(doc.chunk_id)
                || doc.generation != query.generation.to_string()
                || doc.profile.as_deref() != Some(query.profile.as_str())
                || doc.dimension.as_deref() != Some(query.dimension.to_string().as_str())
                || expected.get(&doc.chunk_id) != Some(&source)
            {
                return Err(inconsistent());
            }
            out.push(VectorHit {
                id: doc.chunk_id,
                source,
                score,
            });
        }
        Ok(out)
    }

    /// Reconcile orphan removal against a current group version. A lost cleanup
    /// response is resolved by reading current membership before another CAS.
    pub async fn delete_objects(&self, scope: Scope, ids: Vec<Uuid>) -> StorageResult<()> {
        let ids: HashSet<Uuid> = ids.into_iter().collect();
        if ids.is_empty() {
            return Ok(());
        }
        let wanted: Vec<_> = ids.iter().copied().collect();
        let mut groups = HashSet::new();
        for batch in wanted.chunks(MAX_GROUP) {
            let mut filters = scope_filters(scope);
            filters.push(filter("kind", ["chunk".into()]));
            filters.push(filter("chunk_id", batch.iter().map(Uuid::to_string)));
            let mut offset = 0;
            loop {
                let result = self
                    .query(scope, filters.clone(), "", MAX_GROUP, offset, false)
                    .await?;
                let count = result["count_total"].as_u64().unwrap();
                if count > MAX_CANDIDATES as u64 {
                    return Err(unavailable());
                }
                let hits = result["hits"].as_array().unwrap();
                for hit in hits {
                    let (_, doc) = decode_record(&hit["record"], scope)?;
                    if doc.kind != "chunk" || !ids.contains(&doc.chunk_id) {
                        return Err(inconsistent());
                    }
                    groups.insert(doc.publication);
                }
                offset += hits.len();
                if offset as u64 >= count {
                    break;
                }
                if hits.is_empty() {
                    return Err(unavailable());
                }
            }
        }
        for group in groups {
            let version = self
                .group_version(scope, &group)
                .await?
                .ok_or_else(inconsistent)?;
            let next_version = version.checked_add(1).ok_or_else(inconsistent)?;
            let rows = self.group_records(scope, &group).await?;
            if self.group_version(scope, &group).await? != Some(version) {
                return Err(unavailable());
            }
            let retained: Vec<_> = rows
                .iter()
                .filter(|row| {
                    serde_json::from_value::<Uuid>(row["document"]["chunk_id"].clone())
                        .is_ok_and(|id| !ids.contains(&id))
                })
                .cloned()
                .collect();
            if !retained.is_empty()
                && rows.iter().any(|row| {
                    !row["document"]["graph"].is_null()
                        && !retained.iter().any(|r| r["record_id"] == row["record_id"])
                })
            {
                return Err(invalid(
                    "graph anchor cleanup requires complete source publication",
                ));
            }
            if retained.len() == rows.len() {
                continue;
            }
            let nonce = format!(
                "cleanup:{}",
                hash(
                    serde_json::to_vec(&json!([group, version, retained]))
                        .map_err(|_| unavailable())?
                )
            );
            let (status,result)=self.request(scope, Method::POST,"/group/replace",Some(json!({"operation_id":{"epoch":operation_epoch()?,"nonce":nonce},"group_id":group,"expected_group_version":version,"records":retained}))).await?;
            if !status.is_success() {
                return Err(unavailable());
            }
            if result["atomic"] != true
                || result["receipt"]["group_id"] != group
                || result["receipt"]["group_version"].as_u64() != Some(next_version)
                || result["receipt"]["record_count"].as_u64() != Some(retained.len() as u64)
            {
                return Err(inconsistent());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod unified_tests {
    use super::*;
    fn store() -> AnySeekFullTextStore {
        AnySeekFullTextStore::new(
            AnySeekConfig::new("http://127.0.0.1:1", "test")
                .unwrap()
                .with_embedding("fixed-profile".into(), 2)
                .unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn unified_schema_and_binding_pin_embedding_space() {
        let config = AnySeekFullTextStore::vector_collection_config(2).unwrap();
        assert_eq!(config["vector"]["dimensions"], 2);
        assert!(
            config["schema"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["field"] == "vector" && f["index_vector"] == true)
        );
        assert!(AnySeekFullTextStore::vector_collection_config(0).is_err());
        assert!(
            AnySeekConfig::new("http://127.0.0.1:1", "key")
                .unwrap()
                .with_embedding(String::new(), 2)
                .is_err()
        );
        #[cfg(feature = "local-storage")]
        assert_eq!(
            store().config.binding_identity()["embedding"],
            json!(["fixed-profile", 2])
        );
    }
    #[cfg(feature = "local-storage")]
    #[test]
    fn binding_pins_workspace_routing_and_allows_service_credential_rotation() {
        let make = |key| {
            AnySeekConfig::new("http://127.0.0.1:8080", key)
                .unwrap()
                .binding_identity()
        };
        let selected = make("service-key-one");
        assert_eq!(selected, make("service-key-two"));
        assert_eq!(selected["format"], "origence-content-backend-v2");
        assert_eq!(
            selected["routing"],
            "tenant-uuid/workspace-ws-simple-uuid-v1"
        );
        assert!(!selected.to_string().contains("service-key"));
        assert!(selected.get("tenant").is_none());
        assert!(selected.get("collection").is_none());
    }
    #[tokio::test]
    async fn vector_validation_rejects_before_transport() {
        let scope = Scope {
            tenant_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
        };
        let source = SourceVersion {
            source_id: Uuid::new_v4(),
            version: 1,
        };
        for embedding in [vec![0.0, 0.0], vec![f32::NAN, 1.0], vec![1.0]] {
            let entry = VectorEntry {
                id: Uuid::new_v4(),
                embedding: embedding.clone(),
                profile: "fixed-profile".into(),
                dimension: 2,
                generation: 1,
                source,
            };
            assert!(matches!(
                store().upsert_combined(scope, vec![], vec![entry], 0).await,
                Err(StorageError::InvalidArgument(_))
            ));
            let query = VectorQuery {
                scope,
                artifact_ids: Some(vec![]),
                profile: "fixed-profile".into(),
                dimension: 2,
                generation: 1,
                embedding,
                limit: 10,
            };
            assert!(matches!(
                store().search_vector(query, &HashMap::new()).await,
                Err(StorageError::InvalidArgument(_))
            ));
        }
        let orphan = VectorEntry {
            id: Uuid::new_v4(),
            embedding: vec![1.0, 0.0],
            profile: "fixed-profile".into(),
            dimension: 2,
            generation: 1,
            source,
        };
        assert!(matches!(
            store()
                .upsert_combined(scope, vec![], vec![orphan], 0)
                .await,
            Err(StorageError::Conflict(_))
        ));
    }
}
