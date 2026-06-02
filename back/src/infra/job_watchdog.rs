//! Background task that periodically scans for jobs stuck in `processing`.
//!
//! A job can get stuck when a worker crashes, is OOM-killed, or loses network
//! connectivity after claiming a job but before reporting completion or failure.
//! Without recovery, those jobs would stay in `processing` forever.
//!
//! The watchdog runs every `interval` seconds and calls
//! [`JobRepository::reset_stale`], which resets eligible jobs to `pending`
//! (or to `failed` if they have exhausted their retry budget), using the same
//! retry-count logic as a normal job failure.

use crate::repository::job::JobRepository;
use sqlx::PgPool;
use tokio::time::{Duration, sleep};
use tracing::{error, info};

/// Run the job watchdog loop indefinitely.
///
/// Spawn this with `tokio::spawn` at startup.
///
/// * `db`          — shared Postgres pool.
/// * `timeout_secs`— how long a job may be in `processing` before being reset.
/// * `interval_secs`— how often to run the scan.
pub async fn run(db: PgPool, timeout_secs: i64, interval_secs: u64) {
    let interval = Duration::from_secs(interval_secs);
    info!(timeout_secs, interval_secs, "job watchdog started");

    loop {
        sleep(interval).await;

        match JobRepository::reset_stale(&db, timeout_secs).await {
            Ok(0) => {} // nothing to do — avoid noisy logs when idle
            Ok(n) => info!(reset = n, "job watchdog: reset stale jobs"),
            Err(e) => error!(error = ?e, "job watchdog: reset_stale failed"),
        }
    }
}
