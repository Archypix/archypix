use crate::AppState;
use crate::database::models::User;
use crate::database::user::UserRepository;
use crate::infrastructure::error::AppError;
use axum::Json;
use axum::extract::{Path, State};
use tracing::info;

/// Get a user by username
pub async fn get_user(
    State(state): State<AppState>,
    Path(username): Path<String>,
) -> Result<Json<User>, AppError> {
    info!(
        "Fetching user: @{}:{}",
        username, state.config.webfinger_host
    );

    let user = UserRepository::find_by_username_and_instance(&state.db, &username).await?;

    if let Some(user) = user {
        Ok(Json(user))
    } else {
        Err(AppError::NotFound)
    }
}

#[derive(serde::Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub email: String,
    pub display_name: String,
}

/// Create a new user
pub async fn create_user(
    State(state): State<AppState>,
    Json(payload): Json<CreateUserRequest>,
) -> Result<Json<User>, AppError> {
    info!(
        "Creating user: @{}:{}",
        payload.username, state.config.webfinger_host
    );

    let user = UserRepository::create(
        &state.db,
        &payload.username,
        &payload.email,
        &payload.display_name,
    )
    .await?;

    Ok(Json(user))
}
