use crate::domain::auth::{JwtClaims, TokenType};
use crate::infrastructure::error::AppError;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use chrono::Utc;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use rand::Rng;
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Clone)]
pub struct JwtService {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    issuer: String,
}

impl JwtService {
    pub fn new(secret: &str, issuer: &str) -> Self {
        Self {
            encoding_key: EncodingKey::from_secret(secret.as_bytes()),
            decoding_key: DecodingKey::from_secret(secret.as_bytes()),
            issuer: issuer.to_string(),
        }
    }

    pub fn issue(
        &self,
        subject: &str,
        uid: Option<Uuid>,
        instance: &str,
        token_type: TokenType,
        is_admin: bool,
        audience: &str,
        ttl_secs: i64,
    ) -> anyhow::Result<String> {
        let now = Utc::now().timestamp();
        let claims = JwtClaims {
            sub: subject.to_string(),
            uid,
            instance: instance.to_string(),
            token_type,
            is_admin,
            aud: audience.to_string(),
            iss: self.issuer.clone(),
            exp: now + ttl_secs,
            iat: now,
            jti: Uuid::new_v4().to_string(),
        };
        let header = Header::new(Algorithm::HS256);
        Ok(encode(&header, &claims, &self.encoding_key)?)
    }

    pub fn decode(&self, token: &str, audience: &str) -> Result<JwtClaims, AppError> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_audience(&[audience]);
        validation.set_issuer(&[self.issuer.clone()]);
        let data = decode::<JwtClaims>(token, &self.decoding_key, &validation)
            .map_err(|err| AppError::Unauthorized(err.to_string()))?;
        Ok(data.claims)
    }

    pub fn decode_any_issuer(&self, token: &str, audience: &str) -> Result<JwtClaims, AppError> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_audience(&[audience]);
        let data = decode::<JwtClaims>(token, &self.decoding_key, &validation)
            .map_err(|err| AppError::Unauthorized(err.to_string()))?;
        Ok(data.claims)
    }
}

pub struct PasswordService;

impl PasswordService {
    pub fn hash_password(password: &str) -> Result<String, AppError> {
        let salt =
            argon2::password_hash::SaltString::generate(argon2::password_hash::rand_core::OsRng);
        let argon2 = Argon2::default();
        let hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|err| AppError::InternalServerError(err.to_string()))?
            .to_string();
        Ok(hash)
    }

    pub fn verify_password(password: &str, hash: &str) -> Result<bool, AppError> {
        let parsed_hash = PasswordHash::new(hash)
            .map_err(|err| AppError::InternalServerError(err.to_string()))?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok())
    }
}

pub struct RefreshTokenService;

impl RefreshTokenService {
    pub fn generate_refresh_token() -> String {
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        hex::encode(bytes)
    }

    pub fn hash_refresh_token(token: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        hex::encode(hasher.finalize())
    }
}
