use crate::api::middleware::auth_user::AuthUser;
use crate::domain::picture::Picture;
use crate::infra::error::AppError;
use crate::repository::picture_version::PictureVersionRepository;
use crate::services;
use crate::services::pictures::{
    PictureListParams, PictureListResult, PictureVariant, UploadMetadata,
};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct CreateUploadRequest {
    pub filename: String,
}

#[derive(Debug, Serialize)]
pub struct CreateUploadResponse {
    pub picture_id: Uuid,
    pub presigned_url: String,
}

pub async fn create_upload(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<CreateUploadRequest>,
) -> Result<Json<CreateUploadResponse>, AppError> {
    let (picture_id, presigned_url) = services::pictures::begin_upload(
        &state.redis,
        &state.storage,
        &state.config,
        auth.user_id()?,
        &payload.filename,
    )
    .await?;
    Ok(Json(CreateUploadResponse {
        picture_id,
        presigned_url,
    }))
}

pub async fn complete_upload(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(picture_id): Path<Uuid>,
    Json(meta): Json<UploadMetadata>,
) -> Result<Json<Picture>, AppError> {
    let picture = services::pictures::complete_upload(
        &state.db,
        &state.redis,
        &state.storage,
        &state.config,
        auth.user_id()?,
        picture_id,
        meta,
    )
    .await?;
    Ok(Json(picture))
}

pub async fn list(
    auth: AuthUser,
    State(state): State<AppState>,
    Query(params): Query<PictureListParams>,
) -> Result<Json<PictureListResult>, AppError> {
    let result = services::pictures::list_pictures(
        &state.db,
        &state.redis,
        &state.storage,
        &state.config,
        auth.user_id()?,
        params,
    )
    .await?;
    Ok(Json(result))
}

#[derive(Debug, Deserialize)]
pub struct PictureUrlQuery {
    pub variant: PictureVariant,
}

pub async fn picture_url(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(picture_id): Path<Uuid>,
    Query(query): Query<PictureUrlQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let url = services::pictures::presign_picture_variant(
        &state.db,
        &state.redis,
        &state.storage,
        &state.config,
        auth.user_id()?,
        picture_id,
        query.variant,
    )
    .await?;
    Ok(Json(
        serde_json::json!({ "url": url, "variant": query.variant }),
    ))
}

pub async fn details(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(picture_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    use crate::repository::picture::PictureRepository;

    let picture = PictureRepository::find_by_id(&state.db, picture_id)
        .await?
        .ok_or(AppError::NotFound)?;

    if picture.local_user_id != auth.user_id()? {
        return Err(AppError::NotFound);
    }

    let versions = PictureVersionRepository::list_by_picture(&state.db, picture_id).await?;

    Ok(Json(serde_json::json!({
        "id": picture.id,
        "filename": picture.filename,
        "mime_type": picture.mime_type,
        "file_size": picture.file_size,
        "width": picture.width,
        "height": picture.height,
        "captured_at": picture.captured_at,
        "ingested_at": picture.ingested_at,
        "updated_at": picture.updated_at,
        "owner_username": picture.owner_username,
        "owner_instance_domain": picture.owner_instance_domain,
        "versions": versions,
    })))
}
