use crate::clients::federation::FederationClient;
use crate::domain::picture::{Picture, UploadSession};
use crate::domain::user_settings::VersioningMode;
use crate::infra::config::Config;
use crate::infra::error::AppError;
use crate::infra::redis::{RedisClient, RedisKey};
use crate::infra::s3::{self, StorageClient};
use crate::repository::picture::{
    PictureListFilter, PictureRepository, PictureSortField, SortOrder,
};
use crate::repository::picture_version::PictureVersionRepository;
use crate::repository::share::IncomingShareRepository;
use crate::repository::user::UserRepository;
use crate::repository::user_settings::UserSettingsRepository;
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::str::FromStr;
use tracing::trace;
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

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }
}

impl FromStr for PictureVariant {
    type Err = AppError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "original" => Ok(Self::Original),
            "small" => Ok(Self::Small),
            "medium" => Ok(Self::Medium),
            "large" => Ok(Self::Large),
            other => Err(AppError::BadRequest(format!("Unknown variant: {other}"))),
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
    /// BlurHash string for progressive loading. `None` until the thumbnail worker runs.
    pub blurhash: Option<String>,
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
    trace!(user_id = %user_id, filename, "pictures: begin_upload");
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
            RedisKey::UploadSession(picture_id),
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
    trace!(user_id = %user_id, picture_id = %picture_id, "pictures: complete_upload");
    let session: UploadSession = redis
        .get_json(RedisKey::UploadSession(picture_id))
        .await?
        .ok_or_else(|| AppError::BadRequest("Upload session not found or expired".to_string()))?;

    if session.user_id != user_id {
        return Err(AppError::Unauthorized(
            "Upload session belongs to another user".to_string(),
        ));
    }

    // S3: copy staging → pictures, then delete staging (S3 ops can't be in a DB tx)
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

    // Single DB transaction: create picture row, thumbnail job.
    let mut tx = db
        .begin()
        .await
        .map_err(|e| AppError::InternalServerError(e.to_string()))?;

    let picture = PictureRepository::create(
        &mut *tx,
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

    // Enqueue initial thumbnail generation + EXIF extraction inside the same transaction
    // so no job is orphaned if the picture insert rolls back.
    crate::services::jobs::enqueue_thumbnail_job(&mut *tx, user_id, picture_id, true).await?;

    tx.commit()
        .await
        .map_err(|e| AppError::InternalServerError(e.to_string()))?;

    // Redis cleanup is after commit — a failure here is non-fatal (session expires on its own).
    if let Err(e) = redis.del(RedisKey::UploadSession(picture_id)).await {
        tracing::warn!(picture_id = %picture_id, error = ?e, "failed to delete upload session from Redis");
    }

    Ok(picture)
}

pub async fn list_pictures(
    db: &PgPool,
    redis: &RedisClient,
    storage: &StorageClient,
    config: &Config,
    federation: &FederationClient,
    user_id: Uuid,
    params: PictureListParams,
) -> Result<PictureListResult, AppError> {
    trace!(user_id = %user_id, page = params.page, page_size = params.page_size, "pictures: list");
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
        let thumbnail_url = if let Some(size) = params.thumbnail {
            Some(
                presign_for_picture(db, redis, storage, config, federation, user_id, &pic, size)
                    .await?,
            )
        } else {
            None
        };
        items.push(PictureListItem {
            id: pic.id,
            filename: pic.filename,
            width: pic.width,
            height: pic.height,
            captured_at: pic.captured_at,
            ingested_at: pic.ingested_at,
            blurhash: pic.blurhash.clone(),
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

/// Resolve the presigned URL for `pic` at the given `variant`.
///
/// Checks `PictureUrl` cache first. On miss: owned picture → local S3; received picture →
/// look up owner in local DB (same-backend share) or call the owner's backend via share_token
/// (cross-instance). Result is cached under the same key regardless of path taken.
async fn presign_for_picture(
    db: &PgPool,
    redis: &RedisClient,
    storage: &StorageClient,
    config: &Config,
    federation: &FederationClient,
    local_user_id: Uuid,
    pic: &Picture,
    variant: PictureVariant,
) -> Result<String, AppError> {
    // Single cache check for all picture types (owned, same-backend share, cross-instance share).
    if let Some(cached) = redis
        .get_string(RedisKey::PictureUrl(pic.id, variant.as_str()))
        .await?
    {
        trace!(picture_id = %pic.id, "presign cache hit");
        return Ok(cached);
    }

    let url = if pic.is_owned() {
        let key = s3::picture_key(pic.local_user_id, pic.id);
        let url = storage.presign_get(variant.bucket(config), &key).await?;
        redis
            .set_string_ex(
                RedisKey::PictureUrl(pic.id, variant.as_str()),
                &url,
                config.s3_presign_ttl_secs,
            )
            .await?;
        url
    } else {
        let owner_username = pic.owner_username.as_deref().unwrap_or_default();
        let owner_instance = pic.owner_instance_domain.as_deref().unwrap_or_default();

        // Check if the owner lives on this backend. Multiple backends can share the same global
        // domain (resolver setup), so we must look the user up in the local DB
        if let Some(owner) = UserRepository::find_by_username(db, owner_username).await? {
            let key = s3::picture_key(owner.id, pic.id);
            storage.presign_get(variant.bucket(config), &key).await?
        } else {
            // Owner is on a different backend — authorise via share_token and call remote.
            let share_token = IncomingShareRepository::find_token_by_sender(
                db,
                local_user_id,
                owner_username,
                owner_instance,
            )
            .await?
            .ok_or_else(|| {
                AppError::Unauthorized(format!(
                    "No active incoming share token for {owner_username}@{owner_instance}"
                ))
            })?;
            federation
                .presign_remote_picture(
                    owner_username,
                    owner_instance,
                    pic.id,
                    variant.as_str(),
                    share_token,
                )
                .await?
        }
    };

    let ttl = config
        .s3_presign_ttl_secs
        .saturating_sub(config.s3_presign_cache_margin_secs);
    if ttl > 0 {
        redis
            .set_string_ex(RedisKey::PictureUrl(pic.id, variant.as_str()), &url, ttl)
            .await?;
    }
    Ok(url)
}

pub async fn presign_picture_variant(
    db: &PgPool,
    redis: &RedisClient,
    storage: &StorageClient,
    config: &Config,
    federation: &FederationClient,
    user_id: Uuid,
    picture_id: Uuid,
    variant: PictureVariant,
) -> Result<String, AppError> {
    trace!(user_id = %user_id, picture_id = %picture_id, variant = ?variant, "pictures: presign_variant");
    let picture = PictureRepository::find_by_id(db, picture_id)
        .await?
        .ok_or(AppError::NotFound)?;

    if picture.local_user_id != user_id {
        return Err(AppError::NotFound);
    }

    presign_for_picture(
        db, redis, storage, config, federation, user_id, &picture, variant,
    )
    .await
}
