use crate::api::middleware::auth_user::AuthUser;
use crate::database::picture::PictureRepository;
use crate::database::tag::TagRepository;
use crate::infrastructure::error::AppError;
use crate::infrastructure::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct TagRequest {
    pub picture_id: uuid::Uuid,
    pub tags: Vec<String>,
}

pub async fn list_tags(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = auth
        .claims
        .uid
        .ok_or_else(|| AppError::Unauthorized("Missing user id".to_string()))?;

    let tags = TagRepository::list_by_owner(&state.db, user_id).await?;
    Ok(Json(serde_json::json!({ "tags": tags })))
}

pub async fn assign_tags(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<Vec<TagRequest>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = auth
        .claims
        .uid
        .ok_or_else(|| AppError::Unauthorized("Missing user id".to_string()))?;

    for item in &payload {
        let picture = PictureRepository::find_by_id(&state.db, item.picture_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if picture.owner_id != user_id {
            return Err(AppError::Unauthorized("Invalid picture".to_string()));
        }
        TagRepository::assign_tags(&state.db, item.picture_id, &item.tags).await?;
    }

    Ok(Json(serde_json::json!({ "assigned": true })))
}

pub async fn remove_tags(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<Vec<TagRequest>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = auth
        .claims
        .uid
        .ok_or_else(|| AppError::Unauthorized("Missing user id".to_string()))?;

    for item in &payload {
        let picture = PictureRepository::find_by_id(&state.db, item.picture_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if picture.owner_id != user_id {
            return Err(AppError::Unauthorized("Invalid picture".to_string()));
        }
        TagRepository::remove_tags(&state.db, item.picture_id, &item.tags).await?;
    }

    Ok(Json(serde_json::json!({ "removed": true })))
}

pub async fn assign_picture_tags(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(picture_id): Path<uuid::Uuid>,
    Json(payload): Json<Vec<String>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = auth
        .claims
        .uid
        .ok_or_else(|| AppError::Unauthorized("Missing user id".to_string()))?;

    let picture = PictureRepository::find_by_id(&state.db, picture_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if picture.owner_id != user_id {
        return Err(AppError::Unauthorized("Invalid picture".to_string()));
    }

    TagRepository::assign_tags(&state.db, picture_id, &payload).await?;
    Ok(Json(serde_json::json!({ "assigned": true })))
}

pub async fn remove_picture_tags(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(picture_id): Path<uuid::Uuid>,
    Json(payload): Json<Vec<String>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = auth
        .claims
        .uid
        .ok_or_else(|| AppError::Unauthorized("Missing user id".to_string()))?;

    let picture = PictureRepository::find_by_id(&state.db, picture_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if picture.owner_id != user_id {
        return Err(AppError::Unauthorized("Invalid picture".to_string()));
    }

    TagRepository::remove_tags(&state.db, picture_id, &payload).await?;
    Ok(Json(serde_json::json!({ "removed": true })))
}
