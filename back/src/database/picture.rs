use crate::database::models::Picture;
use crate::infrastructure::error::{AppError, map_sqlx_error};
use sqlx::PgPool;
use uuid::Uuid;

pub struct PictureRepository;

impl PictureRepository {
    pub async fn create(
        pool: &PgPool,
        owner_id: Uuid,
        picture_id: &str,
        s3_key: &str,
        s3_bucket: &str,
        filename: Option<&str>,
    ) -> Result<Picture, AppError> {
        sqlx::query_as!(
            Picture,
            r#"
            INSERT INTO pictures (
                owner_id,
                picture_id,
                s3_key,
                s3_bucket,
                filename,
                exif_data,
                metadata
            )
            VALUES ($1, $2, $3, $4, $5, '{}'::jsonb, '{}'::jsonb)
            RETURNING id, owner_id, picture_id, owner_username, owner_instance_domain,
                      s3_key, s3_bucket, filename, mime_type, file_size, width, height,
                      exif_data, metadata, deleted_at, captured_at, ingested_at, updated_at
            "#,
            owner_id,
            picture_id,
            s3_key,
            s3_bucket,
            filename.unwrap_or("New picture"),
        )
        .fetch_one(pool)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn find_by_id(pool: &PgPool, picture_id: Uuid) -> Result<Option<Picture>, AppError> {
        sqlx::query_as!(
            Picture,
            r#"
            SELECT id, owner_id, picture_id, owner_username, owner_instance_domain,
                   s3_key, s3_bucket, filename, mime_type, file_size, width, height,
                   exif_data, metadata, deleted_at, captured_at, ingested_at, updated_at
            FROM pictures
            WHERE id = $1
            "#,
            picture_id
        )
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn find_owned_by_picture_id(
        pool: &PgPool,
        owner_id: Uuid,
        picture_id: &str,
    ) -> Result<Option<Picture>, AppError> {
        sqlx::query_as!(
            Picture,
            r#"
            SELECT id, owner_id, picture_id, owner_username, owner_instance_domain,
                   s3_key, s3_bucket, filename, mime_type, file_size, width, height,
                   exif_data, metadata, deleted_at, captured_at, ingested_at, updated_at
            FROM pictures
            WHERE owner_id = $1
              AND picture_id = $2
              AND owner_username IS NULL
            "#,
            owner_id,
            picture_id
        )
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn list_by_owner(pool: &PgPool, owner_id: Uuid) -> Result<Vec<Picture>, AppError> {
        sqlx::query_as!(
            Picture,
            r#"
            SELECT id, owner_id, picture_id, owner_username, owner_instance_domain,
                   s3_key, s3_bucket, filename, mime_type, file_size, width, height,
                   exif_data, metadata, deleted_at, captured_at, ingested_at, updated_at
            FROM pictures
            WHERE owner_id = $1
              AND deleted_at IS NULL
            ORDER BY ingested_at DESC
            "#,
            owner_id
        )
        .fetch_all(pool)
        .await
        .map_err(map_sqlx_error)
    }
}
