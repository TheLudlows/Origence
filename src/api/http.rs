use crate::{
    error::{AppError, Result},
    service::Service,
    types::*,
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

pub fn router(service: Service) -> Router {
    Router::new()
        .route(
            "/health/live",
            get(|| async { Json(json!({"status":"ok"})) }),
        )
        .route("/health/ready", get(ready))
        .route("/v1/whoami", get(whoami))
        .route("/v1/workspaces", get(workspaces))
        .route("/v1/workspaces/owned", get(list_owned_workspaces))
        .route("/v1/workspaces/owned/{id}", put(create_owned_workspace))
        .route("/v1/session", get(private_session))
        .route("/admin/keys/{id}/rotate", post(rotate_key))
        .route("/admin/keys", get(list_keys).post(issue_key))
        .route("/admin/keys/{id}", axum::routing::delete(revoke_key))
        .route(
            "/admin/workspace-key-grants",
            get(list_workspace_key_grants),
        )
        .route(
            "/admin/workspace-key-grants/{logical_key_id}",
            axum::routing::put(set_workspace_key_grant).delete(revoke_workspace_key_grant),
        )
        .route("/admin/memory-properties", post(register_memory_property))
        .route("/v1/memory-properties/{predicate}", get(memory_property))
        .route("/v1/memories", post(memory))
        .route("/v1/memories/identified", post(identified_memory))
        .route("/v1/memories/semantic", post(semantic_memory))
        .route("/v1/memories/lookup", post(lookup_memory))
        .route("/v1/captures/semantic", post(semantic_capture))
        .route("/v1/captures", post(capture))
        .route("/v1/captures/identified", post(identified_capture))
        .route("/v1/knowledge", post(knowledge))
        .route("/v1/sessions/{session_id}/turns", post(archive_turn))
        .route(
            "/v1/sessions/{session_id}/turns/{turn_id}",
            get(session_turn),
        )
        .route(
            "/v1/sessions/{session_id}/turns/{turn_id}/feedback",
            post(record_feedback).get(session_feedback),
        )
        .route("/v1/sessions/{session_id}/improve", post(improve))
        .route("/v1/improvements/{id}", get(improvement))
        .route("/v1/learnings/{id}", get(learning))
        .route("/v1/search", post(search))
        .route("/v1/context", post(context_bundle))
        .route("/v1/resolve", post(resolve))
        .route("/v1/assets", get(assets))
        .route("/v1/assets/{id}/runs", get(asset_runs))
        .route("/v1/assets/{id}", get(asset).delete(delete_asset))
        .route("/v1/assets/{id}/restore", post(restore))
        .route("/v1/events/{id}", get(source_event).delete(delete_event))
        .route("/v1/jobs/{id}", get(job))
        .route("/v1/jobs/{id}/{action}", post(job_action))
        .route("/v1/files", post(upload))
        .route("/v1/files/{id}", get(file).delete(delete_file))
        .merge(super::foundation::routes())
        .layer(DefaultBodyLimit::max(crate::parsing::MAX_FILE))
        .with_state(service)
}
fn token(h: &HeaderMap) -> Result<&str> {
    h.get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(AppError::Unauthorized)
}
fn key(h: &HeaderMap) -> Result<&str> {
    h.get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Invalid("Idempotency-Key is required for mutations".into()))
}
pub(super) async fn auth(s: &Service, h: &HeaderMap) -> Result<AuthContext> {
    let token = token(h)?;
    let mut selections = h.get_all("x-origence-workspace").iter();
    let workspace = selections
        .next()
        .map(|value| {
            value
                .to_str()
                .ok()
                .and_then(|id| Uuid::parse_str(id).ok())
                .filter(|id| !id.is_nil())
                .ok_or_else(|| {
                    AppError::Invalid("X-Origence-Workspace must be a workspace UUID".into())
                })
        })
        .transpose()?;
    if selections.next().is_some() {
        return Err(AppError::Invalid(
            "X-Origence-Workspace must occur once".into(),
        ));
    }
    let explicit = s.auth_workspace(token, workspace).await;
    let (Some(target), Err(AppError::Forbidden)) = (workspace, &explicit) else {
        return explicit;
    };
    // The console can additionally use a live root key with owned-workspace
    // capability. The lookup checks that the target is its direct child.
    let root = match s.auth(token).await {
        Ok(root) => root,
        Err(AppError::Invalid(_)) => return explicit,
        Err(error) => return Err(error),
    };
    match s.lookup_owned_workspace(&root, target).await {
        Ok(Some(scoped)) => Ok(scoped),
        Ok(None) | Err(AppError::Conflict(_)) => Err(AppError::NotFound),
        Err(error) => Err(error),
    }
}
async fn ready(State(s): State<Service>) -> impl IntoResponse {
    if s.ready().await {
        (StatusCode::OK, Json(json!({"status":"ready"})))
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status":"unavailable"})),
        )
    }
}
async fn register_memory_property(
    State(s): State<Service>,
    h: HeaderMap,
    Json(i): Json<crate::memory_semantics::MemoryProperty>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.register_memory_property(&auth(&s, &h).await?, key(&h)?, i)
            .await?,
    ))
}
async fn memory_property(
    State(s): State<Service>,
    h: HeaderMap,
    Path(predicate): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.memory_property(&auth(&s, &h).await?, &predicate).await?,
    ))
}
async fn memory(
    State(s): State<Service>,
    h: HeaderMap,
    Json(i): Json<MemoryInput>,
) -> Result<Json<Value>> {
    Ok(Json(s.memory(&auth(&s, &h).await?, key(&h)?, i).await?))
}
async fn semantic_memory(
    State(s): State<Service>,
    h: HeaderMap,
    Json(i): Json<crate::memory_semantics::SemanticMemoryInput>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.semantic_memory(&auth(&s, &h).await?, key(&h)?, i).await?,
    ))
}
async fn identified_memory(
    State(s): State<Service>,
    h: HeaderMap,
    Json(i): Json<IdentifiedMemoryInput>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.identified_memory(&auth(&s, &h).await?, key(&h)?, i)
            .await?,
    ))
}
async fn lookup_memory(
    State(s): State<Service>,
    h: HeaderMap,
    Json(identity): Json<crate::memory_identity::MemoryIdentity>,
) -> Result<Json<Value>> {
    Ok(Json(s.lookup_memory(&auth(&s, &h).await?, identity).await?))
}
async fn semantic_capture(
    State(s): State<Service>,
    h: HeaderMap,
    Json(i): Json<crate::semantic_capture::SemanticCaptureInput>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.semantic_capture(&auth(&s, &h).await?, key(&h)?, i)
            .await?,
    ))
}
async fn identified_capture(
    State(s): State<Service>,
    h: HeaderMap,
    Json(i): Json<IdentifiedMemoryInput>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.identified_capture(&auth(&s, &h).await?, key(&h)?, i)
            .await?,
    ))
}
async fn capture(
    State(s): State<Service>,
    h: HeaderMap,
    Json(i): Json<CaptureInput>,
) -> Result<Json<Value>> {
    Ok(Json(s.capture(&auth(&s, &h).await?, key(&h)?, i).await?))
}
async fn knowledge(
    State(s): State<Service>,
    h: HeaderMap,
    Json(i): Json<KnowledgeInput>,
) -> Result<Json<Value>> {
    Ok(Json(s.knowledge(&auth(&s, &h).await?, key(&h)?, i).await?))
}
async fn search(
    State(s): State<Service>,
    h: HeaderMap,
    Json(i): Json<SearchInput>,
) -> Result<Json<Value>> {
    Ok(Json(s.search(&auth(&s, &h).await?, i).await?))
}
async fn resolve(
    State(s): State<Service>,
    h: HeaderMap,
    Json(i): Json<ResolveInput>,
) -> Result<Json<Value>> {
    Ok(Json(s.resolve(&auth(&s, &h).await?, i).await?))
}
#[derive(Deserialize)]
struct Version {
    version: Option<i32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetPage {
    kind: String,
    memory_filter: Option<String>,
    after: Option<Uuid>,
    limit: Option<usize>,
}

async fn assets(
    State(s): State<Service>,
    h: HeaderMap,
    Query(page): Query<AssetPage>,
) -> Result<Json<Value>> {
    if let Some(filter) = page.memory_filter {
        if page.kind != "memory" || filter.len() > 16384 {
            return Err(AppError::Invalid("invalid memory filter".into()));
        }
        let filter = serde_json::from_str::<crate::memory_identity::MemoryCatalogFilter>(&filter)
            .map_err(|_| AppError::Invalid("invalid memory filter".into()))?;
        return Ok(Json(
            s.list_memories(
                &auth(&s, &h).await?,
                &filter,
                page.after.unwrap_or_default(),
                page.limit.unwrap_or(50),
            )
            .await?,
        ));
    }
    Ok(Json(
        s.list_assets(
            &auth(&s, &h).await?,
            &page.kind,
            page.after.unwrap_or_default(),
            page.limit.unwrap_or(50),
        )
        .await?,
    ))
}
async fn asset(
    State(s): State<Service>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Query(v): Query<Version>,
) -> Result<Json<Value>> {
    Ok(Json(s.get(&auth(&s, &h).await?, id, v.version).await?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetRunPage {
    version: i32,
    after: Option<Uuid>,
    limit: Option<usize>,
}
async fn asset_runs(
    State(s): State<Service>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Query(page): Query<AssetRunPage>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.asset_runs(
            &auth(&s, &h).await?,
            id,
            page.version,
            page.after.unwrap_or_default(),
            page.limit.unwrap_or(25),
        )
        .await?,
    ))
}
async fn restore(
    State(s): State<Service>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(i): Json<RestoreInput>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.restore(&auth(&s, &h).await?, key(&h)?, id, i).await?,
    ))
}
async fn delete_asset(
    State(s): State<Service>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.delete(&auth(&s, &h).await?, key(&h)?, id, "asset")
            .await?,
    ))
}
async fn source_event(
    State(s): State<Service>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(s.source_event(&auth(&s, &h).await?, id).await?))
}
async fn delete_event(
    State(s): State<Service>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.delete(&auth(&s, &h).await?, key(&h)?, id, "event")
            .await?,
    ))
}
async fn delete_file(
    State(s): State<Service>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.delete(&auth(&s, &h).await?, key(&h)?, id, "file").await?,
    ))
}
async fn job(State(s): State<Service>, h: HeaderMap, Path(id): Path<Uuid>) -> Result<Json<Value>> {
    Ok(Json(s.job(&auth(&s, &h).await?, id).await?))
}
async fn job_action(
    State(s): State<Service>,
    h: HeaderMap,
    Path((id, action)): Path<(Uuid, String)>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.job_action(&auth(&s, &h).await?, key(&h)?, id, &action)
            .await?,
    ))
}
#[derive(Deserialize)]
struct Upload {
    name: String,
    format: String,
}
async fn upload(
    State(s): State<Service>,
    h: HeaderMap,
    Query(q): Query<Upload>,
    body: Bytes,
) -> Result<Json<Value>> {
    Ok(Json(
        s.upload(&auth(&s, &h).await?, key(&h)?, &q.name, &q.format, &body)
            .await?,
    ))
}
async fn file(
    State(s): State<Service>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse> {
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "application/octet-stream"),
            (axum::http::header::CONTENT_DISPOSITION, "attachment"),
            (axum::http::header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        s.file(&auth(&s, &h).await?, id).await?,
    ))
}

