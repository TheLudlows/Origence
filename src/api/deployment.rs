//! HTTP endpoints for deployment administrators.
use super::foundation::{PublishInput, ReviewDecision};
use super::http::{CursorPage, Upload, key, token, uncached_json};
use crate::{
    error::Result,
    memory_semantics::SemanticMemoryInput,
    service::Service,
    types::{IdentifiedMemoryInput, KnowledgeInput, RestoreInput},
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::IntoResponse,
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

pub(super) fn routes() -> Router<Service> {
    Router::new()
        .route("/admin/deployment/me", get(me))
        .route("/admin/deployment/workspaces", get(workspaces))
        .route("/admin/deployment/workspaces/{workspace}", put(provision))
        .route("/admin/deployment/workspaces/{workspace}/keys", get(keys))
        .route(
            "/admin/deployment/workspaces/{workspace}/keys/rotate",
            post(rotate_key),
        )
        .route(
            "/admin/deployment/workspaces/{workspace}/sessions",
            get(sessions),
        )
        .route(
            "/admin/deployment/workspaces/{workspace}/sessions/{session}/turns",
            get(turns),
        )
        .route("/admin/deployment/workspaces/{workspace}/runs", get(runs))
        .route(
            "/admin/deployment/workspaces/{workspace}/runs/{run}",
            get(run),
        )
        .route(
            "/admin/deployment/workspaces/{workspace}/skill-reviews",
            get(skill_reviews),
        )
        .route(
            "/admin/deployment/workspaces/{workspace}/skill-reviews/{review}",
            get(skill_review),
        )
        .route(
            "/admin/deployment/workspaces/{workspace}/skill-reviews/{review}/decision",
            post(skill_decision),
        )
        .route(
            "/admin/deployment/workspaces/{workspace}/skill-reviews/{review}/publish",
            post(skill_publish),
        )
        .route(
            "/admin/deployment/workspaces/{workspace}/assets",
            get(assets),
        )
        .route(
            "/admin/deployment/workspaces/{workspace}/assets/{asset}",
            get(asset).delete(delete_asset),
        )
        .route(
            "/admin/deployment/workspaces/{workspace}/assets/{asset}/restore",
            post(restore_asset),
        )
        .route(
            "/admin/deployment/workspaces/{workspace}/events/{event}",
            get(content_source).delete(retract_source),
        )
        .route(
            "/admin/deployment/workspaces/{workspace}/memories/identified",
            post(identified_memory),
        )
        .route(
            "/admin/deployment/workspaces/{workspace}/memories/semantic",
            post(semantic_memory),
        )
        .route(
            "/admin/deployment/workspaces/{workspace}/knowledge",
            post(knowledge),
        )
        .route(
            "/admin/deployment/workspaces/{workspace}/files",
            post(upload),
        )
        .route(
            "/admin/deployment/workspaces/{workspace}/jobs/{job}",
            get(job),
        )
        .route(
            "/admin/deployment/workspaces/{workspace}/jobs/{job}/{action}",
            post(job_action),
        )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceInput {
    name: String,
}

#[derive(Deserialize)]
struct KeyPage {
    after: Option<Uuid>,
    limit: Option<u32>,
}

async fn provision(
    State(service): State<Service>,
    headers: HeaderMap,
    Path(workspace): Path<Uuid>,
    Json(input): Json<WorkspaceInput>,
) -> Result<impl IntoResponse> {
    Ok(uncached_json(
        service
            .provision_deployment_workspace(token(&headers)?, workspace, &input.name)
            .await?,
    ))
}

async fn keys(
    State(service): State<Service>,
    headers: HeaderMap,
    Path(workspace): Path<Uuid>,
    Query(page): Query<KeyPage>,
) -> Result<impl IntoResponse> {
    Ok(uncached_json(
        service
            .deployment_workspace_keys(
                token(&headers)?,
                workspace,
                page.after.unwrap_or_default(),
                page.limit.unwrap_or(50),
            )
            .await?,
    ))
}

async fn rotate_key(
    State(service): State<Service>,
    headers: HeaderMap,
    Path(workspace): Path<Uuid>,
) -> Result<impl IntoResponse> {
    Ok(uncached_json(
        service
            .rotate_deployment_workspace_key(token(&headers)?, workspace)
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SequencePage {
    after: Option<i64>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetPage {
    kind: String,
    after: Option<Uuid>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetVersion {
    version: Option<i32>,
}

async fn me(State(service): State<Service>, headers: HeaderMap) -> Result<impl IntoResponse> {
    service
        .authenticate_deployment_admin(token(&headers)?)
        .await?;
    Ok(uncached_json(json!({"role":"deployment_admin"})))
}

async fn workspaces(
    State(service): State<Service>,
    headers: HeaderMap,
    Query(page): Query<CursorPage>,
) -> Result<impl IntoResponse> {
    Ok(uncached_json(
        service
            .inspect_workspaces(
                token(&headers)?,
                page.after.unwrap_or_default(),
                page.limit.unwrap_or(50),
            )
            .await?,
    ))
}

async fn sessions(
    State(service): State<Service>,
    headers: HeaderMap,
    Path(workspace): Path<Uuid>,
    Query(page): Query<CursorPage>,
) -> Result<impl IntoResponse> {
    Ok(uncached_json(
        service
            .inspect_sessions(
                token(&headers)?,
                workspace,
                page.after.unwrap_or_default(),
                page.limit.unwrap_or(50),
            )
            .await?,
    ))
}

async fn turns(
    State(service): State<Service>,
    headers: HeaderMap,
    Path((workspace, session)): Path<(Uuid, Uuid)>,
    Query(page): Query<SequencePage>,
) -> Result<impl IntoResponse> {
    Ok(uncached_json(
        service
            .inspect_turns(
                token(&headers)?,
                workspace,
                session,
                page.after.unwrap_or_default(),
                page.limit.unwrap_or(20),
            )
            .await?,
    ))
}

async fn runs(
    State(service): State<Service>,
    headers: HeaderMap,
    Path(workspace): Path<Uuid>,
    Query(page): Query<CursorPage>,
) -> Result<impl IntoResponse> {
    Ok(uncached_json(
        service
            .inspect_runs(
                token(&headers)?,
                workspace,
                page.after.unwrap_or_default(),
                page.limit.unwrap_or(50),
            )
            .await?,
    ))
}

async fn skill_reviews(
    State(service): State<Service>,
    headers: HeaderMap,
    Path(workspace): Path<Uuid>,
    Query(page): Query<CursorPage>,
) -> Result<impl IntoResponse> {
    let auth = service
        .authorize_deployment_workspace(token(&headers)?, workspace)
        .await?;
    Ok(uncached_json(
        service
            .deployment_skill_reviews(
                &auth,
                page.after.unwrap_or_default(),
                page.limit.unwrap_or(50),
            )
            .await?,
    ))
}

async fn skill_review(
    State(service): State<Service>,
    headers: HeaderMap,
    Path((workspace, review)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse> {
    let auth = service
        .authorize_deployment_workspace(token(&headers)?, workspace)
        .await?;
    Ok(uncached_json(service.skill_review(&auth, review).await?))
}

async fn skill_decision(
    State(service): State<Service>,
    headers: HeaderMap,
    Path((workspace, review)): Path<(Uuid, Uuid)>,
    Json(input): Json<ReviewDecision>,
) -> Result<impl IntoResponse> {
    let auth = service
        .authorize_deployment_workspace(token(&headers)?, workspace)
        .await?;
    Ok(uncached_json(
        service
            .decide_skill_review(&auth, review, &input.digest, input.approve)
            .await?,
    ))
}

async fn skill_publish(
    State(service): State<Service>,
    headers: HeaderMap,
    Path((workspace, review)): Path<(Uuid, Uuid)>,
    Json(input): Json<PublishInput>,
) -> Result<impl IntoResponse> {
    let auth = service
        .authorize_deployment_workspace(token(&headers)?, workspace)
        .await?;
    Ok(uncached_json(json!(
        service.publish_skill(&auth, review, &input.digest).await?
    )))
}

async fn run(
    State(service): State<Service>,
    headers: HeaderMap,
    Path((workspace, run)): Path<(Uuid, Uuid)>,
    Query(page): Query<SequencePage>,
) -> Result<impl IntoResponse> {
    Ok(uncached_json(
        service
            .inspect_run(
                token(&headers)?,
                workspace,
                run,
                page.after.unwrap_or_default(),
                page.limit.unwrap_or(50),
            )
            .await?,
    ))
}

async fn assets(
    State(service): State<Service>,
    headers: HeaderMap,
    Path(workspace): Path<Uuid>,
    Query(page): Query<AssetPage>,
) -> Result<impl IntoResponse> {
    Ok(uncached_json(
        service
            .inspect_assets(
                token(&headers)?,
                workspace,
                &page.kind,
                page.after.unwrap_or_default(),
                page.limit.unwrap_or(50),
            )
            .await?,
    ))
}

async fn asset(
    State(service): State<Service>,
    headers: HeaderMap,
    Path((workspace, asset)): Path<(Uuid, Uuid)>,
    Query(query): Query<AssetVersion>,
) -> Result<impl IntoResponse> {
    Ok(uncached_json(
        service
            .inspect_asset(token(&headers)?, workspace, asset, query.version)
            .await?,
    ))
}

async fn identified_memory(
    State(service): State<Service>,
    headers: HeaderMap,
    Path(workspace): Path<Uuid>,
    Json(input): Json<IdentifiedMemoryInput>,
) -> Result<impl IntoResponse> {
    let auth = service
        .authorize_deployment_workspace(token(&headers)?, workspace)
        .await?;
    Ok(uncached_json(
        service
            .identified_memory(&auth, key(&headers)?, input)
            .await?,
    ))
}

async fn semantic_memory(
    State(service): State<Service>,
    headers: HeaderMap,
    Path(workspace): Path<Uuid>,
    Json(input): Json<SemanticMemoryInput>,
) -> Result<impl IntoResponse> {
    let auth = service
        .authorize_deployment_workspace(token(&headers)?, workspace)
        .await?;
    Ok(uncached_json(
        service
            .semantic_memory(&auth, key(&headers)?, input)
            .await?,
    ))
}

async fn knowledge(
    State(service): State<Service>,
    headers: HeaderMap,
    Path(workspace): Path<Uuid>,
    Json(input): Json<KnowledgeInput>,
) -> Result<impl IntoResponse> {
    let auth = service
        .authorize_deployment_workspace(token(&headers)?, workspace)
        .await?;
    Ok(uncached_json(
        service.knowledge(&auth, key(&headers)?, input).await?,
    ))
}

async fn delete_asset(
    State(service): State<Service>,
    headers: HeaderMap,
    Path((workspace, asset)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse> {
    let auth = service
        .authorize_deployment_workspace(token(&headers)?, workspace)
        .await?;
    Ok(uncached_json(
        service
            .delete(&auth, key(&headers)?, asset, "asset")
            .await?,
    ))
}

async fn restore_asset(
    State(service): State<Service>,
    headers: HeaderMap,
    Path((workspace, asset)): Path<(Uuid, Uuid)>,
    Json(input): Json<RestoreInput>,
) -> Result<impl IntoResponse> {
    let auth = service
        .authorize_deployment_workspace(token(&headers)?, workspace)
        .await?;
    Ok(uncached_json(
        service.restore(&auth, key(&headers)?, asset, input).await?,
    ))
}

async fn content_source(
    State(service): State<Service>,
    headers: HeaderMap,
    Path((workspace, event)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse> {
    Ok(uncached_json(
        service
            .inspect_content_source(token(&headers)?, workspace, event)
            .await?,
    ))
}

async fn retract_source(
    State(service): State<Service>,
    headers: HeaderMap,
    Path((workspace, event)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse> {
    Ok(uncached_json(
        service
            .retract_deployment_source(token(&headers)?, workspace, key(&headers)?, event)
            .await?,
    ))
}

async fn upload(
    State(service): State<Service>,
    headers: HeaderMap,
    Path(workspace): Path<Uuid>,
    Query(query): Query<Upload>,
    body: Bytes,
) -> Result<impl IntoResponse> {
    let auth = service
        .authorize_deployment_workspace(token(&headers)?, workspace)
        .await?;
    Ok(uncached_json(
        service
            .upload(&auth, key(&headers)?, &query.name, &query.format, &body)
            .await?,
    ))
}

async fn job(
    State(service): State<Service>,
    headers: HeaderMap,
    Path((workspace, job)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse> {
    let auth = service
        .authorize_deployment_workspace(token(&headers)?, workspace)
        .await?;
    Ok(uncached_json(service.job(&auth, job).await?))
}

async fn job_action(
    State(service): State<Service>,
    headers: HeaderMap,
    Path((workspace, job, action)): Path<(Uuid, Uuid, String)>,
) -> Result<impl IntoResponse> {
    let auth = service
        .authorize_deployment_workspace(token(&headers)?, workspace)
        .await?;
    Ok(uncached_json(
        service
            .job_action(&auth, key(&headers)?, job, &action)
            .await?,
    ))
}
