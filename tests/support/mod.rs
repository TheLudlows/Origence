//! Loopback protocol fixture, not an AnySeek engine or ranking-acceptance test.
//! Real Turso, public Service/worker and HTTP adapter code remain under test.
#![allow(dead_code)] // Shared by two integration executables with different fixture needs.
use axum::{
    Json, Router,
    extract::{Path as RoutePath, Query, State},
    http::StatusCode,
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
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};
use uuid::Uuid;

#[derive(Default)]
struct Data {
    records: BTreeMap<String, Value>,
    groups: HashMap<String, u64>,
    receipts: HashMap<String, Value>,
    unavailable: bool,
    reject_writes: bool,
    lose_next_write: bool,
    posts: usize,
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
    pub fn start(dimension: Option<usize>) -> Arc<Self> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let fixture = Arc::new(Self {
            base: format!("http://{}", listener.local_addr().unwrap()),
            dimension,
            data: Arc::new(Mutex::new(Data::default())),
        });
        let app = Router::new()
            .route("/api/v1/index/{id}", get(info))
            .route("/api/v1/index/{id}/group", get(group))
            .route("/api/v1/index/{id}/group/replace", post(replace))
            .route("/api/v1/index/{id}/query", post(query))
            .with_state(fixture.clone());
        runtime().spawn(async move {
            axum::serve(tokio::net::TcpListener::from_std(listener).unwrap(), app)
                .await
                .unwrap();
        });
        fixture
    }
    pub fn config(&self) -> AnySeekConfig {
        AnySeekConfig::new(&self.base, "test-owned-key", "origence-test").unwrap()
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
    let service = Service::open_with_anyseek(dir, models, remote.config()).await?;
    engines()
        .lock()
        .unwrap()
        .insert(Arc::as_ptr(&service.engine) as usize, remote);
    Ok(service)
}

pub fn command_env(command: &mut tokio::process::Command, dir: &Path, dimension: Option<usize>) {
    let remote = fixture(dir, dimension);
    command
        .env("OC_FULL_TEXT_BACKEND", "anyseek")
        .env("OC_ANYSEEK_URL", &remote.base)
        .env("OC_ANYSEEK_API_KEY", "test-owned-key")
        .env("OC_ANYSEEK_COLLECTION", "origence-test");
}

async fn info(State(f): State<Arc<AnySeekFixture>>) -> (StatusCode, Json<Value>) {
    if f.data.lock().unwrap().unavailable {
        return failure(StatusCode::NOT_FOUND);
    }
    let schema = match f.dimension {
        Some(d) => AnySeekFullTextStore::vector_collection_config(d).unwrap(),
        None => AnySeekFullTextStore::collection_config(),
    };
    (
        StatusCode::OK,
        Json(
            json!({"name":schema["index_name"],"schema":schema["schema"],"text_normalization":"None","vector_dimensions":f.dimension}),
        ),
    )
}
fn failure(status: StatusCode) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(json!({"error":"private test backend diagnostic"})),
    )
}
async fn group(
    State(f): State<Arc<AnySeekFixture>>,
    Query(q): Query<HashMap<String, String>>,
) -> (StatusCode, Json<Value>) {
    let data = f.data.lock().unwrap();
    if data.unavailable {
        return failure(StatusCode::NOT_FOUND);
    }
    let id = q.get("group_id").unwrap();
    match data.groups.get(id) {
        Some(v) => (
            StatusCode::OK,
            Json(json!({"group_id":id,"group_version":v})),
        ),
        None => failure(StatusCode::NOT_FOUND),
    }
}
async fn replace(
    State(f): State<Arc<AnySeekFixture>>,
    Json(q): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let mut data = f.data.lock().unwrap();
    if data.unavailable {
        return failure(StatusCode::NOT_FOUND);
    }
    if data.reject_writes {
        return failure(StatusCode::SERVICE_UNAVAILABLE);
    }
    data.posts += 1;
    let operation = q["operation_id"].to_string();
    if let Some(receipt) = data.receipts.get(&operation) {
        return (StatusCode::OK, Json(receipt.clone()));
    }
    let group = q["group_id"].as_str().unwrap().to_string();
    let current = data.groups.get(&group).copied().unwrap_or(0);
    if q["expected_group_version"].as_u64() != Some(current) {
        return failure(StatusCode::CONFLICT);
    }
    let rows = q["records"].as_array().unwrap();
    if rows.len() > 64 {
        return failure(StatusCode::BAD_REQUEST);
    }
    data.records
        .retain(|_, r| r["document"]["publication"] != group);
    for row in rows {
        data.records
            .insert(row["record_id"].as_str().unwrap().into(), row.clone());
    }
    data.groups.insert(group.clone(), current + 1);
    let receipt = json!({"atomic":true,"operation_id":q["operation_id"],"receipt":{"group_id":group,"group_version":current+1,"record_count":rows.len()}});
    data.receipts.insert(operation, receipt.clone());
    if data.lose_next_write {
        data.lose_next_write = false;
        return failure(StatusCode::SERVICE_UNAVAILABLE);
    }
    (StatusCode::OK, Json(receipt))
}
fn matches(record: &Value, filters: &[Value]) -> bool {
    filters.iter().all(|filter| {
        let f = &filter["String"];
        let field = f["field"].as_str().unwrap();
        f["filter"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| *v == record["document"][field])
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
    let terms = record["document"]["terms"]
        .as_str()
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>();
    let wanted = q["query"]
        .as_str()
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>();
    if !wanted.iter().all(|word| terms.contains(word)) {
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
    RoutePath(_): RoutePath<String>,
    Json(q): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let data = f.data.lock().unwrap();
    if data.unavailable {
        return failure(StatusCode::NOT_FOUND);
    }
    let filters = q["filter"].as_array().cloned().unwrap_or_default();
    let mut rows = data
        .records
        .values()
        .filter(|r| matches(r, &filters))
        .filter_map(|r| scores(r, &q).map(|score| (r.clone(), score)))
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        b.1.total_cmp(&a.1)
            .then_with(|| a.0["record_id"].as_str().cmp(&b.0["record_id"].as_str()))
    });
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
        Json(json!({"hits":hits,"count_total":count})),
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
    json!({"record_id":record_id,"document":{"format":"origence-search-sha256-v2","tenant":scope.tenant_id,"workspace":scope.workspace_id,"chunk_id":id,"source_id":source.source_id,"source_version":source.version.to_string(),"generation":generation.to_string(),"publication":format!("p{}",storage::hash(record_id)),"terms":""}})
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
            data.groups.entry(group).or_insert(1);
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
            data.groups.entry(group).or_insert(1);
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
        AnySeekFullTextStore::new(self.0.config())?.check().await
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
