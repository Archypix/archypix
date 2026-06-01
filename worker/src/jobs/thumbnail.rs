use crate::backend::BackendClient;
use crate::backend::models::{ClaimedJob, CompleteJobRequest};
use crate::error::{Result, WorkerError};
use crate::imaging::{exif as exif_mod, resize};
use tempfile::TempDir;
use tracing::{debug, info};

/// Handle a `gen_thumbnail` job.
///
/// Flow:
/// 1. Download original from presigned URL to a temp file.
/// 2. Extract EXIF (if is_initial).
/// 3. Generate small/medium/large WebP thumbnails.
/// 4. Generate blurhash.
/// 5. Upload each thumbnail via its presigned PUT URL.
/// 6. Report completion to backend.
pub async fn handle(client: &BackendClient, job: ClaimedJob) -> Result<()> {
    let job_id = job.job_id;
    let is_initial = job
        .config
        .get("is_initial")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let presigned_read = job
        .presigned_read
        .ok_or_else(|| WorkerError::MissingPresignedUrl {
            key: "original".to_string(),
        })?;

    // Create a temporary directory for all intermediate files.
    let tmp = TempDir::new()?;
    let original_path = tmp.path().join("original");

    info!(job_id = %job_id, "downloading original picture");
    client
        .download_presigned(&presigned_read, &original_path)
        .await?;
    debug!(
        job_id = %job_id,
        size_bytes = original_path.metadata()?.len(),
        "original downloaded"
    );

    // Run blocking image processing in a spawn_blocking thread.
    let original_path_clone = original_path.clone();
    let tmp_dir = tmp.path().to_path_buf();

    let (exif_result, thumbnails, blurhash) = tokio::task::spawn_blocking(move || {
        // EXIF extraction
        let exif = if is_initial {
            match exif_mod::extract_exif(&original_path_clone) {
                Ok(e) => {
                    debug!("EXIF extracted successfully");
                    Some(e)
                }
                Err(e) => {
                    tracing::warn!(error = ?e, "EXIF extraction failed; continuing without EXIF");
                    None
                }
            }
        } else {
            None
        };

        // Thumbnail generation
        let variants = ["small", "medium", "large"];
        let mut paths = std::collections::HashMap::new();
        for variant in &variants {
            let height = resize::thumbnail_height(variant).unwrap();
            let dest = tmp_dir.join(format!("{}.webp", variant));
            match resize::generate_thumbnail(&original_path_clone, &dest, height) {
                Ok(()) => {
                    paths.insert(variant.to_string(), dest);
                }
                Err(e) => {
                    tracing::warn!(variant, error = ?e, "thumbnail generation failed");
                }
            }
        }

        // Blurhash from the original
        let bh = match resize::generate_blurhash(&original_path_clone) {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::warn!(error = ?e, "blurhash generation failed");
                None
            }
        };

        Ok::<_, WorkerError>((exif, paths, bh))
    })
    .await
    .map_err(|e| WorkerError::Imaging(format!("spawn_blocking panicked: {e}")))??;

    // Upload thumbnails via presigned PUT URLs.
    for (variant, path) in &thumbnails {
        let put_url = job.presigned_writes.get(variant.as_str()).ok_or_else(|| {
            WorkerError::MissingPresignedUrl {
                key: variant.clone(),
            }
        })?;
        debug!(job_id = %job_id, variant, "uploading thumbnail");
        client.upload_presigned(put_url, path).await?;
    }

    // Report completion.
    let request = CompleteJobRequest {
        exif: exif_result,
        blurhash,
    };
    client.complete_job(job_id, request).await?;
    info!(job_id = %job_id, "gen_thumbnail job completed");
    Ok(())
}
