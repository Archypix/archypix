use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Enums ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(
    feature = "sqlx",
    sqlx(type_name = "job_status", rename_all = "lowercase")
)]
pub enum JobStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

/// All job types supported by the worker fleet.
///
/// Implements `FromStr` / `Display` for human-readable string conversion
/// (e.g. query parameters, logs) and optionally `sqlx::Type` when the
/// `sqlx` feature is enabled (back/ only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(
    feature = "sqlx",
    sqlx(type_name = "job_type", rename_all = "snake_case")
)]
pub enum JobType {
    GenThumbnail,
    MlStyle,
    MlPeople,
    MlGroupLocation,
    EditPicture,
}

impl std::fmt::Display for JobType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::GenThumbnail => "gen_thumbnail",
            Self::MlStyle => "ml_style",
            Self::MlPeople => "ml_people",
            Self::MlGroupLocation => "ml_group_location",
            Self::EditPicture => "edit_picture",
        };
        f.write_str(s)
    }
}

impl std::str::FromStr for JobType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "gen_thumbnail" => Ok(Self::GenThumbnail),
            "ml_style" => Ok(Self::MlStyle),
            "ml_people" => Ok(Self::MlPeople),
            "ml_group_location" => Ok(Self::MlGroupLocation),
            "edit_picture" => Ok(Self::EditPicture),
            other => Err(format!("unknown job type: '{other}'")),
        }
    }
}

// ── Typed job configs ─────────────────────────────────────────────────────────

/// Discriminated union of all job-specific config payloads.
///
/// Stored as JSONB in the database using an internal `"type"` tag, so the
/// discriminant is self-describing and does not need to be inferred from the
/// `job_type` column.
///
/// ```json
/// {"type": "gen_thumbnail", "picture_id": "…", "is_initial": true}
/// {"type": "edit_picture",  "picture_id": "…", "visual": null}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobConfig {
    GenThumbnail(GenThumbnailConfig),
    EditPicture(EditPictureConfig),
    /// ML jobs carry no extra config for now.
    MlStyle,
    MlPeople,
    MlGroupLocation,
}

impl JobConfig {
    /// Returns the `JobType` discriminant that corresponds to this config variant.
    pub fn job_type(&self) -> JobType {
        match self {
            Self::GenThumbnail(_) => JobType::GenThumbnail,
            Self::EditPicture(_) => JobType::EditPicture,
            Self::MlStyle => JobType::MlStyle,
            Self::MlPeople => JobType::MlPeople,
            Self::MlGroupLocation => JobType::MlGroupLocation,
        }
    }
}

/// Config for `gen_thumbnail` jobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenThumbnailConfig {
    pub picture_id: Uuid,
    /// When `true`, this is the first thumbnail generation for this picture:
    /// the worker must also extract and return EXIF metadata.
    pub is_initial: bool,
}

/// Config for `edit_picture` jobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditPictureConfig {
    pub picture_id: Uuid,
    /// Metadata/EXIF fields to override on the picture row.
    /// `None` means no metadata changes.
    pub exif_overrides: Option<ExifOverrides>,
    /// Visual pixel-level transformations to apply to the file.
    /// `None` means no visual edits; the original file is unchanged.
    pub visual: Option<VisualTransformations>,
}

impl EditPictureConfig {
    /// Returns `true` when the job requires the worker to generate new thumbnails
    /// (i.e., visual transforms change the image content).
    pub fn needs_thumbnail_regen(&self) -> bool {
        self.visual.is_some()
    }
}

/// Partial EXIF/metadata override. Only provided fields are written.
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

/// Pixel-level visual transformations to apply to the image file.
///
/// All transforms are optional; at least one must be set for this struct to be
/// useful. The worker applies them in order: crop first, then resize.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualTransformations {
    /// Crop the image to a rectangular region before any other transforms.
    pub crop: Option<CropTransform>,
    /// Resize the (optionally cropped) image to fixed dimensions.
    pub resize: Option<ResizeTransform>,
}

