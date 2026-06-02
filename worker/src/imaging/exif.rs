use crate::error::{Result, WorkerError};
use archypix_common::job::{ExifOverrides, ExtractedExif};
use rexiv2::{GpsInfo, Metadata};
use std::path::Path;
use tracing::debug;

/// Load and extract EXIF data from an image file.
///
/// Must be called inside `tokio::task::spawn_blocking` since rexiv2 is synchronous.
pub fn extract_exif(path: &Path) -> Result<ExtractedExif> {
    let metadata = Metadata::new_from_path(path)
        .map_err(|e| WorkerError::Exif(format!("failed to open file for EXIF: {e}")))?;

    let captured_at = extract_first_tag(
        &metadata,
        &[
            "Exif.Photo.DateTimeOriginal",
            "Exif.Photo.DateTimeDigitized",
            "Exif.Image.DateTime",
            "Exif.Image.DateTimeOriginal",
            "Exif.Image.DateTimeDigitized",
        ],
    );

    let gps = metadata.get_gps_info();
    let gps_lat = gps.as_ref().map(|g| g.latitude);
    let gps_lng = gps.as_ref().map(|g| g.longitude);
    // Altitude is only present when the tag exists; lat/lng without altitude is common.
    let gps_alt = gps
        .as_ref()
        .filter(|_| metadata.has_tag("Exif.GPSInfo.GPSAltitude"))
        .map(|g| g.altitude as i32);

    let orientation = match metadata.get_tag_numeric("Exif.Image.Orientation") {
        n @ 1..=8 => Some(n as i16),
        _ => None,
    };

    let width = metadata.get_pixel_width();
    let height = metadata.get_pixel_height();

    // Remaining EXIF fields stored as JSON blob.
    let mut exif_map = serde_json::Map::new();

    if let Ok(brand) = metadata.get_tag_string("Exif.Image.Make") {
        if !brand.is_empty() {
            exif_map.insert("camera_brand".to_string(), serde_json::Value::String(brand));
        }
    }
    if let Ok(model) = metadata.get_tag_string("Exif.Image.Model") {
        if !model.is_empty() {
            exif_map.insert("camera_model".to_string(), serde_json::Value::String(model));
        }
    }
    if let Some(f) = rational_to_f64(metadata.get_tag_rational("Exif.Photo.FocalLengthIn35mmFilm"))
    {
        exif_map.insert("focal_length_mm".to_string(), serde_json::json!(round2(f)));
    }
    if let Some(f) = rational_to_f64(metadata.get_tag_rational("Exif.Photo.FNumber")) {
        exif_map.insert("f_number".to_string(), serde_json::json!(round1(f)));
    }
    if let Some(iso) = extract_iso(&metadata) {
        exif_map.insert("iso_speed".to_string(), serde_json::json!(iso));
    }
    if let Some(et) = metadata.get_tag_rational("Exif.Photo.ExposureTime") {
        exif_map.insert(
            "exposure_time_num".to_string(),
            serde_json::json!(*et.numer()),
        );
        exif_map.insert(
            "exposure_time_den".to_string(),
            serde_json::json!(*et.denom()),
        );
    }

    let exif_data = if exif_map.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(exif_map))
    };

    debug!(
        captured_at = ?captured_at,
        has_gps = gps_lat.is_some(),
        "EXIF extraction complete"
    );

    Ok(ExtractedExif {
        width: if width > 0 { Some(width as i32) } else { None },
        height: if height > 0 {
            Some(height as i32)
        } else {
            None
        },
        captured_at,
        gps_lat,
        gps_lng,
        gps_alt,
        orientation,
        exif_data,
    })
}

/// Write EXIF overrides into the file at `path` (in-place via rexiv2).
///
/// Only fields that are `Some` in `overrides` are written; existing values are
/// left untouched for `None` fields.  Must run inside
/// `tokio::task::spawn_blocking`.
pub fn write_exif_overrides(path: &Path, overrides: &ExifOverrides) -> Result<()> {
    let metadata = Metadata::new_from_path(path)
        .map_err(|e| WorkerError::Exif(format!("failed to open file for EXIF write: {e}")))?;

    if let Some(dt) = overrides.captured_at {
        let s = dt.format("%Y:%m:%d %H:%M:%S").to_string();
        // Write to all three common date/time tags; ignore individual failures
        // (not every format supports every tag).
        for tag in &[
            "Exif.Photo.DateTimeOriginal",
            "Exif.Photo.DateTimeDigitized",
            "Exif.Image.DateTime",
        ] {
            let _ = metadata.set_tag_string(tag, &s);
        }
    }

    if let Some(orientation) = overrides.orientation {
        let _ = metadata.set_tag_numeric("Exif.Image.Orientation", orientation as i32);
    }

    // GPS: only touch GPS tags when at least one coordinate is supplied.
    if overrides.gps_lat.is_some() || overrides.gps_lng.is_some() {
        let gps = GpsInfo {
            longitude: overrides.gps_lng.unwrap_or(0.0),
            latitude: overrides.gps_lat.unwrap_or(0.0),
            altitude: overrides.gps_alt.unwrap_or(0) as f64,
        };
        let _ = metadata.set_gps_info(&gps);
    }

    if let Some(ref brand) = overrides.camera_brand {
        let _ = metadata.set_tag_string("Exif.Image.Make", brand);
    }
    if let Some(ref model) = overrides.camera_model {
        let _ = metadata.set_tag_string("Exif.Image.Model", model);
    }
    if let Some(iso) = overrides.iso_speed {
        let _ = metadata.set_tag_numeric("Exif.Photo.ISOSpeedRatings", iso);
    }

    metadata
        .save_to_file(path)
        .map_err(|e| WorkerError::Exif(format!("failed to save EXIF overrides: {e}")))?;

    debug!(path = %path.display(), "EXIF overrides written");
    Ok(())
}

fn extract_first_tag(metadata: &Metadata, tags: &[&str]) -> Option<String> {
    for tag in tags {
        if let Ok(val) = metadata.get_tag_string(tag) {
            if !val.trim().is_empty() {
                return Some(val);
            }
        }
    }
    None
}

fn extract_iso(metadata: &Metadata) -> Option<i32> {
    for tag in &[
        "Exif.Photo.ISOSpeedRatings",
        "Exif.Photo.PhotographicSensitivity",
        "Xmp.exifEX.PhotographicSensitivity",
    ] {
        let val = metadata.get_tag_numeric(tag);
        if val != 0 {
            return Some(val);
        }
    }
    None
}

fn rational_to_f64(r: Option<num_rational::Ratio<i32>>) -> Option<f64> {
    r.and_then(|r| {
        if *r.denom() == 0 {
            None
        } else {
            Some(*r.numer() as f64 / *r.denom() as f64)
        }
    })
}

fn round1(f: f64) -> f64 {
    (f * 10.0).round() / 10.0
}

fn round2(f: f64) -> f64 {
    (f * 100.0).round() / 100.0
}
