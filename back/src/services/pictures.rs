use crate::domain::picture::{Picture, UploadSession};
use crate::domain::user_settings::VersioningMode;
use crate::infra::config::Config;
use crate::infra::error::AppError;
use crate::infra::redis::RedisClient;
use crate::infra::s3::{self, StorageClient};
use crate::repository::picture::{
    PictureListFilter, PictureRepository, PictureSortField, SortOrder,
};
use crate::repository::picture_version::PictureVersionRepository;
use crate::repository::user_settings::UserSettingsRepository;
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

/// Selectable picture variant for presigning. Used both in list thumbnails and the per-picture URL endpoint.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PictureVariant {
    Original,
    Small,
    Medium,
    Large,
}

impl PictureVariant {
    pub fn bucket<'a>(&self, config: &'a Config) -> &'a str {
        match self {
            PictureVariant::Original => &config.s3_bucket_pictures,
            PictureVariant::Small => &config.s3_bucket_small,
            PictureVariant::Medium => &config.s3_bucket_medium,
            PictureVariant::Large => &config.s3_bucket_large,
        }
    }
}

// Keep the old name as an alias so list_pictures still compiles.
pub type ThumbnailSize = PictureVariant;

#[derive(Debug, Clone, Deserialize)]
pub struct UploadMetadata {
    pub mime_type: Option<String>,
    pub file_size: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub exif_data: Option<serde_json::Value>,
    pub captured_at: Option<NaiveDateTime>,
}

fn default_page() -> u32 {
    1
}
fn default_page_size() -> u32 {
    50
}

#[derive(Debug, Clone, Deserialize)]
pub struct PictureListParams {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_page_size")]
    pub page_size: u32,
    #[serde(default)]
    pub sort: PictureSortField,
    #[serde(default)]
    pub order: SortOrder,
    pub tag: Option<String>,
    #[serde(default)]
    pub owned_only: bool,
    #[serde(default)]
    pub shared_with_me: bool,
    #[serde(default)]
    pub include_deleted: bool,
    pub captured_after: Option<DateTime<Utc>>,
    pub captured_before: Option<DateTime<Utc>>,
    pub thumbnail: Option<ThumbnailSize>,
}

