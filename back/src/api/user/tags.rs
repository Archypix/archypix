use crate::api::middleware::auth_user::AuthUser;
use crate::domain::tag::TagPath;
use crate::infra::error::AppError;
use crate::repository::tag::TagRepository;
use crate::services;
use crate::state::AppState;

fn parse_tag_paths(paths: &[String]) -> Result<Vec<String>, AppError> {
    paths
        .iter()
        .map(|p| {
            TagPath::parse(p)
                .map(|t| t.as_ltree().to_string())
                .map_err(AppError::BadRequest)
        })
        .collect()
}
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
        "edit_picture_tags"
    );
    let add_tags = parse_tag_paths(&payload.add_tags)?;
    let remove_tags = parse_tag_paths(&payload.remove_tags)?;
    services::tags::edit_picture_tags(
        &state.db,
        auth.user_id()?,
        &payload.picture_ids,
        &add_tags,
        &remove_tags,
    )
    .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
