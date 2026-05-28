pub mod auth_admin;
pub mod auth_federation;
pub mod auth_resolver;
pub mod auth_user;

use crate::infrastructure::error::AppError;
use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;

pub fn bearer_token(headers: &HeaderMap) -> Result<String, AppError> {
    let header = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("Missing Authorization header".to_string()))?;

    let token = header
        .strip_prefix("Bearer ")
        .ok_or_else(|| AppError::Unauthorized("Invalid Authorization header".to_string()))?;

    if token.trim().is_empty() {
        return Err(AppError::Unauthorized("Empty bearer token".to_string()));
    }

    Ok(token.to_string())
}
