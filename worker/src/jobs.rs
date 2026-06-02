pub mod edit_picture;
pub mod ml;
pub mod thumbnail;

use crate::backend::BackendClient;
use crate::config::Config;
use archypix_common::job::JobConfig;
use archypix_common::transfer::ClaimJobResponse;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::time::{Duration, sleep};
use tracing::{error, info, warn};

pub async fn run_job_loop(config: Arc<Config>, client: Arc<BackendClient>) {
    let sem = Arc::new(Semaphore::new(config.max_concurrent_jobs));
    info!(
        worker_id = %config.worker_id,
        poll_interval_ms = config.poll_interval_ms,
        max_concurrent_jobs = config.max_concurrent_jobs,
        job_types = ?config.job_types,
        "job runner started"
    );

    loop {
        let permit = match sem.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                sleep(Duration::from_millis(config.poll_interval_ms)).await;
                continue;
            }
        };

        match client.claim_next_job().await {
            Ok(None) => {
                drop(permit);
                sleep(Duration::from_millis(config.poll_interval_ms)).await;
            }
            Ok(Some(job)) => {
                let job_id = job.job_id;
                let job_type = job.job_type.clone();
                let client_clone = client.clone();

                tokio::spawn(async move {
                    info!(job_id = %job_id, %job_type, "starting job");
                    dispatch(client_clone.as_ref(), job).await;
                    drop(permit);
                });
            }
            Err(e) => {
                warn!(error = ?e, "error polling for jobs");
                drop(permit);
                sleep(Duration::from_millis(config.poll_interval_ms * 5)).await;
            }
        }
    }
}

/// Decompose the claim response and dispatch to the appropriate handler.
///
/// Errors are reported to the backend via `fail_job` before returning.
/// The function itself is infallible — callers never need to check its result.
async fn dispatch(client: &BackendClient, job: ClaimJobResponse) {
    let job_id = job.job_id;
    let job_type = job.job_type.clone();
    let presigned_read = job.presigned_read;
    let presigned_writes = job.presigned_writes;
    let mime_type = job.mime_type;

    let result = match job.config {
        JobConfig::GenThumbnail(config) => {
            thumbnail::handle(
                client,
                job_id,
                config,
                presigned_read,
                presigned_writes,
                mime_type,
            )
            .await
        }
        JobConfig::EditPicture(config) => {
            edit_picture::handle(
                client,
                job_id,
                config,
                presigned_read,
                presigned_writes,
                mime_type,
            )
            .await
        }
        JobConfig::MlStyle | JobConfig::MlPeople | JobConfig::MlGroupLocation => {
            ml::handle_stub(client, job_id, job_type).await
        }
    };

    if let Err(ref e) = result {
        let permanent = !e.is_retriable();
        error!(job_id = %job_id, permanent, error = ?e, "job failed");
        if let Err(report_err) = client.fail_job(job_id, &e.to_string(), permanent).await {
            error!(
                job_id = %job_id,
                error = ?report_err,
                "failed to report job failure to backend"
            );
        }
    }
}
