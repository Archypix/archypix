use crate::api::middleware::auth_user::AuthUser;
use crate::database::auth::{CredentialRepository, RefreshTokenRepository};
use crate::database::user::UserRepository;
use crate::domain::auth::TokenType;
use crate::infrastructure::error::AppError;
use crate::infrastructure::state::AppState;
use crate::services::auth::{PasswordService, RefreshTokenService};
use axum::Json;
use axum::extract::State;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Debug, Serialize)]
pub struct RefreshResponse {
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Debug, Deserialize)]
pub struct LogoutRequest {
    pub refresh_token: Option<String>,
}

pub async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    let user = UserRepository::find_by_username(&state.db, &payload.username)
        .await?
        .ok_or_else(|| AppError::Unauthorized("Invalid credentials".to_string()))?;

    let password_hash = CredentialRepository::get_password_hash(&state.db, user.id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("Invalid credentials".to_string()))?;

    let valid = PasswordService::verify_password(&payload.password, &password_hash)?;
    if !valid {
        return Err(AppError::Unauthorized("Invalid credentials".to_string()));
    }

    let access_token = state
        .jwt
        .issue(
            &user.username,
            Some(user.id),
            &state.config.host,
            if user.is_admin {
                TokenType::Admin
            } else {
                TokenType::User
            },
            user.is_admin,
            &state.config.host,
            state.config.access_token_ttl_secs,
        )
        .map_err(|err| AppError::InternalServerError(err.to_string()))?;

    let refresh_token = RefreshTokenService::generate_refresh_token();
    let refresh_hash = RefreshTokenService::hash_refresh_token(&refresh_token);
    let expires_at = Utc::now() + Duration::seconds(state.config.refresh_token_ttl_secs);
    RefreshTokenRepository::create(&state.db, user.id, &refresh_hash, expires_at).await?;

    Ok(Json(LoginResponse {
        access_token,
        refresh_token,
    }))
}

pub async fn refresh(
    State(state): State<AppState>,
    Json(payload): Json<RefreshRequest>,
) -> Result<Json<RefreshResponse>, AppError> {
    let refresh_hash = RefreshTokenService::hash_refresh_token(&payload.refresh_token);
    let token = RefreshTokenRepository::find_valid(&state.db, &refresh_hash)
        .await?
        .ok_or_else(|| AppError::Unauthorized("Invalid refresh token".to_string()))?;

    RefreshTokenRepository::revoke(&state.db, token.id).await?;

    let user = UserRepository::find_by_id(&state.db, token.user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("Invalid refresh token".to_string()))?;

    let access_token = state
        .jwt
        .issue(
            &user.username,
            Some(user.id),
            &state.config.host,
            if user.is_admin {
                TokenType::Admin
            } else {
                TokenType::User
            },
            user.is_admin,
            &state.config.host,
            state.config.access_token_ttl_secs,
        )
        .map_err(|err| AppError::InternalServerError(err.to_string()))?;

    let new_refresh_token = RefreshTokenService::generate_refresh_token();
    let new_refresh_hash = RefreshTokenService::hash_refresh_token(&new_refresh_token);
    let expires_at = Utc::now() + Duration::seconds(state.config.refresh_token_ttl_secs);
    RefreshTokenRepository::create(&state.db, user.id, &new_refresh_hash, expires_at).await?;

    Ok(Json(RefreshResponse {
        access_token,
        refresh_token: new_refresh_token,
    }))
}

pub async fn logout(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<LogoutRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if let Some(token) = payload.refresh_token {
        let refresh_hash = RefreshTokenService::hash_refresh_token(&token);
        if let Some(existing) = RefreshTokenRepository::find_valid(&state.db, &refresh_hash).await?
        {
            RefreshTokenRepository::revoke(&state.db, existing.id).await?;
        }
    } else if let Some(user_id) = auth.claims.uid {
        RefreshTokenRepository::revoke_all_for_user(&state.db, user_id).await?;
    }

    Ok(Json(serde_json::json!({ "logged_out": true })))
}

pub async fn me(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = auth
        .claims
        .uid
        .ok_or_else(|| AppError::Unauthorized("Missing user id".to_string()))?;
    let user = UserRepository::find_by_id(&state.db, user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("User not found".to_string()))?;

    Ok(Json(serde_json::json!({
        "id": user.id,
        "username": user.username,
        "email": user.email,
        "display_name": user.display_name,
        "is_admin": user.is_admin
    })))
}
