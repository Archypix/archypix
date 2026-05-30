use crate::domain::auth::TokenType;
use crate::domain::user::User;
use crate::infra::config::Config;
use crate::infra::crypto::{
    JwtService, generate_refresh_token, hash_refresh_token, verify_password,
};
use crate::infra::error::AppError;
use crate::repository::auth::{CredentialRepository, RefreshTokenRepository};
use crate::repository::user::UserRepository;
use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

pub struct AuthTokens {
    pub access_token: String,
    pub refresh_token: String,
}

pub async fn login(
    db: &PgPool,
    jwt: &JwtService,
    config: &Config,
    username: &str,
    password: &str,
) -> Result<AuthTokens, AppError> {
    let user = UserRepository::find_by_username(db, username)
        .await?
        .ok_or_else(|| AppError::Unauthorized("Invalid credentials".to_string()))?;

    let hash = CredentialRepository::get_password_hash(db, user.id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("Invalid credentials".to_string()))?;

    if !verify_password(password, &hash)? {
        return Err(AppError::Unauthorized("Invalid credentials".to_string()));
    }

    issue_tokens(db, jwt, config, &user).await
}

pub async fn refresh(
    db: &PgPool,
    jwt: &JwtService,
    config: &Config,
    refresh_token_raw: &str,
) -> Result<AuthTokens, AppError> {
    let token_hash = hash_refresh_token(refresh_token_raw);
    let stored = RefreshTokenRepository::find_valid(db, &token_hash)
        .await?
        .ok_or_else(|| AppError::Unauthorized("Invalid refresh token".to_string()))?;

    RefreshTokenRepository::revoke(db, stored.id).await?;

    let user = UserRepository::find_by_id(db, stored.user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("User not found".to_string()))?;

    issue_tokens(db, jwt, config, &user).await
}

pub async fn logout(
    db: &PgPool,
    user_id: Option<Uuid>,
    refresh_token_raw: Option<&str>,
) -> Result<(), AppError> {
    if let Some(raw) = refresh_token_raw {
        let hash = hash_refresh_token(raw);
        if let Some(stored) = RefreshTokenRepository::find_valid(db, &hash).await? {
            RefreshTokenRepository::revoke(db, stored.id).await?;
        }
    } else if let Some(uid) = user_id {
        RefreshTokenRepository::revoke_all_for_user(db, uid).await?;
    }
    Ok(())
}

async fn issue_tokens(
    db: &PgPool,
    jwt: &JwtService,
    config: &Config,
    user: &User,
) -> Result<AuthTokens, AppError> {
    let token_type = if user.is_admin {
        TokenType::Admin
    } else {
        TokenType::User
    };
    let access_token = jwt.issue(
        &user.username,
        Some(user.id),
        &config.host,
        token_type,
        user.is_admin,
        &config.host,
        config.access_token_ttl_secs,
    )?;

    let refresh_token_raw = generate_refresh_token();
    let refresh_hash = hash_refresh_token(&refresh_token_raw);
    let expires_at = Utc::now() + Duration::seconds(config.refresh_token_ttl_secs);
    RefreshTokenRepository::create(db, user.id, &refresh_hash, expires_at).await?;

    Ok(AuthTokens {
        access_token,
        refresh_token: refresh_token_raw,
    })
}
