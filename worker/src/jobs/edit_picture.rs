use crate::backend::BackendClient;
use crate::error::{Result, WorkerError};
use crate::imaging::{exif as exif_mod, hash as hash_mod, thumbnailer};
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
///    A write failure is a permanent error — the file format may not support
///    embedded metadata (e.g. some raw formats). MIME screening will be added
///    server-side to prevent creating such jobs in the first place.
/// 3. Compute file_size and file_hash from the (possibly modified) file.
/// 4. Upload the file to the `output` presigned URL.
/// 5. If the backend provided thumbnail presigned URLs (visual edit): regenerate
///    and upload all three variants plus a new BlurHash.
pub async fn handle(
    client: &BackendClient,
    job_id: Uuid,
    claim_token: Uuid,
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
            .map_err(|e| WorkerError::Imaging(format!("spawn_blocking panicked: {e}")))??;
    }

    // ── Visual transforms ────────────────────────────────────────────────────
    // TODO: implement crop / resize once the imaging primitives are ready.
    if config.visual.is_some() {
        warn!(job_id = %job_id, "visual transforms not yet implemented; uploading original");
    }

    // ── File size + hash (after EXIF write, so values match what is uploaded) ─
    let file_size = std::fs::metadata(&file_path).map(|m| m.len() as i64).ok();

    let path_for_hash = file_path.clone();
    let file_hash = match tokio::task::spawn_blocking(move || hash_mod::hash_file(&path_for_hash))
        .await
        .map_err(|e| WorkerError::Imaging(format!("spawn_blocking panicked: {e}")))?
    {
        Ok(h) => Some(h),
        Err(e) => {
            warn!(error = ?e, "edit_picture: file hash failed; skipping");
            None
        }
    };

    // ── Upload modified original ─────────────────────────────────────────────
    info!(job_id = %job_id, "edit_picture: uploading modified original");
    client.upload_presigned(&output_url, &file_path).await?;

    // ── Regenerate thumbnails (visual edits only) ────────────────────────────
    let thumb = thumbnailer::run(client, &file_path, &presigned_writes, tmp.path()).await?;

    client
        .complete_job(
            job_id,
            CompleteJobRequest {
                claim_token,
                exif: None,
                blurhash: thumb.blurhash,
                thumbnails_generated: thumb.generated,
                file_size,
                file_hash,
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
