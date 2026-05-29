use crate::database::auth::CredentialRepository;
use crate::database::models::User;
use crate::database::user::UserRepository;
use crate::infrastructure::error::{AppError, map_sqlx_error};
use crate::services::auth::PasswordService;
use sqlx::PgPool;

pub struct UserAccountService;

impl UserAccountService {
    pub async fn create_user_with_password(
        pool: &PgPool,
        username: &str,
        email: &str,
        display_name: &str,
        password: &str,
        is_admin: bool,
    ) -> Result<User, AppError> {
        if password.trim().is_empty() {
            return Err(AppError::BadRequest("Password is required".to_string()));
        }

        let mut tx = pool.begin().await.map_err(map_sqlx_error)?;
        let user =
            UserRepository::create(&mut *tx, username, email, display_name, is_admin).await?;

        let password_hash = PasswordService::hash_password(password)?;
        CredentialRepository::upsert_password(&mut *tx, user.id, &password_hash).await?;
        tx.commit().await.map_err(map_sqlx_error)?;

        Ok(user)
    }
}
