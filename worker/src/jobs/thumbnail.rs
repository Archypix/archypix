//! Handler for `gen_thumbnail` jobs.
//!
//! Sequence: MIME pre-flight → download → EXIF extraction (initial only)
//! → thumbnail generation → upload.
//!
//! Error policy:
//! - Unsupported MIME for thumbnailing  → `WorkerError::UnsupportedFormat` (permanent)
//! - Image codec failure                → `WorkerError::Imaging` (permanent)
//! - EXIF extraction failure            → log and continue (EXIF is optional)
//! - BlurHash failure                   → log and continue (nice-to-have)
//! - Network / upload failure           → propagated `WorkerError::Http` (retriable)

use crate::backend::BackendClient;
use crate::error::{Result, WorkerError};
use crate::imaging::{exif as exif_mod, thumbnailer};
use archypix_common::job::{ExtractedExif, GenThumbnailConfig};
use archypix_common::transfer::{CompleteJobRequest, PresignedWrites};
use tempfile::TempDir;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Handle a `gen_thumbnail` job.
pub async fn handle(
    client: &BackendClient,
    job_id: Uuid,
    config: GenThumbnailConfig,
    presigned_read: Option<String>,
    presigned_writes: PresignedWrites,
    mime_type: Option<String>,
) -> Result<()> {
    let presigned_read = presigned_read.ok_or_else(|| WorkerError::MissingPresignedUrl {
        key: "original".to_string(),
    })?;

    // ── MIME pre-flight (before downloading) ─────────────────────────────────
    if let Some(ref mime) = mime_type {
        if presigned_writes.has_thumbnails() && !archypix_common::mime::supports_thumbnail(mime) {
            return Err(WorkerError::UnsupportedFormat(format!(
                "MIME type '{mime}' is not supported for thumbnail generation"
            )));
        }
        if config.is_initial && !archypix_common::mime::supports_exif(mime) {
            warn!(mime_type = %mime, "MIME type not supported for EXIF extraction; skipping");
        }
    }

    let should_extract_exif = config.is_initial
        && mime_type
            .as_deref()
            .map(archypix_common::mime::supports_exif)
            .unwrap_or(true);

    // ── Download ──────────────────────────────────────────────────────────────
    let tmp = TempDir::new()?;
    let original_path = tmp.path().join("original");

    info!(job_id = %job_id, "gen_thumbnail: downloading original");
    client
        .download_presigned(&presigned_read, &original_path)
        .await?;
    debug!(
        size_bytes = std::fs::metadata(&original_path)
            .map(|m| m.len())
            .unwrap_or(0),
        "gen_thumbnail: original downloaded"
    );

    // ── EXIF extraction (initial jobs only, blocking) ─────────────────────────
    let exif: Option<ExtractedExif> = if should_extract_exif {
        let path = original_path.clone();
        match tokio::task::spawn_blocking(move || exif_mod::extract_exif(&path))
            .await
            .map_err(|e| WorkerError::Imaging(format!("spawn_blocking panicked: {e}")))?
        {
            Ok(e) => {
                debug!("gen_thumbnail: EXIF extracted");
                Some(e)
            }
            Err(e) => {
                warn!(error = ?e, "gen_thumbnail: EXIF extraction failed; continuing without EXIF");
                None
            }
        }
    } else {
        None
    };

    // ── Thumbnails + BlurHash + upload ────────────────────────────────────────
    let thumb = thumbnailer::run(client, &original_path, &presigned_writes, tmp.path()).await?;

    client
        .complete_job(
            job_id,
            CompleteJobRequest {
                exif,
                blurhash: thumb.blurhash,
                thumbnails_generated: thumb.generated,
            },
        )
        .await?;

    info!(job_id = %job_id, thumbnails_generated = thumb.generated, "gen_thumbnail completed");
    Ok(())
}
