use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Response from GET /api/worker/jobs/next.
/// `None` means no job is currently available.
#[derive(Debug, Deserialize)]
pub struct ClaimedJob {
    pub job_id: Uuid,
    pub job_type: String,
    pub picture_id: Option<Uuid>,
    pub config: serde_json::Value,
    /// Presigned GET URL for the original picture.
    pub presigned_read: Option<String>,
    /// Presigned PUT URLs for output files.
    /// Keys: "small", "medium", "large" for gen_thumbnail; "output" for edit_picture.
    pub presigned_writes: HashMap<String, String>,
}

/// EXIF data extracted from a picture by the worker.
/// Fields match the backend's `ExtractedExif` domain type.
#[derive(Debug, Serialize, Default)]
pub struct ExtractedExif {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<i32>,
    /// EXIF datetime in "YYYY:MM:DD HH:MM:SS" format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gps_lat: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gps_lng: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gps_alt: Option<i32>,
    /// EXIF orientation (1-8). `None` means unspecified/unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orientation: Option<i16>,
    /// Remaining structured EXIF fields (camera brand/model, focal length, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exif_data: Option<serde_json::Value>,
}

/// Body for POST /api/worker/jobs/{id}/complete.
#[derive(Debug, Serialize)]
pub struct CompleteJobRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exif: Option<ExtractedExif>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blurhash: Option<String>,
}

/// Body for POST /api/worker/jobs/{id}/fail.
#[derive(Debug, Serialize)]
pub struct FailJobRequest {
    pub error: String,
}
