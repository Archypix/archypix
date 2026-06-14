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
///
/// The write-through model makes the DB the source of truth: the backend applies the edit to the
/// `pictures` row synchronously at request time and enqueues this job to reconcile the S3 original's
/// embedded EXIF. The config therefore carries an explicit edit delta plus the revert baseline
/// (`ExifEdit::previous`), so a permanent file-write failure can roll the DB back to the old state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditPictureConfig {
    pub picture_id: Uuid,
    /// The EXIF edit delta + revert baseline. `None` for a pure visual job.
    pub exif: Option<ExifEdit>,
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

/// An EXIF edit expressed as a `set`/`clear` delta plus the prior full state.
///
/// - `set`: only `Some` fields are written.
/// - `clear`: fields to delete (column → NULL / JSONB key removed / file tag deleted).
/// - `previous`: the full prior value of every editable field, used by the backend's value-gated
///   revert (§4.3) and completion-time convergence (§5). The worker only reads `set`/`clear`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExifEdit {
    pub set: ExifOverrides,
    #[serde(default)]
    pub clear: Vec<ExifField>,
    pub previous: ExifSnapshot,
}

impl ExifEdit {
    /// The full snapshot the file/DB reaches once this edit's `set`/`clear` is applied to
    /// `previous`. This is the file's content after a successful reconcile.
    pub fn new_state(&self) -> ExifSnapshot {
        self.previous.applied(&self.set, &self.clear)
    }
}

/// One editable EXIF field — the enum form used by `ExifEdit::clear` and the diff machinery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExifField {
    CapturedAt,
    GpsLat,
    GpsLng,
    GpsAlt,
    Orientation,
    CameraBrand,
    CameraModel,
    FocalLengthMm,
    FNumber,
    IsoSpeed,
    ExposureTimeNum,
    ExposureTimeDen,
}

/// Partial EXIF/metadata override. Only provided (`Some`) fields are written.
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

/// A full snapshot of every editable EXIF field. `None` means the field is absent/NULL — unlike
/// [`ExifOverrides`], where `None` means "leave unchanged". Used as the revert baseline and for
/// completion-time convergence diffs.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ExifSnapshot {
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

impl ExifSnapshot {
    /// This snapshot with `set` applied (only `Some` fields overwrite) and `clear` nulled.
    pub fn applied(&self, set: &ExifOverrides, clear: &[ExifField]) -> ExifSnapshot {
        let mut s = self.clone();
        if set.captured_at.is_some() {
            s.captured_at = set.captured_at;
        }
        if set.gps_lat.is_some() {
            s.gps_lat = set.gps_lat;
        }
        if set.gps_lng.is_some() {
            s.gps_lng = set.gps_lng;
        }
        if set.gps_alt.is_some() {
            s.gps_alt = set.gps_alt;
        }
        if set.orientation.is_some() {
            s.orientation = set.orientation;
        }
        if set.camera_brand.is_some() {
            s.camera_brand = set.camera_brand.clone();
        }
        if set.camera_model.is_some() {
            s.camera_model = set.camera_model.clone();
        }
        if set.focal_length_mm.is_some() {
            s.focal_length_mm = set.focal_length_mm;
        }
        if set.f_number.is_some() {
            s.f_number = set.f_number;
        }
        if set.iso_speed.is_some() {
            s.iso_speed = set.iso_speed;
        }
        if set.exposure_time_num.is_some() {
            s.exposure_time_num = set.exposure_time_num;
        }
        if set.exposure_time_den.is_some() {
            s.exposure_time_den = set.exposure_time_den;
        }
        for f in clear {
            s.clear_field(*f);
        }
        s
    }

    fn clear_field(&mut self, f: ExifField) {
        match f {
            ExifField::CapturedAt => self.captured_at = None,
            ExifField::GpsLat => self.gps_lat = None,
            ExifField::GpsLng => self.gps_lng = None,
            ExifField::GpsAlt => self.gps_alt = None,
            ExifField::Orientation => self.orientation = None,
            ExifField::CameraBrand => self.camera_brand = None,
            ExifField::CameraModel => self.camera_model = None,
            ExifField::FocalLengthMm => self.focal_length_mm = None,
            ExifField::FNumber => self.f_number = None,
            ExifField::IsoSpeed => self.iso_speed = None,
            ExifField::ExposureTimeNum => self.exposure_time_num = None,
            ExifField::ExposureTimeDen => self.exposure_time_den = None,
        }
    }

    /// The `set`/`clear` delta that turns `self` into `target`. Empty when they are already equal.
    pub fn diff_to(&self, target: &ExifSnapshot) -> (ExifOverrides, Vec<ExifField>) {
        let mut set = ExifOverrides::default();
        let mut clear = Vec::new();
        macro_rules! diff {
            ($field:ident, $variant:ident) => {
                if self.$field != target.$field {
                    match &target.$field {
                        Some(v) => set.$field = Some(v.clone()),
                        None => clear.push(ExifField::$variant),
                    }
                }
            };
        }
        diff!(captured_at, CapturedAt);
        diff!(gps_lat, GpsLat);
        diff!(gps_lng, GpsLng);
        diff!(gps_alt, GpsAlt);
        diff!(orientation, Orientation);
        diff!(camera_brand, CameraBrand);
        diff!(camera_model, CameraModel);
        diff!(focal_length_mm, FocalLengthMm);
        diff!(f_number, FNumber);
        diff!(iso_speed, IsoSpeed);
        diff!(exposure_time_num, ExposureTimeNum);
        diff!(exposure_time_den, ExposureTimeDen);
        (set, clear)
    }
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
            exif: Some(ExifEdit {
                set: ExifOverrides {
                    gps_lat: Some(48.8566),
                    gps_lng: Some(2.3522),
                    ..Default::default()
                },
                clear: vec![ExifField::GpsAlt, ExifField::Orientation],
                previous: ExifSnapshot {
                    gps_alt: Some(120),
                    orientation: Some(1),
                    ..Default::default()
                },
            }),
            visual: None,
        });
        round_trip(&cfg);
    }

    #[test]
    fn exif_snapshot_applied_and_diff_round_trip() {
        let previous = ExifSnapshot {
            gps_lat: Some(1.0),
            gps_alt: Some(50),
            orientation: Some(1),
            ..Default::default()
        };
        let set = ExifOverrides {
            gps_lat: Some(2.0),
            ..Default::default()
        };
        let clear = vec![ExifField::GpsAlt];
        let new_state = previous.applied(&set, &clear);
        assert_eq!(new_state.gps_lat, Some(2.0));
        assert_eq!(new_state.gps_alt, None);
        assert_eq!(new_state.orientation, Some(1));

        // diff from previous to new_state reproduces the delta.
        let (dset, dclear) = previous.diff_to(&new_state);
        assert_eq!(dset.gps_lat, Some(2.0));
        assert_eq!(dclear, vec![ExifField::GpsAlt]);
        // No-op diff when equal.
        let (empty_set, empty_clear) = new_state.diff_to(&new_state);
        assert!(empty_set.gps_lat.is_none() && empty_clear.is_empty());
    }

    #[test]
    fn job_config_edit_picture_visual_roundtrips_json() {
        let cfg = JobConfig::EditPicture(EditPictureConfig {
            picture_id: Uuid::new_v4(),
            exif: None,
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
