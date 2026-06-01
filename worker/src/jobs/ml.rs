use crate::backend::BackendClient;
use crate::backend::models::ClaimedJob;
use crate::error::Result;
use tracing::info;

/// Placeholder handler for ML-based jobs (ml_style, ml_people, ml_group_location).
/// Not yet implemented — logs and reports success with empty result.
pub async fn handle_stub(client: &BackendClient, job: ClaimedJob) -> Result<()> {
    info!(
        job_id = %job.job_id,
        job_type = %job.job_type,
        "ML job received (not yet implemented); marking as complete with empty result"
    );
    client
        .complete_job(
            job.job_id,
            crate::backend::models::CompleteJobRequest {
                exif: None,
                blurhash: None,
            },
        )
        .await
}
