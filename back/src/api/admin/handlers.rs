use crate::api::admin::models::{CreateUserRequest, UpdateUserRequest, UserResponse};
use crate::api::middleware::auth_admin::AuthAdmin;
use crate::database::auth::CredentialRepository;
use crate::database::user::UserRepository;
use crate::infrastructure::error::AppError;
use crate::infrastructure::state::AppState;
use crate::services::auth::PasswordService;
use axum::Json;
use axum::extract::{Path, State};

pub async fn list_users(
    _auth: AuthAdmin,
    State(state): State<AppState>,
) -> Result<Json<Vec<UserResponse>>, AppError> {
    let users = UserRepository::list(&state.db).await?;
    let response = users
        .into_iter()
        .map(|user| UserResponse {
            id: user.id,
            username: user.username,
            email: user.email,
            display_name: user.display_name,
            is_admin: user.is_admin,
        })
        .collect();
    Ok(Json(response))
}

pub async fn create_user(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Json(payload): Json<CreateUserRequest>,
) -> Result<Json<UserResponse>, AppError> {
    if payload.password.trim().is_empty() {
        return Err(AppError::BadRequest("Password is required".to_string()));
    }

    let user = UserRepository::create(
        &state.db,
        &payload.username,
        &payload.email,
        &payload.display_name,
        payload.is_admin.unwrap_or(false),
    )
    .await?;

    let password_hash = PasswordService::hash_password(&payload.password)?;
    CredentialRepository::upsert_password(&state.db, user.id, &password_hash).await?;

    Ok(Json(UserResponse {
        id: user.id,
        username: user.username,
        email: user.email,
        display_name: user.display_name,
        is_admin: user.is_admin,
    }))
}

pub async fn update_user(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(user_id): Path<uuid::Uuid>,
    Json(payload): Json<UpdateUserRequest>,
) -> Result<Json<UserResponse>, AppError> {
    let user = UserRepository::update(
        &state.db,
        user_id,
        payload.display_name.as_deref(),
        payload.is_admin,
    )
    .await?;

    Ok(Json(UserResponse {
        id: user.id,
        username: user.username,
        email: user.email,
        display_name: user.display_name,
        is_admin: user.is_admin,
    }))
}

pub async fn delete_user(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(user_id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    UserRepository::delete(&state.db, user_id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}
