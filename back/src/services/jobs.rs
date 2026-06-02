use crate::domain::job::{EditPictureConfig, GenThumbnailConfig, Job, JobConfig};
use crate::infra::error::AppError;
use crate::repository::job::JobRepository;
use sqlx::PgPool;
use uuid::Uuid;

/// Enqueue the initial thumbnail + EXIF extraction job for a newly-uploaded picture.
pub async fn enqueue_thumbnail_job(
    db: &PgPool,
    owner_id: Uuid,
    picture_id: Uuid,
) -> Result<Job, AppError> {
    let config = JobConfig::GenThumbnail(GenThumbnailConfig {
        picture_id,
        is_initial: true,
    });
    let idempotency = format!("gen_thumbnail_initial:{picture_id}");
    JobRepository::create(db, owner_id, Some(picture_id), &config, Some(&idempotency)).await
}

/// Enqueue an `edit_picture` job (EXIF override and/or visual transforms).
pub async fn enqueue_edit_picture_job(
    db: &PgPool,
    owner_id: Uuid,
    picture_id: Uuid,
    config: EditPictureConfig,
) -> Result<Job, AppError> {
    let job_config = JobConfig::EditPicture(config);
    JobRepository::create(db, owner_id, Some(picture_id), &job_config, None).await
}
