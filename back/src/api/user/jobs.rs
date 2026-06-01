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

    // Ensure the job belongs to the requesting user.
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

    // Verify the picture belongs to this user.
    let picture = PictureRepository::find_by_id(&state.db, picture_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if picture.local_user_id != auth.user_id()? {
        return Err(AppError::NotFound);
    }

    let jobs = JobRepository::list_by_picture(&state.db, picture_id, auth.user_id()?).await?;
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

    // Verify the picture belongs to this user and is owned (not received).
    let picture = PictureRepository::find_by_id(&state.db, picture_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if picture.local_user_id != auth.user_id()? {
        return Err(AppError::NotFound);
    }
    if !picture.is_owned() {
        return Err(AppError::BadRequest(
            "Cannot edit a picture received via federation".to_string(),
        ));
    }

    // Override the picture_id in the config with the path param (authoritative).
    let config = EditPictureConfig {
        picture_id,
        ..config
    };

    let job =
        services::jobs::enqueue_edit_picture_job(&state.db, auth.user_id()?, picture_id, config)
            .await?;

    Ok(Json(job))
}
