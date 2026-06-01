pub mod edit_picture;
pub mod ml;
pub mod thumbnail;

use crate::backend::BackendClient;
use crate::config::Config;
use crate::error::WorkerError;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::time::{Duration, sleep};
use tracing::{error, info, warn};

/// Run the job polling loop indefinitely.
///
/// Polls the backend for jobs at `config.poll_interval_ms` intervals.
/// Up to `config.max_concurrent_jobs` jobs run concurrently.
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
        // Try to acquire a concurrency slot before polling.
        // This avoids claiming a job we can't immediately start processing.
        let permit = match sem.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                // All slots in use — wait and retry.
                sleep(Duration::from_millis(config.poll_interval_ms)).await;
                continue;
            }
        };

        match client.claim_next_job().await {
            Ok(None) => {
                // No job available — release the slot and wait.
                drop(permit);
                sleep(Duration::from_millis(config.poll_interval_ms)).await;
            }
            Ok(Some(job)) => {
                let job_id = job.job_id;
                let job_type = job.job_type.clone();
                let client_clone = client.clone();

                tokio::spawn(async move {
                    info!(job_id = %job_id, job_type = %job_type, "starting job");
                    let result = dispatch(client_clone.as_ref(), job).await;
                    if let Err(e) = result {
                        error!(job_id = %job_id, job_type = %job_type, error = ?e, "job failed");
                        // The dispatch function is responsible for calling fail_job on the backend.
                        // If it failed before reaching that point (e.g. network error after
                        // claiming), the job will time out on the backend side (future work).
                    }
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

async fn dispatch(
    client: &BackendClient,
    job: crate::backend::models::ClaimedJob,
) -> Result<(), WorkerError> {
    let job_id = job.job_id;
    let job_type = job.job_type.clone();

    let result = match job_type.as_str() {
        "gen_thumbnail" => thumbnail::handle(client, job).await,
        "edit_picture" => edit_picture::handle(client, job).await,
        "ml_style" | "ml_people" | "ml_group_location" => ml::handle_stub(client, job).await,
        other => {
            warn!(job_id = %job_id, job_type = other, "unknown job type — skipping");
            client
                .fail_job(job_id, &format!("unknown job type: {}", other))
                .await
        }
    };

    if let Err(ref e) = result {
        // Best-effort fail report. If this also fails, the job will remain in
        // 'processing' state until a timeout/cleanup mechanism resets it.
        if let Err(report_err) = client.fail_job(job_id, &e.to_string()).await {
            error!(
                job_id = %job_id,
                error = ?report_err,
                "failed to report job failure to backend"
            );
        }
    }
    result
}
