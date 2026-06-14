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
    pub blurhash: Option<String>,
    pub gps_lat: Option<f64>,
    pub gps_lng: Option<f64>,
    pub gps_alt: Option<i32>,
    pub orientation: Option<i16>,
    pub thumbnails_generated_at: Option<NaiveDateTime>,
    /// SHA-256 hex digest of the stored file. Used as WebDAV ETag.
    pub file_hash: Option<String>,
    /// Convergence of the S3 original's embedded EXIF versus this row (the source of truth).
    pub exif_sync_status: ExifSyncStatus,
}

/// Convergence state of a picture's embedded-file EXIF versus the DB row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "picture_exif_sync_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ExifSyncStatus {
    Synced,
    Pending,
    Unsupported,
}

impl Picture {
    /// Capture the picture's current editable-EXIF values as a full snapshot — the revert baseline
    /// for an edit. JSONB camera/lens fields are read from `exif_data`.
    pub fn exif_snapshot(&self) -> crate::domain::job::ExifSnapshot {
        use crate::domain::job::ExifSnapshot;
        let e = &self.exif_data.0;
        let s = |k: &str| e.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());
        let f = |k: &str| e.get(k).and_then(|v| v.as_f64());
        let i = |k: &str| e.get(k).and_then(|v| v.as_i64()).map(|n| n as i32);
        ExifSnapshot {
            captured_at: self.captured_at,
            gps_lat: self.gps_lat,
            gps_lng: self.gps_lng,
            gps_alt: self.gps_alt,
            orientation: self.orientation,
            camera_brand: s("camera_brand"),
            camera_model: s("camera_model"),
            focal_length_mm: f("focal_length_mm"),
            f_number: f("f_number"),
            iso_speed: i("iso_speed"),
            exposure_time_num: i("exposure_time_num"),
            exposure_time_den: i("exposure_time_den"),
        }
    }
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
