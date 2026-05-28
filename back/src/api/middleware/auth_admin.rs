use crate::domain::auth::{JwtClaims, TokenType};
use crate::infrastructure::error::AppError;
use crate::infrastructure::state::AppState;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use super::bearer_token;

#[derive(Clone)]
pub struct AuthAdmin {
    pub claims: JwtClaims,
}

impl FromRequestParts<AppState> for AuthAdmin {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer_token(&parts.headers)?;
        let claims = state.jwt.decode(&token, &state.config.host)?;

        match claims.token_type {
            TokenType::Admin | TokenType::User => {}
            _ => {
                return Err(AppError::Unauthorized(
                    "Invalid token type for admin access".to_string(),
                ));
            }
        }

        if !claims.is_admin {
            return Err(AppError::Unauthorized("Admin access required".to_string()));
        }

        Ok(AuthAdmin { claims })
    }
}
