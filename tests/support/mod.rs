//! Loopback protocol fixture, not an AnySeek engine or ranking-acceptance test.
//! Real SQLite, public Service/worker and HTTP adapter code remain under test.
#![allow(dead_code)] // Shared by two integration executables with different fixture needs.
use axum::{
    Json, Router,
    extract::{Path as RoutePath, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use origence::{
    models::Models,
    service::Service,
    storage::{
        self,
        anyseek::{AnySeekConfig, AnySeekFullTextStore},
        *,
    },
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};
use uuid::Uuid;

/// Compare deterministic context content across separate retrieval attempts.
pub fn without_retrieval_observations(value: &Value) -> Value {
    let mut value = value.clone();
    value.as_object_mut().unwrap().remove("retrieval");
    value
}

#[derive(Default)]
struct Data {
    content_read: Option<Arc<ContentReadGate>>,
    records: BTreeMap<String, Value>,
    collections: HashSet<(String, String)>,
    private_configs: HashMap<(String, String), Value>,
    private_records: BTreeMap<(String, String, String), Value>,
    private_seq: u64,
    creations: usize,
    routes: Vec<(String, String)>,
    lose_next_create: bool,
    stale_graph_anchors: Vec<Value>,
    groups: HashMap<String, u64>,
    receipts: HashMap<String, Value>,
    unavailable: bool,
    reject_writes: bool,
    lose_next_write: bool,
    posts: usize,
    queries: Vec<Value>,
    profile: Option<String>,
    info_overrides: BTreeMap<String, Value>,
    bad_diagnostics: bool,
}

#[derive(Default)]
pub struct ContentReadGate {
    pub entered: tokio::sync::Notify,
    pub resume: tokio::sync::Notify,
}

/// A deterministic in-process HTTP peer with independent lifetime from Service.
pub struct AnySeekFixture {
    pub base: String,
    dimension: Option<usize>,
    data: Arc<Mutex<Data>>,
}
fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
    })
}
fn fixtures() -> &'static Mutex<HashMap<PathBuf, Arc<AnySeekFixture>>> {
    static FIXTURES: OnceLock<Mutex<HashMap<PathBuf, Arc<AnySeekFixture>>>> = OnceLock::new();
    FIXTURES.get_or_init(Default::default)
}
fn engines() -> &'static Mutex<HashMap<usize, Arc<AnySeekFixture>>> {
    static ENGINES: OnceLock<Mutex<HashMap<usize, Arc<AnySeekFixture>>>> = OnceLock::new();
    ENGINES.get_or_init(Default::default)
}
impl AnySeekFixture {
    pub fn pause_next_content_read(&self) -> Arc<ContentReadGate> {
        let gate = Arc::new(ContentReadGate::default());
        self.data.lock().unwrap().content_read = Some(gate.clone());
        gate
    }
    pub fn mutate_payload(&self, id: Uuid, mutate: impl FnOnce(&mut Value)) {
        let mut data = self.data.lock().unwrap();
        let record = data
            .records
            .values_mut()
            .find(|record| record["document"]["chunk_id"] == id.to_string())
            .unwrap();
        mutate(&mut record["document"]["payload"]);
    }
    pub fn start(dimension: Option<usize>) -> Arc<Self> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let fixture = Arc::new(Self {
            base: format!("http://{}", listener.local_addr().unwrap()),
            dimension,
            data: Arc::new(Mutex::new(Data::default())),
        });
        let app = Router::new()
            .route("/api/v1/live", get(|| async { Json("fixture-1") }))
            .route("/api/v1/index", get(list).post(create))
            .route("/api/v1/index/{id}", get(info))
            .route("/api/v1/index/{id}/group", get(group))
            .route("/api/v1/index/{id}/group/replace", post(replace))
            .route("/api/v1/index/{id}/query", post(query))
            .layer(axum::middleware::from_fn(request_id))
            .with_state(fixture.clone());
        runtime().spawn(async move {
            axum::serve(tokio::net::TcpListener::from_std(listener).unwrap(), app)
                .await
                .unwrap();
        });
        fixture
    }
    pub fn bind_profile(&self, profile: Option<String>) {
        let mut data = self.data.lock().unwrap();
        if data.profile.is_none() {
            data.profile = profile;
        }
    }
    pub fn info_override(&self, key: &str, value: Value) {
        self.data
            .lock()
            .unwrap()
            .info_overrides
            .insert(key.into(), value);
    }
    pub fn bad_diagnostics(&self, value: bool) {
        self.data.lock().unwrap().bad_diagnostics = value;
    }
    pub fn config(&self) -> AnySeekConfig {
        AnySeekConfig::new(&self.base, "test-owned-key").unwrap()
    }
    pub fn creations(&self) -> usize {
        self.data.lock().unwrap().creations
    }
    pub fn routes(&self) -> Vec<(String, String)> {
        self.data.lock().unwrap().routes.clone()
    }
    pub fn lose_next_create(&self) {
        self.data.lock().unwrap().lose_next_create = true;
    }
    pub fn remove_collection(&self, scope: Scope) {
        self.data.lock().unwrap().collections.remove(&(
            scope.tenant_id.to_string(),
            format!("ws_{}", scope.workspace_id.simple()),
        ));
    }
    pub fn unavailable(&self, value: bool) {
        self.data.lock().unwrap().unavailable = value;
    }
    pub fn reject_writes(&self, value: bool) {
        self.data.lock().unwrap().reject_writes = value;
    }
    pub fn lose_next_write(&self) {
        self.data.lock().unwrap().lose_next_write = true;
    }
    /// Wire requests only; this does not emulate native query planning.
    pub fn queries(&self) -> Vec<Value> {
        self.data.lock().unwrap().queries.clone()
    }
    pub fn clear_queries(&self) {
        self.data.lock().unwrap().queries.clear();
    }
    /// Retain exact old projection rows for fail-closed authority regression.
    pub fn snapshot_records(&self) -> Vec<Value> {
        self.data
            .lock()
            .unwrap()
            .records
            .values()
            .cloned()
            .collect()
    }
    pub fn published_payloads(&self, scope: Scope, ids: &[Uuid]) -> HashMap<Uuid, ChunkPayload> {
        let data = self.data.lock().unwrap();
        ids.iter()
            .map(|id| {
                let document = data
                    .records
                    .values()
                    .map(|row| &row["document"])
                    .find(|doc| {
                        doc["chunk_id"] == id.to_string()
                            && doc["tenant"] == scope.tenant_id.to_string()
                            && doc["workspace"] == scope.workspace_id.to_string()
                    })
                    .unwrap();
                (
                    *id,
                    serde_json::from_value(document["payload"].clone()).unwrap(),
                )
            })
            .collect()
    }
    /// Inject stale graph-bearing anchors only into graph-filter queries. They
    /// must never become stale ordinary keyword/vector candidates in this test.
    pub fn restore_stale_records(&self, records: &[Value]) {
        self.data.lock().unwrap().stale_graph_anchors = records.to_vec();
    }
    pub fn posts(&self) -> usize {
        self.data.lock().unwrap().posts
    }
}

