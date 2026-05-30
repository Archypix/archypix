use crate::domain::picture::{Picture, UploadSession};
use crate::infra::config::Config;
use crate::infra::error::AppError;
use crate::infra::redis::RedisClient;
use crate::infra::s3::StorageClient;
use crate::repository::picture::PictureRepository;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn begin_upload(
    redis: &RedisClient,
    storage: &StorageClient,
    config: &Config,
    user_id: Uuid,
    filename: &str,
) -> Result<(String, String), AppError> {
    if filename.trim().is_empty() {
        return Err(AppError::BadRequest("Filename cannot be empty".to_string()));
    }

    let upload_id = Uuid::new_v4().to_string();
    let s3_key_staging = format!("staging/{}/{}", user_id, upload_id);

    let presigned_url = storage
        .presign_put(&config.s3_bucket_staging, &s3_key_staging)
        .await?;

    let session = UploadSession {
        user_id,
        s3_key_staging,
        filename: filename.to_string(),
    };
    redis
        .set_json_ex(
            &upload_session_key(&upload_id),
            &session,
            config.s3_presign_ttl_secs + 60,
        )
        .await?;

    Ok((upload_id, presigned_url))
}

pub async fn complete_upload(
    db: &PgPool,
    redis: &RedisClient,
    storage: &StorageClient,
    config: &Config,
    user_id: Uuid,
    upload_id: &str,
) -> Result<Picture, AppError> {
    let key = upload_session_key(upload_id);
    let session: UploadSession = redis
        .get_json(&key)
        .await?
        .ok_or_else(|| AppError::BadRequest("Upload session not found or expired".to_string()))?;

    if session.user_id != user_id {
        return Err(AppError::Unauthorized(
            "Upload session belongs to another user".to_string(),
        ));
    }

    let s3_key_original = format!("originals/{}/{}", session.user_id, upload_id);
    storage
        .copy_object(
            &config.s3_bucket_staging,
            &session.s3_key_staging,
            &config.s3_bucket_originals,
            &s3_key_original,
        )
        .await?;
    storage
        .delete_object(&config.s3_bucket_staging, &session.s3_key_staging)
        .await?;

    let picture = PictureRepository::create(
        db,
        session.user_id,
        &s3_key_original,
        Some(&session.filename),
    )
    .await?;
    redis.del(&key).await?;

    Ok(picture)
}

fn upload_session_key(upload_id: &str) -> String {
    format!("upload:{}", upload_id)
}
