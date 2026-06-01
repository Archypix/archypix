use crate::config::Config;
use crate::error::{Result, WorkerError};
use chrono::Utc;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// JWT claims structure matching the backend's `JwtClaims`.
#[derive(Debug, Serialize, Deserialize)]
struct WorkerClaims {
    sub: String,
    uid: Option<Uuid>,
    is_admin: bool,
    instance: String,
    token_type: String,
    aud: String,
    iss: String,
    exp: i64,
    iat: i64,
    jti: String,
}

/// Generate a fresh worker JWT valid for 300 seconds.
pub fn generate_token(config: &Config) -> Result<String> {
    let now = Utc::now().timestamp();
    let claims = WorkerClaims {
        sub: config.worker_id.clone(),
        uid: None,
        is_admin: false,
        instance: config.global_domain.clone(),
        token_type: "worker".to_string(),
        aud: config.back_domain.clone(),
        iss: config.worker_id.clone(),
        exp: now + 300,
        iat: now,
        jti: Uuid::new_v4().to_string(),
    };
    let key = EncodingKey::from_secret(config.worker_jwt_secret.as_bytes());
    encode(&Header::new(Algorithm::HS256), &claims, &key)
        .map_err(|e| WorkerError::Jwt(e.to_string()))
}
