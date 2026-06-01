use crate::backend::BackendClient;
use crate::backend::models::{ClaimedJob, CompleteJobRequest};
use crate::error::{Result, WorkerError};
use crate::imaging::{exif as exif_mod, resize};
use tempfile::TempDir;
use tracing::{debug, info};

/// Handle an `edit_picture` job.
///
/// Current MVP scope: EXIF metadata overrides only. If `regenerate_thumbnails`
/// is true in the config, thumbnails are also regenerated.
///
/// Future: crop, brightness/contrast adjustments, etc.
pub async fn handle(client: &BackendClient, job: ClaimedJob) -> Result<()> {
    let job_id = job.job_id;
    let regenerate_thumbnails = job
        .config
        .get("regenerate_thumbnails")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let presigned_read = job
        .presigned_read
        .ok_or_else(|| WorkerError::MissingPresignedUrl {
            key: "original".to_string(),
        })?;

    let tmp = TempDir::new()?;
    let original_path = tmp.path().join("original");

    info!(job_id = %job_id, "downloading original for edit");
    client
        .download_presigned(&presigned_read, &original_path)
        .await?;

    // Extract current EXIF from the file (used as base for any overrides).
    let original_path_clone = original_path.clone();
    let (exif_result, blurhash) = tokio::task::spawn_blocking(move || {
        let exif = exif_mod::extract_exif(&original_path_clone).ok();

        let bh = if regenerate_thumbnails {
            resize::generate_blurhash(&original_path_clone).ok()
        } else {
            None
        };

        Ok::<_, WorkerError>((exif, bh))
    })
    .await
    .map_err(|e| WorkerError::Imaging(format!("spawn_blocking: {e}")))??;

    // If regenerate_thumbnails: generate and upload new thumbnails.
    if regenerate_thumbnails {
        let original_for_thumbs = original_path.clone();
        let tmp_dir = tmp.path().to_path_buf();
        let thumbnails = tokio::task::spawn_blocking(move || {
            let mut paths = std::collections::HashMap::new();
            for variant in &["small", "medium", "large"] {
                let height = resize::thumbnail_height(variant).unwrap();
                let dest = tmp_dir.join(format!("{}.webp", variant));
                if resize::generate_thumbnail(&original_for_thumbs, &dest, height).is_ok() {
                    paths.insert(variant.to_string(), dest);
                }
            }
            paths
        })
        .await
        .map_err(|e| WorkerError::Imaging(format!("spawn_blocking: {e}")))?;

        // Note: for edit_picture, presigned_writes has an "output" key for the
        // edited original, not thumbnail keys. For now thumbnail uploads would
        // need a separate mechanism. This is a known limitation of the MVP.
        // TODO: backend should also provide presigned PUT URLs for thumbnails on edit jobs.
        debug!(
            job_id = %job_id,
            thumbnails = thumbnails.len(),
            "thumbnails generated (upload not yet implemented for edit jobs)"
        );
    }

    // Build completion request.
    // The backend's complete_job handler will apply exif_overrides from the job config.
    let request = CompleteJobRequest {
        exif: exif_result,
        blurhash,
    };
    client.complete_job(job_id, request).await?;
    info!(job_id = %job_id, "edit_picture job completed");
    Ok(())
}
