use crate::api::middleware::auth_worker::AuthWorker;
use crate::api::worker::models::{ClaimJobResponse, CompleteJobRequest, FailJobRequest};
use crate::domain::job::{JobConfig, JobType};
use crate::domain::user_settings::VersioningMode;
use crate::infra::error::AppError;
use crate::infra::s3;
use crate::repository::job::JobRepository;
use crate::repository::picture::PictureRepository;
use crate::repository::picture_version::PictureVersionRepository;
use crate::repository::user_settings::UserSettingsRepository;
use crate::state::AppState;
use archypix_common::transfer::ClaimQuery;
use archypix_common::transfer::PresignedWrites;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::NaiveDateTime;
use serde::Deserialize;
use tracing::debug;
use uuid::Uuid;

pub async fn claim_next_job(
    auth: AuthWorker,
    State(state): State<AppState>,
    Query(query): Query<ClaimQuery>,
) -> Result<Json<Option<ClaimJobResponse>>, AppError> {
    //debug!(worker = auth.worker_id(), "worker: claim_next_job");

    let Some(job) = JobRepository::claim_next(&state.db, auth.worker_id(), &query.types).await?
    else {
        return Ok(Json(None));
    };

    debug!(
        worker = auth.worker_id(),
        job_type = job.job_type.to_string(),
        "worker: claim_next_job"
    );

    // ML jobs have no picture and no S3 I/O for now — return early with empty presigned fields.
    if matches!(
        job.job_type,
        JobType::MlStyle | JobType::MlPeople | JobType::MlGroupLocation
    ) {
        let config = job.typed_config().map_err(|e| {
            AppError::InternalServerError(format!("failed to parse job config: {e}"))
        })?;
        return Ok(Json(Some(ClaimJobResponse {
            job_id: job.id,
            job_type: job.job_type,
            picture_id: job.picture_id,
            mime_type: None,
            config,
            presigned_read: None,
            presigned_writes: PresignedWrites::default(),
        })));
    }

    let picture_id = job.picture_id.ok_or_else(|| {
        AppError::InternalServerError("claimed job has no picture_id".to_string())
    })?;

    let picture = PictureRepository::find_by_id(&state.db, picture_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let original_key = s3::picture_key(picture.local_user_id, picture_id);
    let presigned_read = state
        .storage
        .presign_get_worker(&state.config.s3_bucket_pictures, &original_key)
        .await?;

    // Parse the job config once — used both to build presigned writes and to populate
    // the response.  Doing this early also catches corrupt JSONB before any S3 calls.
    let config = job
        .typed_config()
        .map_err(|e| AppError::InternalServerError(format!("failed to parse job config: {e}")))?;

    // For edit_picture jobs: create a version snapshot of the current file BEFORE
    // handing out the presigned write URL that would overwrite it.
    if job.job_type == JobType::EditPicture {
        let settings =
            UserSettingsRepository::get_or_default(&state.db, picture.local_user_id).await?;
        if settings.versioning_mode != VersioningMode::None {
            let version_id = Uuid::new_v4();
            state
                .storage
                .copy_object(
                    &state.config.s3_bucket_pictures,
                    &original_key,
                    &state.config.s3_bucket_versions,
                    &s3::version_key(picture.local_user_id, picture_id, version_id),
                )
                .await?;
            let version_num =
                PictureVersionRepository::next_version_number(&state.db, picture_id).await?;
            PictureVersionRepository::create(
                &state.db,
                picture_id,
                version_num,
                picture.file_size,
                picture.mime_type.as_deref(),
            )
            .await?;
        }
    }

    // Build presigned writes based on job type / config.
    // All worker-facing presigns use presign_*_worker so the embedded host
    // matches S3_WORKERS_ENDPOINT (may differ from S3_PUBLIC_ENDPOINT).
    let thumb_key = s3::picture_key(picture.local_user_id, picture_id);
    let presigned_writes = match &config {
        JobConfig::GenThumbnail(_) => PresignedWrites::thumbnails(
            state
                .storage
                .presign_put_worker(&state.config.s3_bucket_small, &thumb_key)
                .await?,
            state
                .storage
                .presign_put_worker(&state.config.s3_bucket_medium, &thumb_key)
                .await?,
            state
                .storage
                .presign_put_worker(&state.config.s3_bucket_large, &thumb_key)
                .await?,
        ),
        JobConfig::EditPicture(edit_cfg) => {
            // output URL is always provided so the worker can re-upload the
            // (possibly EXIF-modified or transformed) original.
            let output = state
                .storage
                .presign_put_worker(&state.config.s3_bucket_pictures, &original_key)
                .await?;
            if edit_cfg.visual.is_some() {
                // Visual transforms → also regenerate thumbnails.
                PresignedWrites::edit_with_visual(
                    output,
                    state
                        .storage
                        .presign_put_worker(&state.config.s3_bucket_small, &thumb_key)
                        .await?,
                    state
                        .storage
                        .presign_put_worker(&state.config.s3_bucket_medium, &thumb_key)
                        .await?,
                    state
                        .storage
                        .presign_put_worker(&state.config.s3_bucket_large, &thumb_key)
                        .await?,
                )
            } else {
                // EXIF-only edit: worker re-uploads the file with updated embedded
                // EXIF, but pixel content is unchanged so thumbnails are not needed.
                PresignedWrites::exif_only(output)
            }
        }
        _ => PresignedWrites::default(), // ML jobs have no presigned writes yet
    };

    Ok(Json(Some(ClaimJobResponse {
        job_id: job.id,
        job_type: job.job_type,
        picture_id: job.picture_id,
        mime_type: picture.mime_type.clone(),
        config,
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

    let picture_id = job.picture_id;

    // Update picture metadata when worker-extracted EXIF is present (gen_thumbnail initial jobs).
    // update_from_worker also sets thumbnails_generated_at via COALESCE.
    if let (Some(exif), Some(pid)) = (&body.exif, picture_id) {
        let captured_at = exif.captured_at.as_deref().and_then(parse_exif_datetime);
        PictureRepository::update_from_worker(
            &state.db,
            pid,
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
    } else if body.thumbnails_generated {
        // No EXIF was extracted (edit_picture or non-initial gen_thumbnail), but thumbnails
        // were generated — mark the timestamp explicitly.
        if let Some(pid) = picture_id {
            PictureRepository::set_thumbnails_generated(&state.db, pid).await?;
        }
    }

    // For edit_picture jobs: apply the user-requested EXIF overrides from the job config.
    // Done after the worker-EXIF path so overrides always take precedence.
    if job.job_type == JobType::EditPicture {
        if let Some(pid) = picture_id {
            let cfg = job.typed_config().map_err(|e| {
                AppError::InternalServerError(format!("failed to parse job config: {e}"))
            })?;
            if let JobConfig::EditPicture(edit_cfg) = cfg {
                if let Some(overrides) = edit_cfg.exif_overrides {
                    crate::repository::picture::PictureRepository::apply_exif_overrides(
                        &state.db, pid, &overrides,
                    )
                    .await?;
                }
            }
        }
    }

    let result = serde_json::json!({
        "worker_id": auth.worker_id(),
        "has_exif": body.exif.is_some(),
        "has_blurhash": body.blurhash.is_some(),
        "thumbnails_generated": body.thumbnails_generated,
    });
    JobRepository::complete(&state.db, job_id, result).await?;

    // TODO: trigger in-process tagging pipeline if EXIF was updated.

    Ok(StatusCode::NO_CONTENT)
}

pub async fn fail_job(
    auth: AuthWorker,
    State(state): State<AppState>,
    Path(job_id): Path<Uuid>,
    Json(body): Json<FailJobRequest>,
) -> Result<StatusCode, AppError> {
    debug!(worker = auth.worker_id(), job_id = %job_id, permanent = body.permanent, error = %body.error, "worker: fail_job");
    JobRepository::fail(&state.db, job_id, &body.error, body.permanent).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Parse an EXIF datetime string (`"YYYY:MM:DD HH:MM:SS"`) or ISO-8601 into `NaiveDateTime`.
fn parse_exif_datetime(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .ok()
}
