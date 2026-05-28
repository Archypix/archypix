use crate::api::middleware::auth_user::AuthUser;
use crate::database::picture::PictureRepository;
use crate::infrastructure::error::AppError;
use crate::infrastructure::state::AppState;
use crate::services::storage::StorageService;
use axum::Json;
use axum::extract::{Path, State};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct CreateUploadRequest {
    pub filename: String,
}

#[derive(Debug, Serialize)]
pub struct CreateUploadResponse {
    pub upload_id: String,
    pub presigned_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct UploadSession {
    user_id: Uuid,
    s3_key: String,
    bucket: String,
    filename: String,
}

pub async fn create_upload(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<CreateUploadRequest>,
) -> Result<Json<CreateUploadResponse>, AppError> {
    if payload.filename.trim().is_empty() {
        return Err(AppError::BadRequest("Filename is required".to_string()));
    }

    let user_id = auth
        .claims
        .uid
        .ok_or_else(|| AppError::Unauthorized("Missing user id".to_string()))?;

    let upload_id = Uuid::new_v4().to_string();
    let s3_key = format!("uploads/{}/{}", user_id, upload_id);

    let storage = StorageService::new(
        state.s3.clone(),
        state.config.s3_bucket.clone(),
        Duration::from_secs(state.config.s3_presign_ttl_secs),
    );
    let presigned_url = storage.presign_put(&s3_key).await?;

    let session = UploadSession {
        user_id,
        s3_key,
        bucket: state.config.s3_bucket.clone(),
        filename: payload.filename,
    };

    let mut redis = state.redis.clone();
    let key = format!("upload:{}", upload_id);
    let value = serde_json::to_string(&session)
        .map_err(|err| AppError::InternalServerError(err.to_string()))?;
    let _: () = redis
        .set_ex(&key, value, state.config.s3_presign_ttl_secs)
        .await
        .map_err(|err| AppError::InternalServerError(err.to_string()))?;

    Ok(Json(CreateUploadResponse {
        upload_id,
        presigned_url,
    }))
}

pub async fn complete_upload(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(upload_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = auth
        .claims
        .uid
        .ok_or_else(|| AppError::Unauthorized("Missing user id".to_string()))?;

    let key = format!("upload:{}", upload_id);
    let mut redis = state.redis.clone();
    let value: Option<String> = redis
        .get(&key)
        .await
        .map_err(|err| AppError::InternalServerError(err.to_string()))?;

    let value = value.ok_or_else(|| AppError::BadRequest("Upload session expired".to_string()))?;
    let session: UploadSession = serde_json::from_str(&value)
        .map_err(|err| AppError::InternalServerError(err.to_string()))?;

    if session.user_id != user_id {
        return Err(AppError::Unauthorized("Invalid upload session".to_string()));
    }

    let picture = PictureRepository::create(
        &state.db,
        session.user_id,
        &upload_id,
        &session.s3_key,
        &session.bucket,
        Some(&session.filename),
    )
    .await?;

    let _: () = redis
        .del(&key)
        .await
        .map_err(|err| AppError::InternalServerError(err.to_string()))?;

    Ok(Json(serde_json::json!({
        "id": picture.id,
        "picture_id": picture.picture_id,
        "filename": picture.filename,
        "s3_key": picture.s3_key
    })))
}

pub async fn list_pictures(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = auth
        .claims
        .uid
        .ok_or_else(|| AppError::Unauthorized("Missing user id".to_string()))?;

    let pictures = PictureRepository::list_by_owner(&state.db, user_id).await?;
    let items: Vec<_> = pictures
        .into_iter()
        .map(|picture| {
            serde_json::json!({
                "id": picture.id,
                "picture_id": picture.picture_id,
                "filename": picture.filename,
                "created_at": picture.ingested_at,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "items": items })))
}

pub async fn get_picture(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(picture_id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = auth
        .claims
        .uid
        .ok_or_else(|| AppError::Unauthorized("Missing user id".to_string()))?;

    let picture = PictureRepository::find_by_id(&state.db, picture_id)
        .await?
        .ok_or(AppError::NotFound)?;

    if picture.owner_id != user_id {
        return Err(AppError::NotFound);
    }

    Ok(Json(serde_json::json!({
        "id": picture.id,
        "picture_id": picture.picture_id,
        "filename": picture.filename,
        "s3_key": picture.s3_key,
        "s3_bucket": picture.s3_bucket
    })))
}

pub async fn download_picture(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(picture_id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = auth
        .claims
        .uid
        .ok_or_else(|| AppError::Unauthorized("Missing user id".to_string()))?;

    let picture = PictureRepository::find_by_id(&state.db, picture_id)
        .await?
        .ok_or(AppError::NotFound)?;

    if picture.owner_id != user_id {
        return Err(AppError::NotFound);
    }

    let storage = StorageService::new(
        state.s3.clone(),
        state.config.s3_bucket.clone(),
        Duration::from_secs(state.config.s3_presign_ttl_secs),
    );

    let url = storage
        .presign_get_in_bucket(&picture.s3_bucket, &picture.s3_key)
        .await?;

    Ok(Json(serde_json::json!({ "url": url })))
}