#[derive(Debug, Serialize)]
pub struct PictureListItem {
    pub id: Uuid,
    pub filename: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub captured_at: Option<NaiveDateTime>,
    pub ingested_at: NaiveDateTime,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PictureListResult {
    pub total: i64,
    pub page: u32,
    pub page_size: u32,
    pub items: Vec<PictureListItem>,
}

pub async fn begin_upload(
    redis: &RedisClient,
    storage: &StorageClient,
    config: &Config,
    user_id: Uuid,
    filename: &str,
) -> Result<(Uuid, String), AppError> {
    if filename.trim().is_empty() {
        return Err(AppError::BadRequest("Filename cannot be empty".to_string()));
    }

    let picture_id = Uuid::new_v4();
    let s3_key_staging = format!("staging/{}/{}", user_id, picture_id);

    let presigned_url = storage
        .presign_put(&config.s3_bucket_staging, &s3_key_staging)
        .await?;

    let session = UploadSession {
        user_id,
        picture_id,
        s3_key_staging,
        filename: filename.to_string(),
    };
    redis
        .set_json_ex(
            &upload_session_key(picture_id),
            &session,
            config.s3_presign_ttl_secs + 60,
        )
        .await?;

    Ok((picture_id, presigned_url))
}

pub async fn complete_upload(
    db: &PgPool,
    redis: &RedisClient,
    storage: &StorageClient,
    config: &Config,
    user_id: Uuid,
    picture_id: Uuid,
    meta: UploadMetadata,
) -> Result<Picture, AppError> {
    let key = upload_session_key(picture_id);
    let session: UploadSession = redis
        .get_json(&key)
        .await?
        .ok_or_else(|| AppError::BadRequest("Upload session not found or expired".to_string()))?;

    if session.user_id != user_id {
        return Err(AppError::Unauthorized(
            "Upload session belongs to another user".to_string(),
        ));
    }

    // Copy staging → pictures bucket
    let pictures_key = s3::picture_key(user_id, picture_id);
    storage
        .copy_object(
            &config.s3_bucket_staging,
            &session.s3_key_staging,
            &config.s3_bucket_pictures,
            &pictures_key,
        )
        .await?;
    storage
        .delete_object(&config.s3_bucket_staging, &session.s3_key_staging)
        .await?;

    let picture = PictureRepository::create(
        db,
        picture_id,
        user_id,
        Some(session.filename.as_str()),
        meta.mime_type.as_deref(),
        meta.file_size,
        meta.width,
        meta.height,
        meta.exif_data.clone(),
        meta.captured_at,
    )
    .await?;

    // If versioning enabled, store the original as version 1 in the versions bucket.
    let settings = UserSettingsRepository::get_or_default(db, user_id).await?;
    if settings.versioning_mode != VersioningMode::None {
        let version_id = Uuid::new_v4();
        storage
            .copy_object(
                &config.s3_bucket_pictures,
                &pictures_key,
                &config.s3_bucket_versions,
                &s3::version_key(user_id, picture_id, version_id),
            )
            .await?;
        PictureVersionRepository::create(
            db,
            picture_id,
            1,
            meta.file_size,
            meta.mime_type.as_deref(),
        )
        .await?;
    }

    redis.del(&key).await?;
    Ok(picture)
}

pub async fn list_pictures(
    db: &PgPool,
    redis: &RedisClient,
    storage: &StorageClient,
    config: &Config,
    user_id: Uuid,
    params: PictureListParams,
) -> Result<PictureListResult, AppError> {
    if params.page_size > 200 {
        return Err(AppError::BadRequest(
            "page_size cannot exceed 200".to_string(),
        ));
    }

    let filter = PictureListFilter {
        page: params.page as i64,
        page_size: params.page_size as i64,
        sort: params.sort,
        order: params.order,
        tag: params.tag,
        owned_only: params.owned_only,
        shared_with_me: params.shared_with_me,
        include_deleted: params.include_deleted,
        captured_after: params.captured_after.map(|dt| dt.naive_utc()),
        captured_before: params.captured_before.map(|dt| dt.naive_utc()),
    };

    let (pictures, total) = PictureRepository::list(db, user_id, &filter).await?;

    let mut items = Vec::with_capacity(pictures.len());
    for pic in pictures {
        let thumbnail_url = match params.thumbnail {
            Some(size) => {
                let key = s3::picture_key(pic.local_user_id, pic.id);
                Some(cached_presign_get(redis, storage, config, size.bucket(config), &key).await?)
            }
            None => None,
        };
        items.push(PictureListItem {
            id: pic.id,
            filename: pic.filename,
            width: pic.width,
            height: pic.height,
            captured_at: pic.captured_at,
            ingested_at: pic.ingested_at,
            thumbnail_url,
        });
    }

    Ok(PictureListResult {
        total,
        page: params.page,
        page_size: params.page_size,
        items,
    })
}

async fn cached_presign_get(
    redis: &RedisClient,
    storage: &StorageClient,
    config: &Config,
    bucket: &str,
    key: &str,
) -> Result<String, AppError> {
    let cache_key = format!("presign:{}:{}", bucket, key);
    if let Some(cached) = redis.get_string(&cache_key).await? {
        return Ok(cached);
    }
    let url = storage.presign_get(bucket, key).await?;
    let ttl = config
        .s3_presign_ttl_secs
        .saturating_sub(config.s3_presign_cache_margin_secs);
    if ttl > 0 {
        redis.set_string_ex(&cache_key, &url, ttl).await?;
    }
    Ok(url)
}

pub async fn presign_picture_variant(
    db: &PgPool,
    redis: &RedisClient,
    storage: &StorageClient,
    config: &Config,
    user_id: Uuid,
    picture_id: Uuid,
    variant: PictureVariant,
) -> Result<String, AppError> {
    let picture = PictureRepository::find_by_id(db, picture_id)
        .await?
        .ok_or(AppError::NotFound)?;

    if picture.local_user_id != user_id {
        return Err(AppError::NotFound);
    }

    let key = s3::picture_key(user_id, picture_id);
    cached_presign_get(redis, storage, config, variant.bucket(config), &key).await
}

fn upload_session_key(picture_id: Uuid) -> String {
    format!("upload:{}", picture_id)
}
