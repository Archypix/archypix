use crate::api::middleware::auth_user::AuthUser;
use crate::domain::job::{EditPictureConfig, Job};
use crate::infra::error::AppError;
use crate::repository::job::JobRepository;
use crate::repository::picture::PictureRepository;
use crate::services;
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use tracing::debug;
use uuid::Uuid;

/// `GET /api/authenticated/jobs/{id}` — get the status of a single job.
pub async fn get_job(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(job_id): Path<Uuid>,
) -> Result<Json<Job>, AppError> {
    debug!(user = %auth.claims.sub, job_id = %job_id, "get_job");
    let job = JobRepository::find_by_id(&state.db, job_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if job.owner_id != auth.user_id()? {
        return Err(AppError::NotFound);
    }
    Ok(Json(job))
}

/// `GET /api/authenticated/pictures/{id}/jobs` — list all jobs for a picture.
pub async fn list_picture_jobs(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(picture_id): Path<Uuid>,
) -> Result<Json<Vec<Job>>, AppError> {
    debug!(user = %auth.claims.sub, picture_id = %picture_id, "list_picture_jobs");
    let jobs = services::jobs::list_picture_jobs(&state.db, picture_id, auth.user_id()?).await?;
    Ok(Json(jobs))
}

/// `POST /api/authenticated/pictures/{id}/edit` — enqueue an edit_picture job.
pub async fn enqueue_edit(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(picture_id): Path<Uuid>,
    Json(config): Json<EditPictureConfig>,
) -> Result<Json<Job>, AppError> {
    debug!(user = %auth.claims.sub, picture_id = %picture_id, "enqueue_edit");
    let job = services::jobs::enqueue_edit_for_user(&state.db, auth.user_id()?, picture_id, config)
        .await?;
    Ok(Json(job))
}
