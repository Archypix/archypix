use crate::backend::BackendClient;
use crate::error::{Result, WorkerError};
use crate::imaging::{exif as exif_mod, thumbnailer};
use archypix_common::job::EditPictureConfig;
use archypix_common::transfer::{CompleteJobRequest, PresignedWrites};
use tempfile::TempDir;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Handle an `edit_picture` job.
///
/// Processing order:
/// 1. Download the original file.
/// 2. If `exif_overrides` is set: write them into the file's embedded EXIF.
/// 3. If `visual` transforms are set: apply them (crop/resize — **not yet
///    implemented**; thumbnails are regenerated from the current file as a
///    best-effort fallback).
/// 4. Upload the (possibly modified) file to the `output` presigned URL so the
///    stored original always reflects the latest edits.
/// 5. If the backend provided thumbnail presigned URLs (visual edit): regenerate
///    and upload all three variants plus a new BlurHash.
///
/// Note: the backend independently applies `exif_overrides` to the picture row
/// columns at completion time, so the DB and the stored file stay in sync.
pub async fn handle(
    client: &BackendClient,
    job_id: Uuid,
    config: EditPictureConfig,
    presigned_read: Option<String>,
    presigned_writes: PresignedWrites,
    _mime_type: Option<String>,
) -> Result<()> {
    let presigned_read = presigned_read.ok_or_else(|| WorkerError::MissingPresignedUrl {
        key: "original".to_string(),
    })?;
    let output_url = presigned_writes
        .output
        .as_deref()
        .ok_or_else(|| WorkerError::MissingPresignedUrl {
            key: "output".to_string(),
        })?
        .to_string();

    // ── Download ──────────────────────────────────────────────────────────────
    let tmp = TempDir::new()?;
    let file_path = tmp.path().join("original");

    info!(job_id = %job_id, "edit_picture: downloading original");
    client
        .download_presigned(&presigned_read, &file_path)
        .await?;
    debug!(
        size_bytes = std::fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0),
        "edit_picture: original downloaded"
    );

    // ── Write EXIF overrides into the file ───────────────────────────────────
    if let Some(ref overrides) = config.exif_overrides {
        let path = file_path.clone();
        let overrides = overrides.clone();
        tokio::task::spawn_blocking(move || exif_mod::write_exif_overrides(&path, &overrides))
            .await
            .map_err(|e| WorkerError::Imaging(format!("spawn_blocking panicked: {e}")))?
            .map_err(|e| {
                // EXIF write failures are permanent — the file format may not
                // support embedded metadata (e.g. some raw formats).
                warn!(error = ?e, job_id = %job_id, "EXIF write failed; uploading file as-is");
                e
            })
            // Non-fatal: log and continue so the file is still uploaded.
            .unwrap_or(());
    }

    // ── Visual transforms ────────────────────────────────────────────────────
    // TODO: implement crop / resize once the imaging primitives are ready.
    // The output presigned URL is already available; the transformed file just
    // needs to be written to `file_path` before the upload below.
    if config.visual.is_some() {
        warn!(job_id = %job_id, "visual transforms not yet implemented; uploading original");
    }

    // ── Upload modified original ─────────────────────────────────────────────
    info!(job_id = %job_id, "edit_picture: uploading modified original");
    client.upload_presigned(&output_url, &file_path).await?;

    // ── Regenerate thumbnails (visual edits only) ────────────────────────────
    let thumb = thumbnailer::run(client, &file_path, &presigned_writes, tmp.path()).await?;

    client
        .complete_job(
            job_id,
            CompleteJobRequest {
                // EXIF overrides are applied server-side from the job config;
                // we do not re-extract them here.
                exif: None,
                blurhash: thumb.blurhash,
                thumbnails_generated: thumb.generated,
            },
        )
        .await?;

    info!(
        job_id = %job_id,
        thumbnails_regenerated = thumb.generated,
        "edit_picture completed"
    );
    Ok(())
}