pub fn fixture(dir: impl AsRef<Path>, dimension: Option<usize>) -> Arc<AnySeekFixture> {
    let path = dir.as_ref().to_owned();
    fixtures()
        .lock()
        .unwrap()
        .entry(path)
        .or_insert_with(|| AnySeekFixture::start(dimension))
        .clone()
}

pub async fn open_service(
    dir: impl AsRef<Path>,
    models: Models,
) -> origence::error::Result<Service> {
    let remote = fixture(
        dir.as_ref(),
        models
            .profile
            .as_ref()
            .map(|_| models.embedding_dimension()),
    );
    remote.bind_profile(models.profile.clone());
    let service = Service::open_with_anyseek(dir, models, remote.config()).await?;
    engines()
        .lock()
        .unwrap()
        .insert(Arc::as_ptr(&service.engine) as usize, remote);
    Ok(service)
}

pub fn command_env(command: &mut tokio::process::Command, dir: &Path, dimension: Option<usize>) {
    let remote = fixture(dir, dimension);
    // Keep model fixtures on loopback even when the parent has dedicated endpoints.
    command
        .env_remove("OC_EMBEDDING_BASE_URL")
        .env_remove("OC_EMBEDDING_API_KEY")
        .env_remove("OC_EXTRACTION_BASE_URL")
        .env_remove("OC_EXTRACTION_API_KEY")
        .env("OC_FULL_TEXT_BACKEND", "anyseek")
        .env("OC_ANYSEEK_URL", &remote.base)
        .env("OC_ANYSEEK_API_KEY", "test-owned-key");
}

