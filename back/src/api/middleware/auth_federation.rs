use crate::domain::auth::{JwtClaims, TokenType};
use crate::infra::error::AppError;
use crate::state::AppState;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use super::bearer_token;

#[derive(Clone)]
pub struct AuthFederation {
    pub claims: JwtClaims,
}

impl FromRequestParts<AppState> for AuthFederation {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer_token(&parts.headers)?;
        let claims = state.jwt.decode(&token, &state.config.host)?;

        if claims.token_type != TokenType::Federation {
            return Err(AppError::Unauthorized(
                "Invalid token type for federation access".to_string(),
            ));
        }

        Ok(AuthFederation { claims })
    }
}
