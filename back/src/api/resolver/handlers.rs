use crate::api::middleware::auth_resolver::AuthResolver;
use crate::api::resolver::models::{CreateUserRequest, UserResponse};
use crate::database::auth::CredentialRepository;
use crate::database::user::UserRepository;
use crate::infrastructure::error::AppError;
use crate::infrastructure::state::AppState;
use crate::services::auth::PasswordService;
use axum::Json;
use axum::extract::{Path, State};
use tracing::info;

pub async fn get_user(
    _auth: AuthResolver,
    State(state): State<AppState>,
    Path(username): Path<String>,
) -> Result<Json<UserResponse>, AppError> {
    info!("Resolver fetch user {}", username);
    let user = UserRepository::find_by_username(&state.db, &username).await?;
    let user = user.ok_or(AppError::NotFound)?;

    Ok(Json(UserResponse {
        id: user.id,
        username: user.username,
        email: user.email,
        display_name: user.display_name,
        is_admin: user.is_admin,
    }))
}

pub async fn create_user(
    _auth: AuthResolver,
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
        is_admin: user.is_admin,
    }))
}