#[derive(Clone)]
struct QueryId(String);
#[derive(Clone)]
struct Namespace(String);
fn analysis_fingerprint() -> String {
    storage::hash("fixture-analysis-v1")
}
async fn request_id(
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if request
        .headers()
        .get("apikey")
        .and_then(|v| v.to_str().ok())
        != Some("test-owned-key")
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"code":"unauthorized"})),
        )
            .into_response();
    }
    if request.uri().path() != "/api/v1/live" {
        let tenant = request
            .headers()
            .get("x-anyseek-tenant")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if Uuid::parse_str(tenant).is_err() {
            return failure(StatusCode::BAD_REQUEST).into_response();
        }
        let tenant = tenant.to_owned();
        request.extensions_mut().insert(Namespace(tenant));
    }
    let id = Uuid::new_v4().simple().to_string();
    request.extensions_mut().insert(QueryId(id.clone()));
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert("x-anyseek-request-id", id.parse().unwrap());
    let code = match response.status() {
        StatusCode::NOT_FOUND => Some("not_found"),
        StatusCode::SERVICE_UNAVAILABLE => Some("recovery_required"),
        _ => None,
    };
    if let Some(code) = code {
        response
            .headers_mut()
            .insert("x-anyseek-error-code", code.parse().unwrap());
    }
    response
}

