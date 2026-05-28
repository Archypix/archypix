use crate::domain::auth::{JwtClaims, TokenType};
use crate::infrastructure::error::AppError;
use crate::infrastructure::state::AppState;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use super::bearer_token;

#[derive(Clone)]
pub struct AuthUser {
    pub claims: JwtClaims,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer_token(&parts.headers)?;
        let claims = state.jwt.decode(&token, &state.config.host)?;

        match claims.token_type {
            TokenType::User | TokenType::Admin => {}
            _ => {
                return Err(AppError::Unauthorized(
                    "Invalid token type for user access".to_string(),
                ));
            }
        }

        if claims.uid.is_none() {
            return Err(AppError::Unauthorized("Missing user id".to_string()));
        }

        Ok(AuthUser { claims })
    }
}
