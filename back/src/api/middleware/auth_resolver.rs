use crate::domain::auth::{JwtClaims, TokenType};
use crate::infrastructure::error::AppError;
use crate::infrastructure::state::AppState;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use super::bearer_token;

#[derive(Clone)]
pub struct AuthResolver {
    pub claims: JwtClaims,
}

impl FromRequestParts<AppState> for AuthResolver {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer_token(&parts.headers)?;
        let claims = state
            .resolver_jwt
            .decode_any_issuer(&token, &state.config.host)
            .map_err(|err| AppError::Unauthorized(err.to_string()))?;

        if claims.token_type != TokenType::Resolver {
            return Err(AppError::Unauthorized(
                "Invalid token type for resolver access".to_string(),
            ));
        }

        Ok(AuthResolver { claims })
    }
}
