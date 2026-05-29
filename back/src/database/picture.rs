use crate::database::models::Picture;
use crate::infrastructure::error::{AppError, map_sqlx_error};
use sqlx::{Executor, Postgres};
use uuid::Uuid;

const PICTURE_COLUMNS: &str =
    "id, local_user_id, remote_picture_id, owner_username, owner_instance_domain,
    s3_key_original, s3_key_small, s3_key_medium, s3_key_large,
    filename, mime_type, file_size, width, height,
    exif_data, metadata, deleted_at, captured_at, ingested_at, updated_at";

pub struct PictureRepository;

impl PictureRepository {
    pub async fn create<'e, E>(
        ex: E,
        local_user_id: Uuid,
        s3_key_original: &str,
        filename: Option<&str>,
    ) -> Result<Picture, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as!(
            Picture,
            r#"
            INSERT INTO pictures (local_user_id, s3_key_original, filename, exif_data, metadata)
            VALUES ($1, $2, $3, '{}'::jsonb, '{}'::jsonb)
            RETURNING id, local_user_id, remote_picture_id, owner_username, owner_instance_domain,
                      s3_key_original, s3_key_small, s3_key_medium, s3_key_large,
                      filename, mime_type, file_size, width, height,
                      exif_data, metadata, deleted_at, captured_at, ingested_at, updated_at
            "#,
            local_user_id,
            s3_key_original,
            filename,
        )
        .fetch_one(ex)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn find_by_id<'e, E>(ex: E, id: Uuid) -> Result<Option<Picture>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as!(
            Picture,
            r#"
            SELECT id, local_user_id, remote_picture_id, owner_username, owner_instance_domain,
                   s3_key_original, s3_key_small, s3_key_medium, s3_key_large,
                   filename, mime_type, file_size, width, height,
                   exif_data, metadata, deleted_at, captured_at, ingested_at, updated_at
            FROM pictures
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(ex)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn find_received_by_remote_id<'e, E>(
        ex: E,
        local_user_id: Uuid,
        remote_picture_id: &str,
    ) -> Result<Option<Picture>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as!(
            Picture,
            r#"
            SELECT id, local_user_id, remote_picture_id, owner_username, owner_instance_domain,
                   s3_key_original, s3_key_small, s3_key_medium, s3_key_large,
                   filename, mime_type, file_size, width, height,
                   exif_data, metadata, deleted_at, captured_at, ingested_at, updated_at
            FROM pictures
            WHERE local_user_id = $1
              AND remote_picture_id = $2
            "#,
            local_user_id,
            remote_picture_id
        )
        .fetch_optional(ex)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn list_by_local_user<'e, E>(
        ex: E,
        local_user_id: Uuid,
    ) -> Result<Vec<Picture>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as!(
            Picture,
            r#"
            SELECT id, local_user_id, remote_picture_id, owner_username, owner_instance_domain,
                   s3_key_original, s3_key_small, s3_key_medium, s3_key_large,
                   filename, mime_type, file_size, width, height,
                   exif_data, metadata, deleted_at, captured_at, ingested_at, updated_at
            FROM pictures
            WHERE local_user_id = $1
              AND deleted_at IS NULL
            ORDER BY ingested_at DESC
            "#,
            local_user_id
        )
        .fetch_all(ex)
        .await
        .map_err(map_sqlx_error)
    }
}
