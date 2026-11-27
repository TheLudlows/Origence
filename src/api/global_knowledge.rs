use super::http::{CursorPage, auth, token, uncached_json};
use crate::{
    error::Result,
    service::{GlobalKnowledgeDecision, Service},
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;
use uuid::Uuid;

pub(super) fn routes() -> Router<Service> {
    Router::new()
        .route("/v1/global-knowledge", get(catalog).post(submit))
        .route("/v1/global-knowledge/{id}", get(snapshot))
        .route("/admin/deployment/global-knowledge", get(reviews))
        .route(
            "/admin/deployment/global-knowledge/{id}",
            get(review_detail),
        )
        .route(
            "/admin/deployment/global-knowledge/{id}/review",
            post(review),
        )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Submission {
    asset_id: Uuid,
    version: i32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewPage {
    state: Option<String>,
    after: Option<Uuid>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Review {
    decision: GlobalKnowledgeDecision,
}

async fn submit(
    State(service): State<Service>,
    headers: HeaderMap,
    Json(input): Json<Submission>,
) -> Result<impl IntoResponse> {
    let auth = auth(&service, &headers).await?;
    Ok(uncached_json(
        service
            .submit_global_knowledge(&auth, input.asset_id, input.version)
            .await?,
    ))
}

async fn catalog(
    State(service): State<Service>,
    headers: HeaderMap,
    Query(page): Query<CursorPage>,
) -> Result<impl IntoResponse> {
    let auth = auth(&service, &headers).await?;
    Ok(uncached_json(
        service
            .global_knowledge_catalog(
                &auth,
                page.after.unwrap_or_default(),
                page.limit.unwrap_or(50),
            )
            .await?,
    ))
}

async fn snapshot(
    State(service): State<Service>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse> {
    let auth = auth(&service, &headers).await?;
    Ok(uncached_json(service.global_knowledge(&auth, id).await?))
}

async fn reviews(
    State(service): State<Service>,
    headers: HeaderMap,
    Query(page): Query<ReviewPage>,
) -> Result<impl IntoResponse> {
    Ok(uncached_json(
        service
            .global_review_catalog(
                token(&headers)?,
                page.state.as_deref().unwrap_or("pending"),
                page.after.unwrap_or_default(),
                page.limit.unwrap_or(50),
            )
            .await?,
    ))
}

async fn review_detail(
    State(service): State<Service>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse> {
    Ok(uncached_json(
        service.global_review_detail(token(&headers)?, id).await?,
    ))
}

async fn review(
    State(service): State<Service>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<Review>,
) -> Result<impl IntoResponse> {
    Ok(uncached_json(
        service
            .review_global_knowledge(token(&headers)?, id, input.decision)
            .await?,
    ))
}
