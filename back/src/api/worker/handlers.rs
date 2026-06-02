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
use tracing::debug;
use uuid::Uuid;

pub async fn claim_next_job(
    auth: AuthWorker,
    State(state): State<AppState>,
    Query(query): Query<ClaimQuery>,
) -> Result<Json<Option<ClaimJobResponse>>, AppError> {
    let Some(job) = JobRepository::claim_next(&state.db, auth.worker_id(), &query.types).await?
    else {
        return Ok(Json(None));
    };

    debug!(
        worker = auth.worker_id(),
        job_type = job.job_type.to_string(),
        job_id = %job.id,
        "worker: claim_next_job"
    );

    // claim_token was generated and stored by claim_next; forward it to the worker.
    let claim_token = job.claim_token.ok_or_else(|| {
        AppError::InternalServerError("claimed job has no claim_token".to_string())
    })?;

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
            claim_token,
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

    let config = job
        .typed_config()
        .map_err(|e| AppError::InternalServerError(format!("failed to parse job config: {e}")))?;

    // For edit_picture jobs: snapshot the current file as a new version BEFORE
    // issuing the presigned write URL that would overwrite it.
    if job.job_type == JobType::EditPicture {
        let settings =
            UserSettingsRepository::get_or_default(&state.db, picture.local_user_id).await?;
        if settings.versioning_mode != VersioningMode::None {
            let version_id = Uuid::new_v4();
            // S3 copy first (outside DB tx) — safe because no DB record exists yet.
            state
                .storage
                .copy_object(
                    &state.config.s3_bucket_pictures,
                    &original_key,
                    &state.config.s3_bucket_versions,
                    &s3::version_key(picture.local_user_id, picture_id, version_id),
                )
                .await?;
            // DB: insert version record in a transaction so version_number is
            // computed and stored atomically.
            let mut vtx = state.db.begin().await.map_err(|e| {
                AppError::InternalServerError(format!("failed to begin version tx: {e}"))
            })?;
            let version_num =
                PictureVersionRepository::next_version_number(&mut *vtx, picture_id).await?;
            PictureVersionRepository::create(
                &mut *vtx,
                version_id,
                picture_id,
                version_num,
                picture.file_size,
                picture.mime_type.as_deref(),
            )
            .await?;
            vtx.commit().await.map_err(|e| {
                AppError::InternalServerError(format!("failed to commit version tx: {e}"))
            })?;
        }
    }

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
            let output = state
                .storage
                .presign_put_worker(&state.config.s3_bucket_pictures, &original_key)
                .await?;
            if edit_cfg.visual.is_some() {
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
                PresignedWrites::exif_only(output)
            }
        }
        _ => PresignedWrites::default(),
    };

    Ok(Json(Some(ClaimJobResponse {
        job_id: job.id,
        job_type: job.job_type,
        picture_id: job.picture_id,
        mime_type: picture.mime_type.clone(),
        config,
        presigned_read: Some(presigned_read),
        presigned_writes,
        claim_token,
    })))
}

pub async fn complete_job(
    auth: AuthWorker,
    State(state): State<AppState>,
    Path(job_id): Path<Uuid>,
    Json(body): Json<CompleteJobRequest>,
) -> Result<StatusCode, AppError> {
    debug!(worker = auth.worker_id(), job_id = %job_id, "worker: complete_job");

    // Read job outside the transaction to get type/config early (fail fast on
    // corrupt JSONB). The claim_token guard inside the UPDATE makes this safe.
    let job = JobRepository::find_by_id(&state.db, job_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let picture_id = job.picture_id;

    // Pre-parse edit_picture config so we fail before touching the DB if it is corrupt.
    let edit_cfg = if job.job_type == JobType::EditPicture {
        match job.typed_config() {
            Ok(JobConfig::EditPicture(c)) => Some(c),
            Ok(_) => None,
            Err(e) => {
                return Err(AppError::InternalServerError(format!(
                    "failed to parse job config: {e}"
                )));
            }
        }
    } else {
        None
    };

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| AppError::InternalServerError(format!("failed to begin tx: {e}")))?;

    // Update picture columns from worker output.
    if let (Some(exif), Some(pid)) = (&body.exif, picture_id) {
        let captured_at = exif.captured_at.as_deref().and_then(parse_exif_datetime);
        PictureRepository::update_from_worker(
            &mut *tx,
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
            body.file_size,
            body.file_hash.as_deref(),
        )
        .await?;
    } else if let Some(pid) = picture_id {
        // No EXIF (edit_picture or non-initial gen_thumbnail): still update
        // thumbnails_generated_at, blurhash, file_size, file_hash as available.
        PictureRepository::update_after_processing(
            &mut *tx,
            pid,
            body.thumbnails_generated,
            body.blurhash.as_deref(),
            body.file_size,
            body.file_hash.as_deref(),
        )
        .await?;
    }

    // Apply edit_picture EXIF overrides from job config (server-side authoritative values).
    // Done after worker-EXIF path so user-requested overrides always win.
    if let (Some(cfg), Some(pid)) = (&edit_cfg, picture_id) {
        if let Some(ref overrides) = cfg.exif_overrides {
            PictureRepository::apply_exif_overrides(&mut *tx, pid, overrides).await?;
        }
    }

    let result = serde_json::json!({
        "worker_id": auth.worker_id(),
        "has_exif": body.exif.is_some(),
        "has_blurhash": body.blurhash.is_some(),
        "thumbnails_generated": body.thumbnails_generated,
    });

    // Mark job complete — returns None if claim_token mismatch or wrong status.
    let completed = JobRepository::complete(&mut *tx, job_id, body.claim_token, result).await?;
    if completed.is_none() {
        tx.rollback().await.ok();
        return Err(AppError::Conflict(
            "job is no longer in processing state or claim token does not match".to_string(),
        ));
    }

    tx.commit()
        .await
        .map_err(|e| AppError::InternalServerError(format!("failed to commit tx: {e}")))?;

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
        permanent = body.permanent,
        error = %body.error,
        "worker: fail_job"
    );
    let updated = JobRepository::fail(
        &state.db,
        job_id,
        body.claim_token,
        &body.error,
        body.permanent,
    )
    .await?;
    if updated.is_none() {
        return Err(AppError::Conflict(
            "job is no longer in processing state or claim token does not match".to_string(),
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

fn parse_exif_datetime(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .ok()
}
