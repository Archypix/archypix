use crate::domain::job::{ExtractedExif, JobType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Response to `GET /api/worker/jobs/next`.
/// `None` at the JSON level means no job is currently available.
#[derive(Debug, Serialize)]
pub struct ClaimJobResponse {
    pub job_id: Uuid,
    pub job_type: JobType,
    pub picture_id: Option<Uuid>,
    /// Raw job config (type-specific; see `domain::job` for typed structs).
    pub config: serde_json::Value,
    /// Presigned GET URL for the primary input (the original picture file).
    pub presigned_read: Option<String>,
    /// Presigned PUT URLs for job outputs.
    /// Keys: `"small"`, `"medium"`, `"large"` for `gen_thumbnail`;
    ///       `"output"` for `edit_picture`.
    pub presigned_writes: HashMap<String, String>,
}

/// Body for `POST /api/worker/jobs/{id}/complete`.
#[derive(Debug, Deserialize)]
pub struct CompleteJobRequest {
    /// EXIF data extracted from the image. Required for `gen_thumbnail` with
    /// `is_initial = true`; optional for `edit_picture`.
    pub exif: Option<ExtractedExif>,
    /// BlurHash string for the picture. Only provided for `gen_thumbnail` jobs.
    pub blurhash: Option<String>,
}

/// Body for `POST /api/worker/jobs/{id}/fail`.
#[derive(Debug, Deserialize)]
pub struct FailJobRequest {
    /// Human-readable error description for debugging and display.
    pub error: String,
}
