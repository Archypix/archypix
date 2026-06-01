use crate::domain::job::{EditPictureConfig, GenThumbnailConfig, Job, JobType};
use crate::infra::error::AppError;
use crate::repository::job::JobRepository;
use sqlx::PgPool;
use uuid::Uuid;

/// Enqueue the initial thumbnail + EXIF extraction job for a newly-uploaded picture.
/// Uses an idempotency key so double-calling for the same picture is safe.
pub async fn enqueue_thumbnail_job(
    db: &PgPool,
    owner_id: Uuid,
    picture_id: Uuid,
) -> Result<Job, AppError> {
    let config = GenThumbnailConfig {
        picture_id,
        is_initial: true,
    };
    let idempotency = format!("gen_thumbnail_initial:{}", picture_id);
    JobRepository::create(
        db,
        owner_id,
        JobType::GenThumbnail,
        Some(picture_id),
        serde_json::to_value(config).map_err(|e| AppError::InternalServerError(e.to_string()))?,
        Some(&idempotency),
    )
    .await
}

/// Enqueue an `edit_picture` job (EXIF override and/or thumbnail regeneration).
pub async fn enqueue_edit_picture_job(
    db: &PgPool,
    owner_id: Uuid,
    picture_id: Uuid,
    config: EditPictureConfig,
) -> Result<Job, AppError> {
    JobRepository::create(
        db,
        owner_id,
        JobType::EditPicture,
        Some(picture_id),
        serde_json::to_value(config).map_err(|e| AppError::InternalServerError(e.to_string()))?,
        None, // edit jobs are not deduplicated (user may submit multiple edits)
    )
    .await
}
