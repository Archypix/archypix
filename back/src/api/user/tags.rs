use crate::api::middleware::auth_user::AuthUser;
use crate::infra::error::AppError;
use crate::repository::picture::PictureRepository;
use crate::repository::tag::TagRepository;
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use tracing::debug;

#[derive(Debug, Deserialize)]
pub struct TagRequest {
    pub picture_id: uuid::Uuid,
    pub tags: Vec<String>,
}

pub async fn list(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(user = %auth.claims.sub, token_type = auth.token_type(), "list_tags");
    let tags = TagRepository::list_paths_by_user(&state.db, auth.user_id()?).await?;
    Ok(Json(serde_json::json!({ "tags": tags })))
}

pub async fn assign(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<Vec<TagRequest>>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(user = %auth.claims.sub, token_type = auth.token_type(), count = payload.len(), "assign_tags");
    let user_id = auth.user_id()?;
    for item in &payload {
        let picture = PictureRepository::find_by_id(&state.db, item.picture_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if picture.local_user_id != user_id {
            return Err(AppError::Unauthorized(
                "Picture belongs to another user".to_string(),
            ));
        }
        TagRepository::assign(&state.db, item.picture_id, &item.tags).await?;
    }
    Ok(Json(serde_json::json!({ "assigned": true })))
}

pub async fn remove(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<Vec<TagRequest>>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(user = %auth.claims.sub, token_type = auth.token_type(), count = payload.len(), "remove_tags");
    let user_id = auth.user_id()?;
    for item in &payload {
        let picture = PictureRepository::find_by_id(&state.db, item.picture_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if picture.local_user_id != user_id {
            return Err(AppError::Unauthorized(
                "Picture belongs to another user".to_string(),
            ));
        }
        TagRepository::remove(&state.db, item.picture_id, &item.tags).await?;
    }
    Ok(Json(serde_json::json!({ "removed": true })))
}

pub async fn assign_to_picture(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(picture_id): Path<uuid::Uuid>,
    Json(tags): Json<Vec<String>>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(user = %auth.claims.sub, token_type = auth.token_type(), picture_id = %picture_id, count = tags.len(), "assign_tags_to_picture");
    let picture = PictureRepository::find_by_id(&state.db, picture_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if picture.local_user_id != auth.user_id()? {
        return Err(AppError::Unauthorized(
            "Picture belongs to another user".to_string(),
        ));
    }
    TagRepository::assign(&state.db, picture_id, &tags).await?;
    Ok(Json(serde_json::json!({ "assigned": true })))
}

pub async fn remove_from_picture(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(picture_id): Path<uuid::Uuid>,
    Json(tags): Json<Vec<String>>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(user = %auth.claims.sub, token_type = auth.token_type(), picture_id = %picture_id, count = tags.len(), "remove_tags_from_picture");
    let picture = PictureRepository::find_by_id(&state.db, picture_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if picture.local_user_id != auth.user_id()? {
        return Err(AppError::Unauthorized(
            "Picture belongs to another user".to_string(),
        ));
    }
    TagRepository::remove(&state.db, picture_id, &tags).await?;
    Ok(Json(serde_json::json!({ "removed": true })))
}
