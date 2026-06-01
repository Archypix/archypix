use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TokenType {
    User,
    Resolver,
    Federation,
    Worker,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    /// Subject: username for user tokens; global (WebFinger) domain for federation tokens.
    pub sub: String,
    /// User UUID — present for user tokens, absent for federation/resolver tokens.
    pub uid: Option<Uuid>,
    pub is_admin: bool,
    /// Global (WebFinger) domain of the issuing instance (e.g. `example.com`).
    /// Never the backend domain — that is resolved at request time via WebFinger.
    pub instance: String,
    pub token_type: TokenType,
    /// Audience: backend domain of the verifying instance (matched against `HOST` on decode).
    pub aud: String,
    /// Issuer: backend domain of the signing instance.
    pub iss: String,
    /// Expiry timestamp (Unix seconds).
    pub exp: i64,
    /// Issued-at timestamp (Unix seconds).
    pub iat: i64,
    /// Unique token ID — used for replay protection.
    pub jti: String,
}
