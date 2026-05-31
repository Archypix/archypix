use crate::api::admin::models::{CreateUserRequest, UpdateUserRequest, UserResponse};
use crate::api::middleware::auth_admin::AuthAdmin;
use crate::infra::error::AppError;
use crate::repository::user::UserRepository;
use crate::services;
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use tracing::debug;

pub async fn list_users(
    auth: AuthAdmin,
    State(state): State<AppState>,
) -> Result<Json<Vec<UserResponse>>, AppError> {
    debug!(user = %auth.claims.sub, token_type = "admin", "admin: list_users");
    let users = UserRepository::list(&state.db).await?;
    Ok(Json(users.into_iter().map(UserResponse::from).collect()))
}

pub async fn create_user(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Json(payload): Json<CreateUserRequest>,
) -> Result<Json<UserResponse>, AppError> {
    debug!(user = %auth.claims.sub, token_type = "admin", username = %payload.username, "admin: create_user");
    let user = services::users::create_user(
        &state.db,
        &payload.username,
        &payload.email,
        &payload.display_name,
        &payload.password,
        payload.is_admin.unwrap_or(false),
    )
    .await?;
    Ok(Json(UserResponse::from(user)))
}

pub async fn update_user(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Path(user_id): Path<uuid::Uuid>,
    Json(payload): Json<UpdateUserRequest>,
) -> Result<Json<UserResponse>, AppError> {
    debug!(user = %auth.claims.sub, token_type = "admin", target_user_id = %user_id, "admin: update_user");
    let user = UserRepository::update(
        &state.db,
        user_id,
        payload.display_name.as_deref(),
        payload.is_admin,
    )
    .await?;
    Ok(Json(UserResponse::from(user)))
}

pub async fn delete_user(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Path(user_id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(user = %auth.claims.sub, token_type = "admin", target_user_id = %user_id, "admin: delete_user");
    UserRepository::delete(&state.db, user_id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}