async fn list(
    State(f): State<Arc<AnySeekFixture>>,
    axum::extract::Extension(Namespace(tenant)): axum::extract::Extension<Namespace>,
) -> (StatusCode, Json<Value>) {
    let data = f.data.lock().unwrap();
    if data.unavailable {
        return failure(StatusCode::SERVICE_UNAVAILABLE);
    }
    (
        StatusCode::OK,
        Json(json!(
            data.collections
                .iter()
                .filter(|(t, _)| t == &tenant)
                .map(|(_, id)| json!({"id":id}))
                .collect::<Vec<_>>()
        )),
    )
}
async fn create(
    State(f): State<Arc<AnySeekFixture>>,
    axum::extract::Extension(Namespace(tenant)): axum::extract::Extension<Namespace>,
    Json(q): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let mut data = f.data.lock().unwrap();
    if data.unavailable {
        return failure(StatusCode::SERVICE_UNAVAILABLE);
    }
    let id = q["collection_id"].as_str().unwrap().to_owned();
    if !data.collections.insert((tenant.clone(), id.clone())) {
        return failure(StatusCode::CONFLICT);
    }
    if id.starts_with("pr_") {
        data.private_configs
            .insert((tenant, id.clone()), q["config"].clone());
    } else {
        data.creations += 1;
    }
    if data.lose_next_create {
        data.lose_next_create = false;
        return failure(StatusCode::SERVICE_UNAVAILABLE);
    }
    (StatusCode::OK, Json(json!({"collection_id": id})))
}
async fn info(
    State(f): State<Arc<AnySeekFixture>>,
    RoutePath(id): RoutePath<String>,
    axum::extract::Extension(Namespace(tenant)): axum::extract::Extension<Namespace>,
) -> (StatusCode, Json<Value>) {
    {
        let mut data = f.data.lock().unwrap();
        data.routes.push((tenant.clone(), id.clone()));
        if data.unavailable {
            return failure(StatusCode::NOT_FOUND);
        }
        if !data.collections.contains(&(tenant.clone(), id.clone())) {
            return failure(StatusCode::FORBIDDEN);
        }
    }
    let schema = match f.dimension {
        Some(d) => AnySeekFullTextStore::vector_collection_config(d).unwrap(),
        None => AnySeekFullTextStore::collection_config(),
    };
    let data = f.data.lock().unwrap();
    let schema = data.private_configs.get(&(tenant, id)).unwrap_or(&schema);
    let mut info = json!({"version":"fixture-1", "name":schema["index_name"],"schema":schema["schema"],"text_normalization":"None","vector_dimensions":f.dimension,
        "embedding_space_id":data.profile,"limits":{"max_records":10000,"max_candidates":10000},
        "analysis":{"format":"native-package-analysis:v1","fingerprint":analysis_fingerprint(),"tokenizer":"TantivySimpleV1",
            "tokenizer_identity":"tantivy-simple-v1;tantivy=0.27.0;lowercase;no-stopwords;no-length-filter",
            "stop_words":"None","stop_words_fingerprint":storage::hash(b"\"None\"")}});
    for (key, value) in &data.info_overrides {
        info[key] = value.clone();
    }
    (StatusCode::OK, Json(info))
}
fn failure(status: StatusCode) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(json!({"error":"private test backend diagnostic"})),
    )
}
async fn group(
    State(f): State<Arc<AnySeekFixture>>,
    RoutePath(collection): RoutePath<String>,
    axum::extract::Extension(Namespace(tenant)): axum::extract::Extension<Namespace>,
    Query(q): Query<HashMap<String, String>>,
) -> (StatusCode, Json<Value>) {
    let data = f.data.lock().unwrap();
    if data.unavailable {
        return failure(StatusCode::NOT_FOUND);
    }
    if !data
        .collections
        .contains(&(tenant.clone(), collection.clone()))
    {
        return failure(StatusCode::NOT_FOUND);
    }
    let id = q.get("group_id").unwrap();
    match data.groups.get(&format!("{tenant}/{collection}/{id}")) {
        Some(v) => (
            StatusCode::OK,
            Json(json!({"group_id":id,"group_version":v})),
        ),
        None => failure(StatusCode::NOT_FOUND),
    }
}
async fn replace(
    State(f): State<Arc<AnySeekFixture>>,
    RoutePath(collection): RoutePath<String>,
    axum::extract::Extension(Namespace(tenant)): axum::extract::Extension<Namespace>,
    Json(q): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let mut data = f.data.lock().unwrap();
    if data.unavailable {
        return failure(StatusCode::NOT_FOUND);
    }
    if !data
        .collections
        .contains(&(tenant.clone(), collection.clone()))
    {
        return failure(StatusCode::NOT_FOUND);
    }
    if data.reject_writes {
        return failure(StatusCode::SERVICE_UNAVAILABLE);
    }
    let private = collection.starts_with("pr_");
    if private {
        data.private_seq += 1;
    } else {
        data.posts += 1;
    }
    let operation = format!("{tenant}/{collection}/{}", q["operation_id"]);
    if let Some(receipt) = data.receipts.get(&operation) {
        return (StatusCode::OK, Json(receipt.clone()));
    }
    let group = q["group_id"].as_str().unwrap().to_string();
    let group_key = format!("{tenant}/{collection}/{group}");
    let current = data.groups.get(&group_key).copied().unwrap_or(0);
    if q["expected_group_version"].as_u64() != Some(current) {
        return failure(StatusCode::CONFLICT);
    }
    let rows = q["records"].as_array().unwrap();
    if rows.len() > 64 {
        return failure(StatusCode::BAD_REQUEST);
    }
    if private {
        data.private_records.retain(|(t, c, _), r| {
            t != &tenant || c != &collection || r["document"]["archive"] != group
        });
        for row in rows {
            let id = row["record_id"].as_str().unwrap().to_owned();
            let mut row = row.clone();
            row["version"] = json!(current + 1);
            data.private_records
                .insert((tenant.clone(), collection.clone(), id), row);
        }
    } else {
        data.records
            .retain(|_, r| r["document"]["publication"] != group);
        for row in rows {
            data.records
                .insert(row["record_id"].as_str().unwrap().into(), row.clone());
        }
    }
    data.groups.insert(group_key, current + 1);
    let seq = if private {
        data.private_seq
    } else {
        data.posts as u64
    };
    let receipt = json!({"atomic":true,"operation_id":q["operation_id"],"receipt":{"group_id":group,"group_version":current+1,"record_count":rows.len(),"seq":seq}});
    data.receipts.insert(operation, receipt.clone());
    if data.lose_next_write {
        data.lose_next_write = false;
        return failure(StatusCode::SERVICE_UNAVAILABLE);
    }
    (StatusCode::OK, Json(receipt))
}
fn matches(record: &Value, filters: &[Value]) -> bool {
    filters.iter().all(|filter| {
        if let Some(int) = filter.get("Int") {
            let field = int["field"].as_str().unwrap();
            return record["document"][field].as_i64().is_some_and(|value| {
                let range = &int["filter"];
                range["start"].as_i64().unwrap() <= value && value < range["end"].as_i64().unwrap()
            });
        }
        let is_set = filter.get("StringSet").is_some();
        let f = if is_set {
            &filter["StringSet"]
        } else {
            &filter["String"]
        };
        let field = f["field"].as_str().unwrap();
        f["filter"].as_array().unwrap().iter().any(|v| {
            if is_set {
                record["document"][field]
                    .as_array()
                    .is_some_and(|values| values.contains(v))
            } else {
                record["document"][field].is_string() && *v == record["document"][field]
            }
        })
    })
}
fn scores(record: &Value, q: &Value) -> Option<f64> {
    if let Some(v) = q.get("query_vector") {
        let a = v.as_array()?;
        let b = record["document"]["vector"].as_array()?;
        if a.len() != b.len() {
            return None;
        }
        let (mut dot, mut aa, mut bb) = (0., 0., 0.);
        for (a, b) in a.iter().zip(b) {
            let (a, b) = (a.as_f64()?, b.as_f64()?);
            dot += a * b;
            aa += a * a;
            bb += b * b;
        }
        return Some(if aa * bb > 0. {
            dot / (aa * bb).sqrt()
        } else {
            0.
        });
    }
    let field = q["search_fields"][0].as_str().unwrap_or("terms");
    let terms = record["document"][field]
        .as_str()
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>();
    let wanted = q["query"]
        .as_str()
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>();
    let matched = if q["operator"] == "or" {
        wanted.is_empty() || wanted.iter().any(|word| terms.contains(word))
    } else {
        wanted.iter().all(|word| terms.contains(word))
    };
    if !matched {
        return None;
    }
    // Deliberately simple deterministic fixture score, not native BM25 evidence.
    Some(
        wanted
            .iter()
            .map(|w| terms.iter().filter(|t| *t == w).count() as f64)
            .sum::<f64>()
            / (terms.len().max(1) as f64).sqrt(),
    )
}
async fn query(
    State(f): State<Arc<AnySeekFixture>>,
    RoutePath(collection): RoutePath<String>,
    axum::extract::Extension(Namespace(tenant)): axum::extract::Extension<Namespace>,
    axum::extract::Extension(QueryId(request_id)): axum::extract::Extension<QueryId>,
    Json(q): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let gate = {
        let mut data = f.data.lock().unwrap();
        if q["query"] == ""
            && q["fields"] == json!([])
            && q["filter"].as_array().is_some_and(|filters| {
                filters
                    .iter()
                    .any(|filter| filter["String"]["field"] == "chunk_id")
            })
        {
            data.content_read.take()
        } else {
            None
        }
    };
    if let Some(gate) = gate {
        gate.entered.notify_one();
        gate.resume.notified().await;
    }
    let mut data = f.data.lock().unwrap();
    let private = collection.starts_with("pr_");
    if !private {
        data.queries.push(q.clone());
    }
    if data.unavailable {
        return failure(StatusCode::NOT_FOUND);
    }
    let filters = q["filter"].as_array().cloned().unwrap_or_default();
    let graph_query = filters.iter().any(|filter| {
        matches!(
            filter["StringSet"]["field"].as_str(),
            Some("graph_keys" | "endpoint_ids")
        )
    });
    if !data
        .collections
        .contains(&(tenant.clone(), collection.clone()))
    {
        return failure(StatusCode::NOT_FOUND);
    }
    let mut candidates: BTreeMap<_, _> = data
        .records
        .iter()
        .filter(|(_, row)| {
            row["document"]["tenant"] == tenant
                && row["document"]["workspace"]
                    .as_str()
                    .is_some_and(|ws| collection == format!("ws_{}", ws.replace('-', "")))
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if private {
        candidates.extend(
            data.private_records
                .iter()
                .filter(|((t, c, _), _)| t == &tenant && c == &collection)
                .map(|((_, _, id), value)| (id.clone(), value.clone())),
        );
    }
    if graph_query {
        for anchor in &data.stale_graph_anchors {
            candidates.insert(
                anchor["record_id"].as_str().unwrap().to_owned(),
                anchor.clone(),
            );
        }
    }
    let mut rows = candidates
        .values()
        .filter(|r| matches(r, &filters))
        .filter_map(|r| scores(r, &q).map(|score| (r.clone(), score)))
        .collect::<Vec<_>>();
    if q["sort"] == json!([{"field":"sequence","order":"Ascending"}]) {
        rows.sort_by_key(|(record, _)| record["document"]["sequence"].as_i64().unwrap());
    } else {
        rows.sort_by(|a, b| {
            b.1.total_cmp(&a.1)
                .then_with(|| a.0["record_id"].as_str().cmp(&b.0["record_id"].as_str()))
        });
    }
    let count = rows.len();
    let offset = q["offset"].as_u64().unwrap_or(0) as usize;
    let length = q["length"].as_u64().unwrap_or(10) as usize;
    let hits = rows
        .into_iter()
        .skip(offset)
        .take(length)
        .map(|(mut record, score)| {
            if let Some(fields) = q["fields"].as_array().filter(|v| !v.is_empty()) {
                record["document"]
                    .as_object_mut()
                    .unwrap()
                    .retain(|k, _| fields.iter().any(|f| f.as_str() == Some(k)));
            }
            json!({"record":record,"score":score})
        })
        .collect::<Vec<_>>();
    (
        StatusCode::OK,
        Json(
            json!({"hits":hits,"count_total":count,"visible_seq":if private {data.private_seq} else {data.posts as u64},
            "diagnostics":{"format_version":2,"request_id":if data.bad_diagnostics {"PRIVATE BAD REQUEST ID".into()}else{request_id},"engine_version":"fixture-1",
                "mode":if q.get("query_vector").is_some(){"vector"}else if q["query"].as_str().is_some_and(|s|!s.is_empty()){"lexical"}else{"browse"},
                "elapsed_us":1,"analysis_fingerprint":analysis_fingerprint(),
                "embedding_space_sha256":data.info_overrides.get("embedding_space_id").map_or_else(||data.profile.as_deref().map(storage::hash), |v|v.as_str().map(storage::hash)),
                "index_progress":{"committed_seq":data.posts,"lexical_seq":data.posts,"vector_seq":data.posts,
                    "index_failed":false,"recovery_required":false,"sampling":"after_query_not_atomic_with_result"}}}),
        ),
    )
}

/// Explicit downstream seeding/corruption access for authority-boundary tests.
/// This does not stand in for testing adapter publication or AnySeek ranking.
pub struct Projection(Arc<AnySeekFixture>);
pub fn projection(service: &Service) -> Projection {
    Projection(
        engines()
            .lock()
            .unwrap()
            .get(&(Arc::as_ptr(&service.engine) as usize))
            .expect("open with support::open_service")
            .clone(),
    )
}
fn row(scope: Scope, id: Uuid, source: SourceVersion, generation: i64) -> Value {
    let record_id = format!(
        "c{}",
        storage::hash(format!(
            "{}:{}:{}:{}:{}:{}",
            scope.tenant_id, scope.workspace_id, id, source.source_id, source.version, generation
        ))
    );
    json!({"record_id":record_id,"document":{"format":"origence-content-sha256-v1","kind":"chunk","tenant":scope.tenant_id,"workspace":scope.workspace_id,"chunk_id":id,"source_id":source.source_id,"source_version":source.version.to_string(),"generation":generation.to_string(),"publication":format!("p{}",storage::hash(record_id)),"terms":""}})
}
impl Projection {
    pub async fn upsert_full_text(
        &self,
        scope: Scope,
        entries: Vec<FullTextEntry>,
    ) -> StorageResult<()> {
        let mut data = self.0.data.lock().unwrap();
        for e in entries {
            let r = row(scope, e.id, e.source, e.generation);
            let id = r["record_id"].as_str().unwrap().to_string();
            let existing = data.records.entry(id).or_insert(r);
            if let Some(summary) = &e.payload.summary {
                existing["document"]["summary_terms"] = json!(
                    origence::parsing::lexical(&summary.text)
                        .split_whitespace()
                        .map(|s| format!("t{}", storage::hash(s)))
                        .collect::<Vec<_>>()
                        .join(" ")
                );
            }
            existing["document"]["payload"] = json!(e.payload);
            existing["document"]["terms"] = json!(
                e.search_terms
                    .split_whitespace()
                    .map(|s| format!("t{}", storage::hash(s)))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        let groups = data
            .records
            .values()
            .map(|r| r["document"]["publication"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        for group in groups {
            data.groups
                .entry(format!(
                    "{}/ws_{}/{group}",
                    scope.tenant_id,
                    scope.workspace_id.simple()
                ))
                .or_insert(1);
        }
        Ok(())
    }
    pub async fn upsert(&self, scope: Scope, entries: Vec<VectorEntry>) -> StorageResult<()> {
        let mut data = self.0.data.lock().unwrap();
        for e in entries {
            let r = row(scope, e.id, e.source, e.generation);
            let id = r["record_id"].as_str().unwrap().to_string();
            let existing = data.records.entry(id).or_insert(r);
            existing["document"]["vector"] = json!(e.embedding);
            existing["document"]["profile"] = json!(e.profile);
            existing["document"]["dimension"] = json!(e.dimension.to_string());
        }
        let groups = data
            .records
            .values()
            .map(|r| r["document"]["publication"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        for group in groups {
            data.groups
                .entry(format!(
                    "{}/ws_{}/{group}",
                    scope.tenant_id,
                    scope.workspace_id.simple()
                ))
                .or_insert(1);
        }
        Ok(())
    }
    fn rows(
        &self,
        scope: Scope,
        ids: Option<Vec<Uuid>>,
        generation: i64,
    ) -> StorageResult<Vec<Value>> {
        let data = self.0.data.lock().unwrap();
        if data.unavailable {
            return Err(StorageError::Unavailable("test backend unavailable".into()));
        }
        let generation = generation.to_string();
        Ok(data
            .records
            .values()
            .filter(|r| {
                r["document"]["tenant"] == scope.tenant_id.to_string()
                    && r["document"]["workspace"] == scope.workspace_id.to_string()
                    && r["document"]["generation"] == generation
                    && ids.as_ref().is_none_or(|ids| {
                        ids.iter()
                            .any(|id| r["document"]["chunk_id"] == id.to_string())
                    })
            })
            .cloned()
            .collect())
    }
    pub async fn search_full_text(&self, q: FullTextQuery) -> StorageResult<Vec<FullTextHit>> {
        let request = json!({"query":q.search_terms.split_whitespace().map(|s|format!("t{}",storage::hash(s))).collect::<Vec<_>>().join(" ")});
        let mut hits = self
            .rows(q.scope, q.artifact_ids, q.generation)?
            .into_iter()
            .filter_map(|r| {
                scores(&r, &request).map(|s| FullTextHit {
                    id: serde_json::from_value(r["document"]["chunk_id"].clone()).unwrap(),
                    source: source(&r),
                    score: s as f32,
                })
            })
            .collect::<Vec<_>>();
        hits.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
        hits.truncate(q.limit);
        Ok(hits)
    }
    pub async fn search(&self, q: VectorQuery) -> StorageResult<Vec<VectorHit>> {
        let request = json!({"query_vector":q.embedding});
        let dimension = q.dimension.to_string();
        let mut hits = self
            .rows(q.scope, q.artifact_ids, q.generation)?
            .into_iter()
            .filter(|r| {
                r["document"]["profile"] == q.profile && r["document"]["dimension"] == dimension
            })
            .filter_map(|r| {
                scores(&r, &request).map(|s| VectorHit {
                    id: serde_json::from_value(r["document"]["chunk_id"].clone()).unwrap(),
                    source: source(&r),
                    score: s as f32,
                })
            })
            .collect::<Vec<_>>();
        hits.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
        hits.truncate(q.limit);
        Ok(hits)
    }
    pub async fn delete_objects(&self, scope: Scope, ids: Vec<Uuid>) -> StorageResult<()> {
        self.0.data.lock().unwrap().records.retain(|_, r| {
            !(r["document"]["tenant"] == scope.tenant_id.to_string()
                && r["document"]["workspace"] == scope.workspace_id.to_string()
                && ids
                    .iter()
                    .any(|id| r["document"]["chunk_id"] == id.to_string()))
        });
        Ok(())
    }
    pub async fn check_full_text_available(&self) -> StorageResult<()> {
        let (scopes, profile) = {
            let data = self.0.data.lock().unwrap();
            if data.unavailable {
                return Err(StorageError::Unavailable("test backend unavailable".into()));
            }
            (
                data.collections.iter().cloned().collect::<Vec<_>>(),
                data.profile.clone(),
            )
        };
        let mut config = self.0.config();
        if let (Some(d), Some(p)) = (self.0.dimension, profile) {
            config = config.with_embedding(p, d)?;
        }
        let backend = AnySeekFullTextStore::new(config)?;
        backend.check().await?;
        for (tenant, collection) in scopes {
            let scope = Scope {
                tenant_id: Uuid::parse_str(&tenant).unwrap(),
                workspace_id: Uuid::parse_str(collection.strip_prefix("ws_").unwrap()).unwrap(),
            };
            backend.check_scope(scope, false).await?;
        }
        Ok(())
    }
    pub async fn check_vector_available(
        &self,
        profile: &str,
        dimension: usize,
    ) -> StorageResult<()> {
        AnySeekFullTextStore::new(
            self.0
                .config()
                .with_embedding(profile.into(), self.0.dimension.unwrap_or(0))?,
        )?
        .check_vector(profile, dimension)
        .await
    }
}
fn source(r: &Value) -> SourceVersion {
    SourceVersion {
        source_id: serde_json::from_value(r["document"]["source_id"].clone()).unwrap(),
        version: r["document"]["source_version"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap(),
    }
}

mod fixture_workspace;

pub async fn seed_workspace(
    dir: &Path,
    name: &str,
    parent: Option<uuid::Uuid>,
) -> serde_json::Value {
    fixture_workspace::seed_member_workspace(dir, name, parent)
        .await
        .unwrap()
}
