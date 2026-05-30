use crate::api::middleware::auth_user::AuthUser;
use crate::infra::error::AppError;
use crate::repository::picture::PictureRepository;
use crate::services;
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};
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

pub async fn create_upload(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<CreateUploadRequest>,
) -> Result<Json<CreateUploadResponse>, AppError> {
    let (upload_id, presigned_url) = services::pictures::begin_upload(
        &state.redis,
        &state.storage,
        &state.config,
        auth.user_id()?,
        &payload.filename,
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
    let picture = services::pictures::complete_upload(
        &state.db,
        &state.redis,
        &state.storage,
        &state.config,
        auth.user_id()?,
        &upload_id,
    )
    .await?;
    Ok(Json(serde_json::json!({
        "id": picture.id,
        "filename": picture.filename,
        "s3_key_original": picture.s3_key_original,
    })))
}

pub async fn list(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pictures = PictureRepository::list_by_user(&state.db, auth.user_id()?).await?;
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

pub async fn get(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(picture_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let picture = PictureRepository::find_by_id(&state.db, picture_id)
        .await?
        .ok_or(AppError::NotFound)?;

    if picture.local_user_id != auth.user_id()? {
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

pub async fn download(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(picture_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let picture = PictureRepository::find_by_id(&state.db, picture_id)
        .await?
        .ok_or(AppError::NotFound)?;

    if picture.local_user_id != auth.user_id()? {
        return Err(AppError::NotFound);
    }

    let url = state
        .storage
        .presign_get(&state.config.s3_bucket_originals, &picture.s3_key_original)
        .await?;
    Ok(Json(serde_json::json!({ "url": url })))
}
