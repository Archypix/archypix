use crate::api::middleware::auth_user::AuthUser;
use crate::infra::error::{AppError, map_sqlx_error};
use crate::repository::tag::TagRepository;
use crate::state::AppState;
use axum::Json;
use axum::extract::{Query, State};
use serde::Deserialize;
use tracing::debug;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct ListTagsQuery {
    pub picture_id: Option<Uuid>,
}

pub async fn list(
    auth: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<ListTagsQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(user = %auth.claims.sub, token_type = auth.token_type(), "list_tags");
    let user_id = auth.user_id()?;

    if let Some(picture_id) = query.picture_id {
        let tags = TagRepository::list_for_picture(&state.db, user_id, picture_id).await?;
        let paths: Vec<String> = tags.into_iter().map(|t| t.tag_path).collect();
        return Ok(Json(serde_json::json!({ "tags": paths })));
    }

    let tags = TagRepository::list_paths_by_user(&state.db, user_id).await?;
    Ok(Json(serde_json::json!({ "tags": tags })))
}

#[derive(Debug, Deserialize)]
pub struct EditPictureTagsRequest {
    pub picture_ids: Vec<Uuid>,
    #[serde(default)]
    pub add_tags: Vec<String>,
    #[serde(default)]
    pub remove_tags: Vec<String>,
}

pub async fn edit(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<EditPictureTagsRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(
        user = %auth.claims.sub,
        token_type = auth.token_type(),
        picture_count = payload.picture_ids.len(),
        add_count = payload.add_tags.len(),
        remove_count = payload.remove_tags.len(),
        "edit_picture_tags"
    );

    if payload.picture_ids.is_empty() {
        return Err(AppError::BadRequest(
            "picture_ids must not be empty".to_string(),
        ));
    }
    if payload.add_tags.is_empty() && payload.remove_tags.is_empty() {
        return Err(AppError::BadRequest(
            "at least one of add_tags or remove_tags must be non-empty".to_string(),
        ));
    }

    let user_id = auth.user_id()?;
    let mut tx = state.db.begin().await.map_err(map_sqlx_error)?;

    TagRepository::batch_remove(
        &mut *tx,
        user_id,
        &payload.picture_ids,
        &payload.remove_tags,
    )
    .await?;
    TagRepository::batch_assign(&mut *tx, user_id, &payload.picture_ids, &payload.add_tags).await?;

    tx.commit().await.map_err(map_sqlx_error)?;

    Ok(Json(serde_json::json!({ "ok": true })))
}
