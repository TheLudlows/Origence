//! HTTP adapter for agent-data foundation features.
use super::http::auth;
use crate::{
    error::{AppError, Result},
    evidence::EvidenceInput,
    run::{RunEventInput, RunInput},
    service::Service,
    skill::{SkillCandidateInput, SkillPackage, SkillReviewInput, SkillReviewPacket},
    skill_candidate::SelectedExperience,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

pub(super) fn routes() -> Router<Service> {
    Router::new()
        .route("/v1/runs", post(create_run).get(list_runs))
        .route("/v1/runs/overview", get(run_creation_overview))
        .route("/v1/runs/{id}", get(run_trace))
        .route("/v1/runs/{id}/prefix", get(run_prefix))
        .route("/v1/runs/{id}/pdf-source", get(run_pdf_source))
        .route(
            "/v1/runs/{id}/verifications",
            get(run_verifications).post(record_run_verification),
        )
        .route(
            "/v1/runs/{id}/verifications/{verification}/withdraw",
            post(withdraw_run_verification),
        )
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunPage {
    after: Option<Uuid>,
    limit: Option<usize>,
}

async fn list_runs(
    State(service): State<Service>,
    headers: HeaderMap,
    Query(page): Query<RunPage>,
) -> Result<Json<Value>> {
    Ok(Json(
        service
            .list_runs(
                &auth(&service, &headers).await?,
                page.after.unwrap_or_default(),
                page.limit.unwrap_or(50),
            )
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunOverviewQuery {
    window: Option<String>,
}

async fn run_creation_overview(
    State(service): State<Service>,
    headers: HeaderMap,
    Query(query): Query<RunOverviewQuery>,
) -> Result<Json<Value>> {
    let caller = auth(&service, &headers).await?;
    Ok(Json(
        service
            .run_creation_overview(&caller, query.window.as_deref().unwrap_or("24h"))
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
    Ok(Json(
        service
            .prepare_run_context(&caller, run, input.event_id, input.context)
            .await?,
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
    candidate: CandidateDraft,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CandidateDraft {
    Manual(SkillCandidateInput),
    Selected(SelectedExperience),
}

async fn save_skill_candidate(
    State(s): State<Service>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(i): Json<CandidateUpdate>,
) -> Result<Json<Value>> {
    let auth = auth(&s, &h).await?;
    let candidate = match i.candidate {
        CandidateDraft::Manual(candidate) => candidate,
        CandidateDraft::Selected(selection) => selection.candidate()?,
    };
    Ok(Json(
        s.save_skill_candidate(&auth, id, i.expected_revision, candidate)
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VerificationPage {
    #[serde(default)]
    after: Uuid,
    limit: Option<usize>,
}
async fn run_verifications(
    State(service): State<Service>,
    headers: HeaderMap,
    Path(run): Path<Uuid>,
    Query(q): Query<VerificationPage>,
) -> Result<Json<Value>> {
    Ok(Json(
        service
            .run_verifications(
                &auth(&service, &headers).await?,
                run,
                q.after,
                q.limit.unwrap_or(25),
            )
            .await?,
    ))
}
async fn record_run_verification(
    State(service): State<Service>,
    headers: HeaderMap,
    Path(run): Path<Uuid>,
    Json(input): Json<crate::run_verification::VerificationInput>,
) -> Result<Json<Value>> {
    use crate::run_verification::VerificationInput;
    let caller = auth(&service, &headers).await?;
    let record = match input {
        VerificationInput::Linked(input) => {
            service.link_run_verification(&caller, run, input).await?
        }
        VerificationInput::Transcribed(input) => {
            service.record_run_verification(&caller, run, input).await?
        }
    };
    Ok(Json(record))
}
async fn withdraw_run_verification(
    State(service): State<Service>,
    headers: HeaderMap,
    Path((run, id)): Path<(Uuid, Uuid)>,
    Json(input): Json<crate::run_verification::WithdrawRunVerificationInput>,
) -> Result<Json<Value>> {
    Ok(Json(
        service
            .withdraw_run_verification(&auth(&service, &headers).await?, run, id, input)
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunPdfQuery {
    sequence: i64,
    evidence_ref_id: Uuid,
    #[serde(default)]
    original: bool,
}

async fn run_pdf_source(
    State(service): State<Service>,
    headers: HeaderMap,
    Path(run): Path<Uuid>,
    Query(q): Query<RunPdfQuery>,
) -> Result<Response> {
    let caller = auth(&service, &headers).await?;
    let no_store = (header::CACHE_CONTROL, "no-store");
    if q.original {
        let bytes = service
            .run_pdf_original(&caller, run, q.sequence, q.evidence_ref_id)
            .await?;
        Ok((
            [
                no_store,
                (header::CONTENT_TYPE, "application/octet-stream"),
                (header::CONTENT_DISPOSITION, "attachment"),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            ],
            bytes,
        )
            .into_response())
    } else {
        Ok((
            [no_store],
            Json(
                service
                    .run_pdf_preview(&caller, run, q.sequence, q.evidence_ref_id)
                    .await?,
            ),
        )
            .into_response())
    }
}
