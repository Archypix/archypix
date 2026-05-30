use crate::domain::user_settings::{UserSettings, VersioningMode};
use crate::infra::error::{AppError, map_sqlx_error};
use sqlx::PgPool;
use uuid::Uuid;

pub struct UserSettingsRepository;

impl UserSettingsRepository {
    /// Get settings for the given user, inserting a default row if not yet present.
    pub async fn get_or_default(db: &PgPool, user_id: Uuid) -> Result<UserSettings, AppError> {
        // Insert defaults if not present, then select
        sqlx::query!(
            "INSERT INTO user_settings (user_id) VALUES ($1) ON CONFLICT (user_id) DO NOTHING",
            user_id
        )
        .execute(db)
        .await
        .map_err(map_sqlx_error)?;

        sqlx::query_as!(
            UserSettings,
            r#"SELECT user_id, versioning_mode as "versioning_mode: VersioningMode", created_at, updated_at
               FROM user_settings
               WHERE user_id = $1"#,
            user_id,
        )
            .fetch_one(db)
            .await
            .map_err(map_sqlx_error)
    }

    pub async fn upsert(
        db: &PgPool,
        user_id: Uuid,
        versioning_mode: VersioningMode,
    ) -> Result<UserSettings, AppError> {
        sqlx::query_as!(
            UserSettings,
            r#"INSERT INTO user_settings (user_id, versioning_mode)
               VALUES ($1, $2)
               ON CONFLICT (user_id) DO UPDATE SET versioning_mode = EXCLUDED.versioning_mode
               RETURNING user_id, versioning_mode as "versioning_mode: VersioningMode", created_at, updated_at"#,
            user_id,
            versioning_mode as VersioningMode,
        )
            .fetch_one(db)
            .await
            .map_err(map_sqlx_error)
    }
}
