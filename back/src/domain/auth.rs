use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TokenType {
    User,
    Admin,
    Resolver,
    Federation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    pub sub: String,
    pub uid: Option<Uuid>,
    pub instance: String,
    pub token_type: TokenType,
    pub is_admin: bool,
    pub aud: String,
    pub iss: String,
    pub exp: i64,
    pub iat: i64,
    pub jti: String,
}
