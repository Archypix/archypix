use crate::api::middleware::auth_user::AuthUser;
use crate::database::picture::PictureRepository;
use crate::infrastructure::error::AppError;
use crate::infrastructure::state::AppState;
use crate::infrastructure::storage_service::StorageService;
use axum::Json;
use axum::extract::{Path, State};
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
    s3_key_original: String,
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
    let s3_key_original = format!("originals/{}/{}", user_id, upload_id);

    let storage = StorageService::new(
        state.s3.clone(),
        Duration::from_secs(state.config.s3_presign_ttl_secs),
    );
    let presigned_url = storage
        .presign_put(&state.config.s3_bucket_originals, &s3_key_original)
        .await?;

    let session = UploadSession {
        user_id,
        s3_key_original,
        filename: payload.filename,
    };

    state
        .redis
        .set_json_ex(
            &format!("upload:{}", upload_id),
            &session,
            state.config.s3_presign_ttl_secs,
        )
        .await?;

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

    let redis_key = format!("upload:{}", upload_id);

    let session = state
        .redis
        .get_json::<UploadSession>(&redis_key)
        .await?
        .ok_or_else(|| AppError::BadRequest("Upload session expired".to_string()))?;

    if session.user_id != user_id {
        return Err(AppError::Unauthorized("Invalid upload session".to_string()));
    }

    let picture = PictureRepository::create(
        &state.db,
        session.user_id,
        &session.s3_key_original,
        Some(&session.filename),
    )
    .await?;

    state.redis.del(&redis_key).await?;

    Ok(Json(serde_json::json!({
        "id": picture.id,
        "filename": picture.filename,
        "s3_key_original": picture.s3_key_original,
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

    let pictures = PictureRepository::list_by_local_user(&state.db, user_id).await?;
    let items: Vec<_> = pictures
        .into_iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "filename": p.filename,
                "captured_at": p.captured_at,
                "ingested_at": p.ingested_at,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "items": items })))
}

pub async fn get_picture(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(picture_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = auth
        .claims
        .uid
        .ok_or_else(|| AppError::Unauthorized("Missing user id".to_string()))?;

    let picture = PictureRepository::find_by_id(&state.db, picture_id)
        .await?
        .ok_or(AppError::NotFound)?;

    if picture.local_user_id != user_id {
        return Err(AppError::NotFound);
    }

    Ok(Json(serde_json::json!({
        "id": picture.id,
        "filename": picture.filename,
        "mime_type": picture.mime_type,
        "width": picture.width,
        "height": picture.height,
        "captured_at": picture.captured_at,
        "ingested_at": picture.ingested_at,
        "owner_username": picture.owner_username,
        "owner_instance_domain": picture.owner_instance_domain,
    })))
}

pub async fn download_picture(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(picture_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = auth
        .claims
        .uid
        .ok_or_else(|| AppError::Unauthorized("Missing user id".to_string()))?;

    let picture = PictureRepository::find_by_id(&state.db, picture_id)
        .await?
        .ok_or(AppError::NotFound)?;

    if picture.local_user_id != user_id {
        return Err(AppError::NotFound);
    }

    let storage = StorageService::new(
        state.s3.clone(),
        Duration::from_secs(state.config.s3_presign_ttl_secs),
    );

    let url = storage
        .presign_get(&state.config.s3_bucket_originals, &picture.s3_key_original)
        .await?;

    Ok(Json(serde_json::json!({ "url": url })))
}
