//! HTTP adapter for agent-data foundation features.
use super::http::auth;
use crate::{
    error::{AppError, Result},
    evidence::EvidenceInput,
    run::{RunEventInput, RunInput},
    service::Service,
    skill::{SkillCandidateInput, SkillPackage, SkillReviewInput, SkillReviewPacket},
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

pub(super) fn routes() -> Router<Service> {
    Router::new()
        .route("/v1/runs", post(create_run))
        .route("/v1/runs/{id}", get(run_trace))
        .route("/v1/runs/{id}/prefix", get(run_prefix))
        .route("/v1/runs/{id}/events", post(append_run_event))
        .route("/v1/runs/{id}/context", post(prepare_run_context))
        .route(
            "/v1/skill-candidates/{id}",
            put(save_skill_candidate).get(skill_candidate),
        )
        .route(
            "/v1/skill-candidates/{id}/reviews/{review}",
            post(submit_skill_review),
        )
        .route("/v1/skill-reviews/{id}", get(skill_review))
        .route("/v1/skill-reviews/{id}/decision", post(decide_skill_review))
        .route("/v1/skill-reviews/{id}/publish", post(publish_skill))
        .route(
            "/v1/skills/{id}/versions/{version}/package",
            get(skill_package),
        )
        .route(
            "/v1/skills/{id}/versions/{version}",
            axum::routing::delete(withdraw_skill),
        )
}

async fn create_run(
    State(service): State<Service>,
    headers: HeaderMap,
    Json(input): Json<RunInput>,
) -> Result<Json<Value>> {
    Ok(Json(
        service
            .create_run(&auth(&service, &headers).await?, input)
            .await?,
    ))
}

async fn append_run_event(
    State(service): State<Service>,
    headers: HeaderMap,
    Path(run): Path<Uuid>,
    Json(input): Json<RunEventInput>,
) -> Result<Json<Value>> {
    Ok(Json(
        service
            .append_run_event(&auth(&service, &headers).await?, run, input)
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunContextInput {
    event_id: Uuid,
    context: EvidenceInput,
}

async fn prepare_run_context(
    State(service): State<Service>,
    headers: HeaderMap,
    Path(run): Path<Uuid>,
    Json(input): Json<RunContextInput>,
) -> Result<Json<Value>> {
    if input.event_id.is_nil() {
        return Err(AppError::Invalid("event_id must not be nil".into()));
    }
    let caller = auth(&service, &headers).await?;
    caller.require("write")?;
    service.check_run_owner(&caller, run).await?;
    input
        .context
        .validate()
        .map_err(|e| AppError::Invalid(e.into()))?;
    let request_digest = crate::storage::hash(
        serde_json::to_vec(&input.context)
            .map_err(|_| AppError::Invalid("invalid context request".into()))?,
    );
    if let Some(prepared) = service
        .replay_run_context_prepared(&caller, run, input.event_id, &request_digest)
        .await?
    {
        return prepared_context_response(prepared);
    }
    let (result, observations) = service
        .observed_evidence_bundle(&caller, input.context)
        .await;
    let bundle = match result {
        Ok(bundle) => bundle,
        Err(error) => {
            if let Some(prepared) = service
                .record_run_context_failed(
                    &caller,
                    run,
                    input.event_id,
                    &request_digest,
                    error.code(),
                    &observations,
                )
                .await?
            {
                return prepared_context_response(prepared);
            }
            return Err(error);
        }
    };
    let preparation = service
        .record_run_context_prepared(&caller, run, input.event_id, &request_digest, &bundle)
        .await?;
    prepared_context_response(preparation)
}

fn prepared_context_response(mut preparation: Value) -> Result<Json<Value>> {
    let bundle = preparation
        .as_object_mut()
        .and_then(|v| v.remove("recorded_bundle"))
        .ok_or_else(|| AppError::Unavailable("invalid prepared context snapshot".into()))?;
    Ok(Json(
        serde_json::json!({"bundle":bundle,"preparation":preparation}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PrefixQuery {
    through_sequence: i64,
}

async fn run_prefix(
    State(service): State<Service>,
    headers: HeaderMap,
    Path(run): Path<Uuid>,
    Query(query): Query<PrefixQuery>,
) -> Result<Json<Value>> {
    Ok(Json(
        service
            .run_prefix(
                &auth(&service, &headers).await?,
                run,
                query.through_sequence,
            )
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceQuery {
    #[serde(default)]
    after: i64,
    limit: Option<usize>,
}

async fn run_trace(
    State(service): State<Service>,
    headers: HeaderMap,
    Path(run): Path<Uuid>,
    Query(query): Query<TraceQuery>,
) -> Result<Json<Value>> {
    Ok(Json(
        service
            .run_trace(
                &auth(&service, &headers).await?,
                run,
                query.after,
                query.limit.unwrap_or(50),
            )
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateUpdate {
    expected_revision: u32,
    candidate: SkillCandidateInput,
}

async fn save_skill_candidate(
    State(s): State<Service>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(i): Json<CandidateUpdate>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.save_skill_candidate(&auth(&s, &h).await?, id, i.expected_revision, i.candidate)
            .await?,
    ))
}

async fn skill_candidate(
    State(s): State<Service>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(s.skill_candidate(&auth(&s, &h).await?, id).await?))
}

async fn submit_skill_review(
    State(s): State<Service>,
    h: HeaderMap,
    Path((id, review)): Path<(Uuid, Uuid)>,
    Json(i): Json<SkillReviewInput>,
) -> Result<Json<SkillReviewPacket>> {
    Ok(Json(
        s.submit_skill_review(&auth(&s, &h).await?, id, review, i)
            .await?,
    ))
}

async fn skill_review(
    State(s): State<Service>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(s.skill_review(&auth(&s, &h).await?, id).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewDecision {
    digest: String,
    approve: bool,
}

async fn decide_skill_review(
    State(s): State<Service>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(i): Json<ReviewDecision>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.decide_skill_review(&auth(&s, &h).await?, id, &i.digest, i.approve)
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishInput {
    digest: String,
}

async fn publish_skill(
    State(s): State<Service>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(i): Json<PublishInput>,
) -> Result<Json<SkillPackage>> {
    Ok(Json(
        s.publish_skill(&auth(&s, &h).await?, id, &i.digest).await?,
    ))
}

async fn skill_package(
    State(s): State<Service>,
    h: HeaderMap,
    Path((id, version)): Path<(Uuid, u32)>,
) -> Result<Json<SkillPackage>> {
    Ok(Json(
        s.skill_package(&auth(&s, &h).await?, id, version).await?,
    ))
}

async fn withdraw_skill(
    State(s): State<Service>,
    h: HeaderMap,
    Path((id, version)): Path<(Uuid, u32)>,
) -> Result<Json<Value>> {
    Ok(Json(
        s.withdraw_skill(&auth(&s, &h).await?, id, version).await?,
    ))
}
