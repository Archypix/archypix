use crate::database::models::User;
use crate::infrastructure::error::{AppError, map_sqlx_error};
use sqlx::{Executor, PgPool, Postgres};

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
            SELECT id, username, email, display_name, is_admin, created_at, updated_at
            FROM users
            WHERE username = $1
            "#,
            username,
        )
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn find_by_username(pool: &PgPool, username: &str) -> Result<Option<User>, AppError> {
        sqlx::query_as!(
            User,
            r#"
            SELECT id, username, email, display_name, is_admin, created_at, updated_at
            FROM users
            WHERE username = $1
            "#,
            username,
        )
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn find_by_id(pool: &PgPool, user_id: uuid::Uuid) -> Result<Option<User>, AppError> {
        sqlx::query_as!(
            User,
            r#"
            SELECT id, username, email, display_name, is_admin, created_at, updated_at
            FROM users
            WHERE id = $1
            "#,
            user_id,
        )
        .fetch_optional(pool)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn list(pool: &PgPool) -> Result<Vec<User>, AppError> {
        sqlx::query_as!(
            User,
            r#"
            SELECT id, username, email, display_name, is_admin, created_at, updated_at
            FROM users
            ORDER BY created_at DESC
            "#
        )
        .fetch_all(pool)
        .await
        .map_err(map_sqlx_error)
    }

    /// Create a new user
    pub async fn create<'e, E>(
        ex: E,
        username: &str,
        email: &str,
        display_name: &str,
        is_admin: bool,
    ) -> Result<User, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let user = sqlx::query_as!(
            User,
            r#"
            INSERT INTO users (username, email, display_name, is_admin)
            VALUES ($1, $2, $3, $4)
            RETURNING id, username, email, display_name, is_admin, created_at, updated_at
            "#,
            username,
            email,
            display_name,
            is_admin
        )
        .fetch_one(ex)
        .await
        .map_err(map_sqlx_error)?;

        Ok(user)
    }

    pub async fn update(
        pool: &PgPool,
        user_id: uuid::Uuid,
        display_name: Option<&str>,
        is_admin: Option<bool>,
    ) -> Result<User, AppError> {
        sqlx::query_as!(
            User,
            r#"
            UPDATE users
            SET display_name = COALESCE($2, display_name),
                is_admin = COALESCE($3, is_admin)
            WHERE id = $1
            RETURNING id, username, email, display_name, is_admin, created_at, updated_at
            "#,
            user_id,
            display_name,
            is_admin
        )
        .fetch_one(pool)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn update_profile(
        pool: &PgPool,
        user_id: uuid::Uuid,
        display_name: Option<&str>,
        email: Option<&str>,
    ) -> Result<User, AppError> {
        sqlx::query_as!(
            User,
            r#"
            UPDATE users
            SET display_name = COALESCE($2, display_name),
                email = COALESCE($3, email)
            WHERE id = $1
            RETURNING id, username, email, display_name, is_admin, created_at, updated_at
            "#,
            user_id,
            display_name,
            email
        )
        .fetch_one(pool)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn delete(pool: &PgPool, user_id: uuid::Uuid) -> Result<(), AppError> {
        sqlx::query!(
            r#"
            DELETE FROM users WHERE id = $1
            "#,
            user_id
        )
        .execute(pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }
}
