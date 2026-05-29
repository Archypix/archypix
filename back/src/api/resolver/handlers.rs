use crate::api::middleware::auth_resolver::AuthResolver;
use crate::api::resolver::models::{CreateUserRequest, UserResponse};
use crate::database::user::UserRepository;
use crate::infrastructure::error::AppError;
use crate::infrastructure::state::AppState;
use crate::services::users::UserAccountService;
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
    let user = UserAccountService::create_user_with_password(
        &state.db,
        &payload.username,
        &payload.email,
        &payload.display_name,
        &payload.password,
        false,
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
