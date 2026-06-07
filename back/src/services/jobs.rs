use crate::domain::job::{EditPictureConfig, GenThumbnailConfig, Job, JobConfig};
use crate::infra::error::AppError;
use crate::repository::job::JobRepository;
use crate::repository::picture::PictureRepository;
use sqlx::{Executor, PgPool, Postgres};
use uuid::Uuid;

/// Enqueue a thumbnail + EXIF extraction job for a picture.
///
/// Pass `is_initial = true` for the first-ever run (worker also extracts EXIF).
/// Pass `is_initial = false` to re-generate thumbnails without EXIF re-extraction.
pub async fn enqueue_thumbnail_job<'e, E>(
    ex: E,
    owner_id: Uuid,
    picture_id: Uuid,
    is_initial: bool,
) -> Result<Job, AppError>
where
    E: Executor<'e, Database = Postgres>,
{
    let config = JobConfig::GenThumbnail(GenThumbnailConfig {
        picture_id,
        is_initial,
    });
    let idempotency = if is_initial {
        Some(format!("gen_thumbnail_initial:{picture_id}"))
    } else {
        None
    };
    JobRepository::create(
        ex,
        owner_id,
        Some(picture_id),
        &config,
        idempotency.as_deref(),
    )
    .await
}

/// Enqueue an `edit_picture` job (EXIF override and/or visual transforms).
pub async fn enqueue_edit_picture_job<'e, E>(
    ex: E,
    owner_id: Uuid,
    picture_id: Uuid,
    config: EditPictureConfig,
) -> Result<Job, AppError>
where
    E: Executor<'e, Database = Postgres>,
{
    let job_config = JobConfig::EditPicture(config);
    JobRepository::create(ex, owner_id, Some(picture_id), &job_config, None).await
}

pub async fn get_job(db: &PgPool, job_id: Uuid, user_id: Uuid) -> Result<Job, AppError> {
    let job = JobRepository::find_by_id(db, job_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if job.owner_id != user_id {
        return Err(AppError::NotFound);
    }
    Ok(job)
}

pub async fn list_picture_jobs(
    db: &PgPool,
    picture_id: Uuid,
    user_id: Uuid,
) -> Result<Vec<Job>, AppError> {
    let picture = PictureRepository::find_by_id(db, picture_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if picture.local_user_id != user_id {
        return Err(AppError::NotFound);
    }
    JobRepository::list_by_picture(db, picture_id, user_id).await
}

/// Validate ownership and enqueue an edit_picture job.
///
/// - Returns `NotFound` if the picture does not belong to `user_id`.
/// - Returns `BadRequest` if the picture is received (not owned).
pub async fn enqueue_edit_for_user(
    db: &PgPool,
    user_id: Uuid,
    picture_id: Uuid,
    config: EditPictureConfig,
) -> Result<Job, AppError> {
    let picture = PictureRepository::find_by_id(db, picture_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if picture.local_user_id != user_id {
        return Err(AppError::NotFound);
    }
    if !picture.is_owned() {
        return Err(AppError::BadRequest(
            "Cannot edit a picture received via federation".to_string(),
        ));
    }
    let config = EditPictureConfig {
        picture_id,
        ..config
    };
    enqueue_edit_picture_job(db, user_id, picture_id, config).await
}
