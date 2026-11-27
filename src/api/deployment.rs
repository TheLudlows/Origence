//! Read-only HTTP endpoints for deployment administrators.
use super::http::token;
use crate::{error::Result, service::Service};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, header},
    response::IntoResponse,
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

pub(super) fn routes() -> Router<Service> {
    Router::new()
        .route("/admin/deployment/me", get(me))
        .route("/admin/deployment/workspaces", get(workspaces))
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
            "/admin/deployment/workspaces/{workspace}/assets",
            get(assets),
        )
        .route(
            "/admin/deployment/workspaces/{workspace}/assets/{asset}",
            get(asset),
        )
}

fn private(value: Value) -> impl IntoResponse {
    ([(header::CACHE_CONTROL, "no-store")], Json(value))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    after: Option<Uuid>,
    limit: Option<usize>,
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
    Ok(private(json!({"role":"deployment_admin"})))
}

async fn workspaces(
    State(service): State<Service>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Result<impl IntoResponse> {
    Ok(private(
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
    Query(page): Query<Page>,
) -> Result<impl IntoResponse> {
    Ok(private(
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
    Ok(private(
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
    Query(page): Query<Page>,
) -> Result<impl IntoResponse> {
    Ok(private(
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

async fn run(
    State(service): State<Service>,
    headers: HeaderMap,
    Path((workspace, run)): Path<(Uuid, Uuid)>,
    Query(page): Query<SequencePage>,
) -> Result<impl IntoResponse> {
    Ok(private(
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
    Ok(private(
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
    Ok(private(
        service
            .inspect_asset(token(&headers)?, workspace, asset, query.version)
            .await?,
    ))
}
