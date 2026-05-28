use crate::database::models::{RefreshToken, UserCredential};
use crate::infrastructure::error::{AppError, map_sqlx_error};
use sqlx::PgPool;
use uuid::Uuid;

pub struct CredentialRepository;

impl CredentialRepository {
    pub async fn get_password_hash(
        pool: &PgPool,
        user_id: Uuid,
    ) -> Result<Option<String>, AppError> {
        sqlx::query_scalar!(
            r#"
            SELECT password_hash
            FROM user_credentials
            WHERE user_id = $1
            "#,
            user_id
        )
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn upsert_password(
        pool: &PgPool,
        user_id: Uuid,
        password_hash: &str,
    ) -> Result<UserCredential, AppError> {
        sqlx::query_as!(
            UserCredential,
            r#"
            INSERT INTO user_credentials (user_id, password_hash)
            VALUES ($1, $2)
            ON CONFLICT (user_id)
            DO UPDATE SET password_hash = $2
            RETURNING user_id, password_hash, created_at, updated_at
            "#,
            user_id,
            password_hash
        )
        .fetch_one(pool)
        .await
        .map_err(map_sqlx_error)
    }
}

pub struct RefreshTokenRepository;

impl RefreshTokenRepository {
    pub async fn create(
        pool: &PgPool,
        user_id: Uuid,
        token_hash: &str,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<RefreshToken, AppError> {
        let expires_at = expires_at.naive_utc();

        sqlx::query_as!(
            RefreshToken,
            r#"
            INSERT INTO refresh_tokens (user_id, token_hash, expires_at)
            VALUES ($1, $2, $3)
            RETURNING id, user_id, token_hash, expires_at, revoked_at, created_at, updated_at
            "#,
            user_id,
            token_hash,
            expires_at
        )
        .fetch_one(pool)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn find_valid(
        pool: &PgPool,
        token_hash: &str,
    ) -> Result<Option<RefreshToken>, AppError> {
        sqlx::query_as!(
            RefreshToken,
            r#"
            SELECT id, user_id, token_hash, expires_at, revoked_at, created_at, updated_at
            FROM refresh_tokens
            WHERE token_hash = $1
              AND revoked_at IS NULL
              AND expires_at > (now() at time zone 'utc')
            "#,
            token_hash
        )
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn revoke(pool: &PgPool, token_id: Uuid) -> Result<(), AppError> {
        sqlx::query!(
            r#"
            UPDATE refresh_tokens
            SET revoked_at = (now() at time zone 'utc')
            WHERE id = $1
            "#,
            token_id
        )
        .execute(pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    pub async fn revoke_all_for_user(pool: &PgPool, user_id: Uuid) -> Result<(), AppError> {
        sqlx::query!(
            r#"
            UPDATE refresh_tokens
            SET revoked_at = (now() at time zone 'utc')
            WHERE user_id = $1 AND revoked_at IS NULL
            "#,
            user_id
        )
        .execute(pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }
}
