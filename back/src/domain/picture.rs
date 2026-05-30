use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Picture {
    pub id: Uuid,
    pub local_user_id: Uuid,
    /// Set only for pictures received via federation (not owned by this instance's user).
    pub remote_picture_id: Option<String>,
    pub owner_username: Option<String>,
    pub owner_instance_domain: Option<String>,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub file_size: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub exif_data: Json<serde_json::Value>,
    pub metadata: Json<serde_json::Value>,
    pub deleted_at: Option<NaiveDateTime>,
    pub captured_at: Option<NaiveDateTime>,
    pub ingested_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl Picture {
    pub fn is_owned(&self) -> bool {
        self.remote_picture_id.is_none()
    }
}

/// Transient upload state stored in Redis during the presigned-URL upload window.
#[derive(Debug, Serialize, Deserialize)]
pub struct UploadSession {
    pub user_id: Uuid,
    pub picture_id: Uuid,
    pub s3_key_staging: String,
    pub filename: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PictureVersion {
    pub id: Uuid,
    pub picture_id: Uuid,
    pub version_number: i32,
    pub file_size: Option<i64>,
    pub mime_type: Option<String>,
    pub created_at: NaiveDateTime,
}