async fn whoami(State(s): State<Service>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(json!(auth(&s, &h).await?)))
}

async fn workspaces(State(s): State<Service>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(
        json!({"workspaces": s.workspace_access(token(&h)?).await?}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspacePage {
    after: Option<Uuid>,
    limit: Option<u32>,
}

async fn list_owned_workspaces(
    State(s): State<Service>,
    h: HeaderMap,
    Query(page): Query<WorkspacePage>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.list_managed_workspaces(
            &auth(&s, &h).await?,
            page.after.unwrap_or_default(),
            page.limit.unwrap_or(50),
        )
        .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnedWorkspaceInput {
    name: String,
}

async fn create_owned_workspace(
    State(s): State<Service>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<OwnedWorkspaceInput>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.create_managed_workspace(&auth(&s, &h).await?, id, &input.name)
            .await?,
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceGrantInput {
    role: String,
}

async fn set_workspace_key_grant(
    State(s): State<Service>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<WorkspaceGrantInput>,
) -> Result<Json<Value>> {
    let a = auth(&s, &h).await?;
    s.set_workspace_key_grant(&a, id, Some(&input.role)).await?;
    Ok(Json(
        json!({"workspace_id":a.workspace_id,"logical_key_id":id,"role":input.role}),
    ))
}

async fn revoke_workspace_key_grant(
    State(s): State<Service>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let a = auth(&s, &h).await?;
    s.set_workspace_key_grant(&a, id, None).await?;
    Ok(Json(
        json!({"workspace_id":a.workspace_id,"logical_key_id":id,"revoked":true}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyInput {
    workspace_id: Uuid,
    role: String,
    #[serde(default)]
    allow_owned_workspaces: bool,
}

async fn list_keys(
    State(s): State<Service>,
    h: HeaderMap,
    Query(page): Query<WorkspacePage>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.list_issued_keys(
            &auth(&s, &h).await?,
            page.after.unwrap_or_default(),
            page.limit.unwrap_or(50),
        )
        .await?,
    ))
}

async fn list_workspace_key_grants(
    State(s): State<Service>,
    h: HeaderMap,
    Query(page): Query<WorkspacePage>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.list_workspace_key_grants(
            &auth(&s, &h).await?,
            page.after.unwrap_or_default(),
            page.limit.unwrap_or(50),
        )
        .await?,
    ))
}

async fn issue_key(
    State(s): State<Service>,
    h: HeaderMap,
    Json(input): Json<KeyInput>,
) -> Result<impl IntoResponse> {
    let a = auth(&s, &h).await?;
    if input.workspace_id != a.workspace_id {
        return Err(AppError::Forbidden);
    }
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(
            s.issue_key_with_capabilities(
                &a,
                &input.role,
                crate::storage::KeyCapabilities {
                    allow_owned_workspaces: input.allow_owned_workspaces,
                },
            )
            .await?,
        ),
    ))
}
async fn revoke_key(
    State(s): State<Service>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(s.revoke_key(&auth(&s, &h).await?, id).await?))
}

async fn archive_turn(
    State(s): State<Service>,
    h: HeaderMap,
    Path(session): Path<Uuid>,
    Json(input): Json<crate::session::SessionTurnInput>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.archive_turn(&auth(&s, &h).await?, key(&h)?, session, input)
            .await?,
    ))
}
async fn session_turn(
    State(s): State<Service>,
    h: HeaderMap,
    Path((session, turn)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.session_turn(&auth(&s, &h).await?, session, turn).await?,
    ))
}
async fn record_feedback(
    State(s): State<Service>,
    h: HeaderMap,
    Path((session, turn)): Path<(Uuid, Uuid)>,
    Json(input): Json<crate::session::FeedbackInput>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.record_feedback(&auth(&s, &h).await?, key(&h)?, session, turn, input)
            .await?,
    ))
}
async fn session_feedback(
    State(s): State<Service>,
    h: HeaderMap,
    Path((session, turn)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.session_feedback(&auth(&s, &h).await?, session, turn)
            .await?,
    ))
}

async fn improve(
    State(s): State<Service>,
    h: HeaderMap,
    Path(session): Path<Uuid>,
    Json(input): Json<crate::learning::ImproveInput>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.improve(&auth(&s, &h).await?, key(&h)?, session, input)
            .await?,
    ))
}
async fn improvement(
    State(s): State<Service>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(s.improvement(&auth(&s, &h).await?, id).await?))
}
async fn learning(
    State(s): State<Service>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(s.learning(&auth(&s, &h).await?, id).await?))
}

async fn context_bundle(
    State(s): State<Service>,
    h: HeaderMap,
    Json(input): Json<crate::evidence::EvidenceInput>,
) -> Result<Json<crate::evidence::EvidenceBundle>> {
    Ok(Json(s.evidence_bundle(&auth(&s, &h).await?, input).await?))
}

async fn private_session(State(s): State<Service>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(s.private_session(&auth(&s, &h).await?).await?))
}
async fn rotate_key(
    State(s): State<Service>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse> {
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(s.rotate_key(&auth(&s, &h).await?, id).await?),
    ))
}
