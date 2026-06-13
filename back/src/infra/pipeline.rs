//! Tagging pipeline background loop.
//!
//! The pipeline evaluates enabled tagging services against dirty pictures and
//! applies the resulting tag assignments, then diffs share coverage against the
//! `share_announcements` tracking table to announce/unannounce shared pictures.
//! A picture is dirty when:
//! - Its `last_pipeline_run_at` is NULL (never processed), or
//! - Its `last_pipeline_run_at` is older than any enabled service's `last_invalidated_at`.
//!
//! # Wake model
//! The loop uses a `tokio::sync::Notify` for event-driven wakes (e.g. after ingest,
//! manual tag change, service config change, or share accept) and falls back to a
//! configurable polling interval for crash recovery.

pub mod announcement;
pub mod evaluation;

use crate::infra::config::Config;
use crate::infra::error::{AppError, map_sqlx_error};
use crate::infra::tasks::TaskQueue;
use crate::repository::pipeline::PipelineRepository;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use uuid::Uuid;

/// Spawn the pipeline loop as a Tokio task.
///
/// `notify` is shared with `AppState`; call `notify.notify_one()` to wake the loop immediately.
///
/// Returns a future that runs forever (until the process exits). Spawn it with `tokio::spawn`.
pub fn create(
    db: PgPool,
    notify: Arc<Notify>,
    poll_interval: Duration,
    task_queue: TaskQueue,
    config: Config,
) -> impl Future<Output = ()> {
    async move { run(db, notify, poll_interval, task_queue, config).await }
}

async fn run(
    db: PgPool,
    notify: Arc<Notify>,
    poll_interval: Duration,
    task_queue: TaskQueue,
    config: Config,
) {
    tracing::info!(
        poll_interval_secs = poll_interval.as_secs(),
        "tagging pipeline loop started"
    );
    loop {
        tokio::select! {
            _ = notify.notified() => {
                tracing::debug!("pipeline: woken by event");
            }
            _ = tokio::time::sleep(poll_interval) => {
                tracing::debug!("pipeline: recovery sweep");
            }
        }

        if let Err(e) = sweep(&db, &task_queue, &config).await {
            tracing::error!(error = ?e, "pipeline sweep error");
        }
    }
}

async fn sweep(db: &PgPool, task_queue: &TaskQueue, config: &Config) -> Result<(), AppError> {
    let users = PipelineRepository::find_users_with_dirty_pictures(db).await?;

    if users.is_empty() {
        return Ok(());
    }

    tracing::debug!(user_count = users.len(), "pipeline: processing dirty users");
    for user_id in users {
        if let Err(e) = evaluation::run_for_user(db, task_queue, config, user_id).await {
            tracing::error!(user_id = %user_id, error = ?e, "pipeline: failed for user");
        }
    }
    Ok(())
}

/// Used for testing only.
pub async fn run_once_for_user(
    db: &PgPool,
    task_queue: &TaskQueue,
    config: &Config,
    user_id: Uuid,
) -> Result<(), AppError> {
    evaluation::run_for_user(db, task_queue, config, user_id).await
}
