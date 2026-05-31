use crate::domain::auth::{JwtClaims, TokenType};
use crate::infra::error::AppError;
use crate::state::AppState;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use uuid::Uuid;

use super::bearer_token;

#[derive(Clone)]
pub struct AuthUser {
    pub claims: JwtClaims,
}

impl AuthUser {
    pub fn user_id(&self) -> Result<Uuid, AppError> {
        self.claims
            .uid
            .ok_or_else(|| AppError::Unauthorized("Missing user id in token".to_string()))
    }
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer_token(&parts.headers)?;
        let claims = state.jwt.decode(&token, &state.config.back_domain)?;

        if claims.token_type != TokenType::User {
            return Err(AppError::Unauthorized("Invalid token type".to_string()));
        }
        if claims.uid.is_none() {
            return Err(AppError::Unauthorized("Missing user id".to_string()));
        }

        Ok(AuthUser { claims })
    }
}
