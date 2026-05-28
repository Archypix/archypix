use crate::api::middleware::auth_user::AuthUser;
use crate::database::auth::CredentialRepository;
use crate::database::user::UserRepository;
use crate::infrastructure::error::{AppError, map_sqlx_error};
use crate::infrastructure::state::AppState;
use crate::services::auth::PasswordService;
use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
    pub display_name: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateMeRequest {
    pub display_name: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: uuid::Uuid,
    pub username: String,
    pub email: String,
    pub display_name: String,
}

pub async fn register_public(
    State(state): State<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> Result<Json<UserResponse>, AppError> {
    if state.config.use_resolver {
        return Err(AppError::BadRequest(
            "Registration is handled by resolver".to_string(),
        ));
    }

    if payload.password.trim().is_empty() {
        return Err(AppError::BadRequest("Password is required".to_string()));
    }

    let mut tx = state.db.begin().await.map_err(map_sqlx_error)?;

    let user = UserRepository::create(
        &mut *tx,
        &payload.username,
        &payload.email,
        &payload.display_name,
        false,
    )
    .await?;

    let password_hash = PasswordService::hash_password(&payload.password)?;
    CredentialRepository::upsert_password(&state.db, user.id, &password_hash).await?;

    Ok(Json(UserResponse {
        id: user.id,
        username: user.username,
        email: user.email,
        display_name: user.display_name,
    }))
}

pub async fn get_public_user(
    State(state): State<AppState>,
    Path(username): Path<String>,
) -> Result<Json<UserResponse>, AppError> {
    let user = UserRepository::find_by_username(&state.db, &username)
        .await?
        .ok_or(AppError::NotFound)?;

    Ok(Json(UserResponse {
        id: user.id,
        username: user.username,
        email: user.email,
        display_name: user.display_name,
    }))
}

pub async fn update_me(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<UpdateMeRequest>,
) -> Result<Json<UserResponse>, AppError> {
    let user_id = auth
        .claims
        .uid
        .ok_or_else(|| AppError::Unauthorized("Missing user id".to_string()))?;

    let user = UserRepository::update_profile(
        &state.db,
        user_id,
        payload.display_name.as_deref(),
        payload.email.as_deref(),
    )
    .await?;

    Ok(Json(UserResponse {
        id: user.id,
        username: user.username,
        email: user.email,
        display_name: user.display_name,
    }))
}
