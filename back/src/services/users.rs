use crate::domain::user::User;
use crate::infra::crypto::hash_password;
use crate::infra::error::{AppError, map_sqlx_error};
use crate::repository::auth::CredentialRepository;
use crate::repository::user::UserRepository;
use sqlx::PgPool;

pub async fn create_user(
    db: &PgPool,
    username: &str,
    email: &str,
    display_name: &str,
    password: &str,
    is_admin: bool,
) -> Result<User, AppError> {
    if password.trim().is_empty() {
        return Err(AppError::BadRequest("Password cannot be empty".to_string()));
    }

    let password_hash = hash_password(password)?;

    let mut tx = db.begin().await.map_err(map_sqlx_error)?;
    let user = UserRepository::create(&mut *tx, username, email, display_name, is_admin).await?;
    CredentialRepository::upsert_password(&mut *tx, user.id, &password_hash).await?;
    tx.commit().await.map_err(map_sqlx_error)?;

    Ok(user)
}
