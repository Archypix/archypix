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
    pub s3_key_original: String,
    pub s3_key_small: Option<String>,
    pub s3_key_medium: Option<String>,
    pub s3_key_large: Option<String>,
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

    pub fn owner_identity(&self) -> Option<(&str, &str)> {
        match (&self.owner_username, &self.owner_instance_domain) {
            (Some(u), Some(i)) => Some((u.as_str(), i.as_str())),
            _ => None,
        }
    }
}

/// Transient upload state stored in Redis during the presigned-URL upload window.
#[derive(Debug, Serialize, Deserialize)]
pub struct UploadSession {
    pub user_id: Uuid,
    pub s3_key_staging: String,
    pub filename: String,
}