/// Crop region in pixels, measured from the top-left corner of the image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CropTransform {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Target dimensions for a resize operation. The worker preserves aspect ratio
/// by fitting within the given bounds (no distortion).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResizeTransform {
    pub width: u32,
    pub height: u32,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    /// Serialize `value` to JSON, deserialize back, re-serialize, and assert the two JSON
    /// strings are identical. `JobConfig` and friends don't derive `PartialEq`, so comparing
    /// JSON is the most reliable equality check.
    fn round_trip<T: serde::Serialize + serde::de::DeserializeOwned>(value: &T) {
        let json = serde_json::to_string(value).expect("serialize");
        let back: T = serde_json::from_str(&json).expect("deserialize");
        let json2 = serde_json::to_string(&back).expect("re-serialize");
        assert_eq!(json, json2, "round-trip must produce identical JSON");
    }

    #[test]
    fn job_config_gen_thumbnail_roundtrips_json() {
        let cfg = JobConfig::GenThumbnail(GenThumbnailConfig {
            picture_id: Uuid::new_v4(),
            is_initial: true,
        });
        round_trip(&cfg);
    }

    #[test]
    fn job_config_edit_picture_exif_only_roundtrips_json() {
        let cfg = JobConfig::EditPicture(EditPictureConfig {
            picture_id: Uuid::new_v4(),
            exif_overrides: Some(ExifOverrides {
                captured_at: None,
                gps_lat: Some(48.8566),
                gps_lng: Some(2.3522),
                ..Default::default()
            }),
            visual: None,
        });
        round_trip(&cfg);
    }

    #[test]
    fn job_config_edit_picture_visual_roundtrips_json() {
        let cfg = JobConfig::EditPicture(EditPictureConfig {
            picture_id: Uuid::new_v4(),
            exif_overrides: None,
            visual: Some(VisualTransformations {
                crop: Some(CropTransform {
                    x: 10,
                    y: 20,
                    width: 800,
                    height: 600,
                }),
                resize: Some(ResizeTransform {
                    width: 1920,
                    height: 1080,
                }),
            }),
        });
        round_trip(&cfg);
    }

    #[test]
    fn job_config_ml_variants_roundtrip_json() {
        round_trip(&JobConfig::MlStyle);
        round_trip(&JobConfig::MlPeople);
        round_trip(&JobConfig::MlGroupLocation);
    }

    /// The `"type"` discriminant tag must survive a JSON round-trip so the worker
    /// can always deserialize configs stored as JSONB in the database.
    #[test]
    fn job_config_type_tag_is_snake_case() {
        let cfg = JobConfig::GenThumbnail(GenThumbnailConfig {
            picture_id: Uuid::nil(),
            is_initial: true,
        });
        let json = serde_json::to_value(&cfg).unwrap();
        assert_eq!(
            json["type"].as_str().unwrap(),
            "gen_thumbnail",
            "type discriminant must be snake_case"
        );
    }
}

// ── Worker result types ───────────────────────────────────────────────────────

/// EXIF metadata extracted from a picture and returned in the job completion body.
/// The backend merges this into the `pictures` row.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtractedExif {
    pub width: Option<i32>,
    pub height: Option<i32>,
    /// EXIF capture timestamp in `"YYYY:MM:DD HH:MM:SS"` format (or RFC3339).
    pub captured_at: Option<String>,
    pub gps_lat: Option<f64>,
    pub gps_lng: Option<f64>,
    pub gps_alt: Option<i32>,
    /// EXIF orientation tag (1–8). `None` means absent or unknown.
    pub orientation: Option<i16>,
    /// Remaining camera/lens metadata stored as a JSON object:
    /// `camera_brand`, `camera_model`, `focal_length_mm`, `f_number`,
    /// `iso_speed`, `exposure_time_num`, `exposure_time_den`.
    pub exif_data: Option<serde_json::Value>,
}
