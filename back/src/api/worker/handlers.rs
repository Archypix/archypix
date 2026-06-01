use crate::api::middleware::auth_worker::AuthWorker;
use crate::api::worker::models::{ClaimJobResponse, CompleteJobRequest, FailJobRequest};
use crate::domain::job::JobType;
use crate::infra::error::AppError;
use crate::infra::s3;
use crate::repository::job::JobRepository;
use crate::repository::picture::PictureRepository;
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::NaiveDateTime;
use serde::Deserialize;
use std::collections::HashMap;
use tracing::{debug, warn};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct ClaimQuery {
    /// Comma-separated list of job types this worker handles.
    /// When omitted, the worker accepts any job type.
    pub types: Option<String>,
}

pub async fn claim_next_job(
    auth: AuthWorker,
    State(state): State<AppState>,
    Query(query): Query<ClaimQuery>,
) -> Result<Json<Option<ClaimJobResponse>>, AppError> {
    debug!(worker = auth.worker_id(), "worker: claim_next_job");

    let job_types: Vec<JobType> = query
        .types
        .as_deref()
        .unwrap_or("")
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .filter_map(|s| match s.trim() {
            "gen_thumbnail" => Some(JobType::GenThumbnail),
            "ml_style" => Some(JobType::MlStyle),
            "ml_people" => Some(JobType::MlPeople),
            "ml_group_location" => Some(JobType::MlGroupLocation),
            "edit_picture" => Some(JobType::EditPicture),
            other => {
                warn!(
                    worker = auth.worker_id(),
                    job_type = other,
                    "unknown job type in filter"
                );
                None
            }
        })
        .collect();

    let Some(job) = JobRepository::claim_next(&state.db, auth.worker_id(), &job_types).await?
    else {
        return Ok(Json(None));
    };

    // Build presigned URLs based on job type.
    let picture_id = job
        .picture_id
        .ok_or_else(|| AppError::InternalServerError("Job has no picture_id".to_string()))?;

    let picture = PictureRepository::find_by_id(&state.db, picture_id)
        .await?
        .ok_or(AppError::NotFound)?;

    // Presigned GET for the original picture.
    let original_key = s3::picture_key(picture.local_user_id, picture_id);
    let presigned_read = state
        .storage
        .presign_get(&state.config.s3_bucket_pictures, &original_key)
        .await?;

    // Presigned PUTs for outputs.
    let mut presigned_writes: HashMap<String, String> = HashMap::new();
    match job.job_type {
        JobType::GenThumbnail => {
            for size in ["small", "medium", "large"] {
                let bucket = match size {
                    "small" => &state.config.s3_bucket_small,
                    "medium" => &state.config.s3_bucket_medium,
                    _ => &state.config.s3_bucket_large,
                };
                let thumb_key = s3::picture_key(picture.local_user_id, picture_id);
                let url = state.storage.presign_put(bucket, &thumb_key).await?;
                presigned_writes.insert(size.to_string(), url);
            }
        }
        JobType::EditPicture => {
            // The edited picture replaces the original in the pictures bucket.
            let url = state
                .storage
                .presign_put(&state.config.s3_bucket_pictures, &original_key)
                .await?;
            presigned_writes.insert("output".to_string(), url);
        }
        _ => {} // ML jobs don't need presigned writes for now
    }

    Ok(Json(Some(ClaimJobResponse {
        job_id: job.id,
        job_type: job.job_type,
        picture_id: job.picture_id,
        config: job.config.0,
        presigned_read: Some(presigned_read),
        presigned_writes,
    })))
}

pub async fn complete_job(
    auth: AuthWorker,
    State(state): State<AppState>,
    Path(job_id): Path<Uuid>,
    Json(body): Json<CompleteJobRequest>,
) -> Result<StatusCode, AppError> {
    debug!(worker = auth.worker_id(), job_id = %job_id, "worker: complete_job");

    let job = JobRepository::find_by_id(&state.db, job_id)
        .await?
        .ok_or(AppError::NotFound)?;

    // Apply EXIF data to the picture if this is an initial thumbnail run.
    if let (JobType::GenThumbnail, Some(picture_id)) = (&job.job_type, job.picture_id) {
        // Parse is_initial from config.
        let is_initial = job
            .config
            .0
            .get("is_initial")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if is_initial {
            if let Some(ref exif) = body.exif {
                let captured_at = parse_exif_datetime(exif.captured_at.as_deref());
                PictureRepository::update_from_worker(
                    &state.db,
                    picture_id,
                    exif.width,
                    exif.height,
                    captured_at,
                    exif.gps_lat,
                    exif.gps_lng,
                    exif.gps_alt,
                    exif.orientation,
                    body.blurhash.as_deref(),
                    exif.exif_data.clone(),
                )
                .await?;
            } else {
                // No EXIF provided — at least mark thumbnails as generated.
                PictureRepository::set_thumbnails_generated(&state.db, picture_id).await?;
            }
        } else {
            PictureRepository::set_thumbnails_generated(&state.db, picture_id).await?;
        }
    }

    // For edit_picture: the worker already uploaded the new file via presigned PUT.
    // Update picture metadata if EXIF overrides were provided.
    if let (JobType::EditPicture, Some(picture_id)) = (&job.job_type, job.picture_id) {
        if let Some(ref exif) = body.exif {
            let captured_at = parse_exif_datetime(exif.captured_at.as_deref());
            PictureRepository::update_from_worker(
                &state.db,
                picture_id,
                exif.width,
                exif.height,
                captured_at,
                exif.gps_lat,
                exif.gps_lng,
                exif.gps_alt,
                exif.orientation,
                body.blurhash.as_deref(),
                exif.exif_data.clone(),
            )
            .await?;
        }
    }

    // Store result and mark complete.
    let result = serde_json::json!({
        "worker_id": auth.worker_id(),
        "has_exif": body.exif.is_some(),
        "has_blurhash": body.blurhash.is_some(),
    });
    JobRepository::complete(&state.db, job_id, result).await?;

    // TODO: trigger in-process tagging pipeline (metadata event) if EXIF was updated.

    Ok(StatusCode::NO_CONTENT)
}

pub async fn fail_job(
    auth: AuthWorker,
    State(state): State<AppState>,
    Path(job_id): Path<Uuid>,
    Json(body): Json<FailJobRequest>,
) -> Result<StatusCode, AppError> {
    debug!(
        worker = auth.worker_id(),
        job_id = %job_id,
        error = %body.error,
        "worker: fail_job"
    );
    JobRepository::fail(&state.db, job_id, &body.error).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Parse EXIF datetime string ("YYYY:MM:DD HH:MM:SS") or ISO-like into a NaiveDateTime.
fn parse_exif_datetime(s: Option<&str>) -> Option<NaiveDateTime> {
    let s = s?;
    // Try EXIF format first
    NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .ok()
}
