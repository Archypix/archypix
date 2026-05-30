use crate::api::middleware::auth_user::AuthUser;
use crate::domain::user_settings::{UserSettings, VersioningMode};
use crate::infra::error::AppError;
use crate::services;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use serde::Deserialize;

pub async fn get_settings(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<UserSettings>, AppError> {
    let settings = services::user_settings::get(&state.db, auth.user_id()?).await?;
    Ok(Json(settings))
}

#[derive(Debug, Deserialize)]
pub struct UpdateSettingsBody {
    pub versioning_mode: VersioningMode,
}

pub async fn update_settings(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<UpdateSettingsBody>,
) -> Result<Json<UserSettings>, AppError> {
    let settings =
        services::user_settings::update(&state.db, auth.user_id()?, body.versioning_mode).await?;
    Ok(Json(settings))
}
