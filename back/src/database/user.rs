use crate::database::models::User;
use crate::infrastructure::error::AppError;
use sqlx::PgPool;

/// User-related database operations (Repository pattern)
pub struct UserRepository;

impl UserRepository {
    /// Find a user by username and instance domain
    pub async fn find_by_username_and_instance(
        pool: &PgPool,
        username: &str,
    ) -> Result<Option<User>, AppError> {
        sqlx::query_as!(
            User,
            r#"
            SELECT id, username, email, display_name, created_at, updated_at
            FROM users
            WHERE username = $1
            "#,
            username,
        )
        .fetch_optional(pool)
        .await
        .map_err(Into::<AppError>::into)
    }

    /// Create a new user
    pub async fn create(
        pool: &PgPool,
        username: &str,
        email: &str,
        display_name: &str,
    ) -> anyhow::Result<User> {
        let user = sqlx::query_as!(
            User,
            r#"
            INSERT INTO users (username, email, display_name)
            VALUES ($1, $2, $3)
            RETURNING id, username, email, display_name, created_at, updated_at
            "#,
            username,
            email,
            display_name
        )
        .fetch_one(pool)
        .await
        .map_err(Into::<AppError>::into)?;

        Ok(user)
    }
}
