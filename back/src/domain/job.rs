use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "job_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "job_type", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum JobType {
    GenThumbnail,
    MlStyle,
    MlPeople,
    MlGroupLocation,
    EditPicture,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Job {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub job_type: JobType,
    pub status: JobStatus,
    pub config: Json<serde_json::Value>,
    pub result: Option<Json<serde_json::Value>>,
    pub result_s3_keys: Option<Vec<String>>,
    pub error_message: Option<String>,
    pub retry_count: i32,
    pub max_retries: i32,
    pub idempotency_key: Option<String>,
    /// Primary picture for single-picture jobs. NULL for batch jobs.
    pub picture_id: Option<Uuid>,
    /// Worker instance ID while status = 'processing'.
    pub claimed_by: Option<String>,
    pub created_at: NaiveDateTime,
    pub started_at: Option<NaiveDateTime>,
    pub completed_at: Option<NaiveDateTime>,
}

// ── Typed job configs ─────────────────────────────────────────────────────────

/// Config payload for `gen_thumbnail` jobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenThumbnailConfig {
    pub picture_id: Uuid,
    /// When true, this is the first thumbnail run: the worker must also extract
    /// and return EXIF metadata so the backend can populate the picture row.
    pub is_initial: bool,
}

/// Partial EXIF override supplied by the user when editing picture metadata.
/// All fields are optional — only provided fields are applied.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExifOverrides {
    pub captured_at: Option<NaiveDateTime>,
    pub gps_lat: Option<f64>,
    pub gps_lng: Option<f64>,
    pub gps_alt: Option<i32>,
    pub orientation: Option<i16>,
    pub camera_brand: Option<String>,
    pub camera_model: Option<String>,
    pub focal_length_mm: Option<f64>,
    pub f_number: Option<f64>,
    pub iso_speed: Option<i32>,
    pub exposure_time_num: Option<i32>,
    pub exposure_time_den: Option<i32>,
}

/// Config payload for `edit_picture` jobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditPictureConfig {
    pub picture_id: Uuid,
    /// EXIF / metadata fields to override on the picture.
    pub exif_overrides: Option<ExifOverrides>,
    /// When true (or when visual edits are present), the worker should also
    /// regenerate thumbnails after applying the edits.
    pub regenerate_thumbnails: bool,
}

// ── Worker result types ───────────────────────────────────────────────────────

/// EXIF data extracted from a picture by a worker and returned in the completion
/// body. The backend merges this into the `pictures` row.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtractedExif {
    pub width: Option<i32>,
    pub height: Option<i32>,
    /// Captured-at timestamp in "YYYY:MM:DD HH:MM:SS" (EXIF format) or RFC3339.
    pub captured_at: Option<String>,
    pub gps_lat: Option<f64>,
    pub gps_lng: Option<f64>,
    pub gps_alt: Option<i32>,
    pub orientation: Option<i16>,
    /// Remaining EXIF fields (camera brand/model, focal length, f-number, ISO,
    /// exposure time) stored as a JSON object; merged into `pictures.exif_data`.
    pub exif_data: Option<serde_json::Value>,
}
