use crate::domain::job::{EditPictureConfig, GenThumbnailConfig, Job, JobConfig};
use crate::infra::error::AppError;
use crate::repository::job::JobRepository;
use sqlx::{Executor, Postgres};
use uuid::Uuid;

/// Enqueue a thumbnail + EXIF extraction job for a picture.
///
/// Pass `is_initial = true` for the first-ever run (worker also extracts EXIF).
/// Pass `is_initial = false` to re-generate thumbnails without EXIF re-extraction
/// (future use, e.g. quality improvement pass).
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
