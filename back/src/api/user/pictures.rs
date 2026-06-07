use crate::api::middleware::auth_user::AuthUser;
use crate::infra::error::AppError;
use crate::services;
use crate::services::pictures::{
    PictureListParams, PictureListResult, PictureVariant, UploadMetadata,
};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};
use tracing::debug;
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
    debug!(user = %auth.claims.sub, token_type = auth.token_type(), filename = %payload.filename, "create_upload");
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
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(user = %auth.claims.sub, token_type = auth.token_type(), picture_id = %picture_id, "complete_upload");
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
    Ok(Json(serde_json::json!({ "id": picture.id })))
}

pub async fn list(
    auth: AuthUser,
    State(state): State<AppState>,
    Query(params): Query<PictureListParams>,
) -> Result<Json<PictureListResult>, AppError> {
    debug!(user = %auth.claims.sub, token_type = auth.token_type(), page = params.page, page_size = params.page_size, "list_pictures");
    let result = services::pictures::list_pictures(
        &state.db,
        &state.redis,
        &state.storage,
        &state.config,
        &state.federation,
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
    debug!(user = %auth.claims.sub, token_type = auth.token_type(), picture_id = %picture_id, variant = ?query.variant, "picture_url");
    let url = services::pictures::presign_picture_variant(
        &state.db,
        &state.redis,
        &state.storage,
        &state.config,
        &state.federation,
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
    debug!(user = %auth.claims.sub, token_type = auth.token_type(), picture_id = %picture_id, "picture_details");
    let d = services::pictures::get_picture_details(&state.db, auth.user_id()?, picture_id).await?;
    Ok(Json(serde_json::json!({
        "id": d.picture.id,
        "filename": d.picture.filename,
        "mime_type": d.picture.mime_type,
        "file_size": d.picture.file_size,
        "width": d.picture.width,
        "height": d.picture.height,
        "captured_at": d.picture.captured_at,
        "ingested_at": d.picture.ingested_at,
        "updated_at": d.picture.updated_at,
        "owner_username": d.picture.owner_username,
        "owner_instance_domain": d.picture.owner_instance_domain,
        "versions": d.versions,
    })))
}
