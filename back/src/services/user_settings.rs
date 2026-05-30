use crate::domain::user_settings::{UserSettings, VersioningMode};
use crate::infra::error::AppError;
use crate::repository::user_settings::UserSettingsRepository;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn get(db: &PgPool, user_id: Uuid) -> Result<UserSettings, AppError> {
    UserSettingsRepository::get_or_default(db, user_id).await
}

pub async fn update(
    db: &PgPool,
    user_id: Uuid,
    versioning_mode: VersioningMode,
) -> Result<UserSettings, AppError> {
    UserSettingsRepository::upsert(db, user_id, versioning_mode).await
}
