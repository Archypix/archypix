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
//! Producers call [`PipelineWaker::wake`] with the **id of the user whose pictures or shares
//! changed** (not necessarily the request caller). The wake is an `mpsc<Uuid>` message consumed by
//! the loop's per-user scheduler. A configurable poll interval provides a recovery sweep for
//! crash/lost-wake recovery, so a missed wake is only a latency issue, never a correctness one.
//!
//! # Concurrency
//! Per-user runs are serialized (one worker per `user_id` at a time — concurrent runs for the same
//! user would race on its tag reconcile and tracking writes) and parallel across users, bounded by
//! `PIPELINE_CONCURRENCY`. Wakes that arrive while a user is running are coalesced into a single
//! re-run. See `doc/features/02_pipeline_announcement_robustness.md` §7.

pub mod announcement;
pub mod evaluation;

use crate::clients::federation::FederationClient;
use crate::infra::config::Config;
use crate::infra::error::AppError;
use crate::infra::redis::Cache;
use crate::repository::pipeline::PipelineRepository;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{Semaphore, mpsc};
use uuid::Uuid;

/// Borrowed dependencies for a single per-user pipeline run. Delivery is now inline (the pipeline
/// announces/unannounces itself rather than enqueuing tasks), so a run needs the federation client,
/// the cache (for same-backend resolution via `find_local_user_id`), and the waker (to wake
/// same-backend recipients after local registration).
pub struct PipelineRun<'a> {
    pub db: &'a PgPool,
    pub federation: &'a FederationClient,
    pub cache: &'a dyn Cache,
    pub config: &'a Config,
    pub waker: &'a PipelineWaker,
}

// ── Waker ───────────────────────────────────────────────────────────────────

/// Cheaply-cloneable handle for waking the pipeline for a specific user. Clone this into
/// `AppState` and the task runner; call [`wake`](Self::wake) after any event that creates dirty
/// pictures or share work for that user (ingest, tag edit, service config change, share accept,
/// same-backend (un)announce, …).
#[derive(Clone)]
pub struct PipelineWaker {
    tx: mpsc::UnboundedSender<Uuid>,
}

impl PipelineWaker {
    /// Wake the pipeline for `user_id`. Silently no-ops if the loop has shut down — a missed wake is
    /// recovered by the poll sweep.
    pub fn wake(&self, user_id: Uuid) {
        let _ = self.tx.send(user_id);
    }

    /// A waker not attached to any loop; its wakes are discarded. For tests and standalone calls.
    pub fn disconnected() -> Self {
        let (tx, _rx) = mpsc::unbounded_channel();
        PipelineWaker { tx }
    }
}

/// Build the waker and the receiver consumed by [`create`]. Splitting construction lets `main` wire
/// the waker into the `TaskQueue` (which wakes recipients after same-backend delivery) before the
/// loop future is built, breaking the waker ↔ task_queue cycle.
pub fn channel() -> (PipelineWaker, mpsc::UnboundedReceiver<Uuid>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (PipelineWaker { tx }, rx)
}

// ── Loop ─────────────────────────────────────────────────────────────────────

/// Per-user run state held by the scheduler.
enum RunState {
    /// A worker is running (or queued on the semaphore) for this user.
    Running,
    /// A worker is running and a fresh wake arrived meanwhile → run once more on completion.
    Rerun,
}

/// Shared context handed to each per-user worker. Holds owned dependencies; each run borrows them
/// into a [`PipelineRun`].
struct Scheduler {
    db: PgPool,
    federation: FederationClient,
    cache: Arc<dyn Cache>,
    config: Config,
    waker: PipelineWaker,
    sem: Arc<Semaphore>,
    state: Arc<Mutex<HashMap<Uuid, RunState>>>,
}

impl Scheduler {
    /// Ensure a worker is (or will be) running for `user_id`, coalescing concurrent wakes.
    fn schedule(self: &Arc<Self>, user_id: Uuid) {
        {
            let mut map = self
                .state
                .lock()
                .expect("pipeline scheduler mutex poisoned");
            match map.get_mut(&user_id) {
                Some(s) => {
                    *s = RunState::Rerun; // a run is in flight — request a re-run after it
                    return;
                }
                None => {
                    map.insert(user_id, RunState::Running);
                }
            }
        }

        let this = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                let permit = this
                    .sem
                    .clone()
                    .acquire_owned()
                    .await
                    .expect("pipeline semaphore closed");
                let run = PipelineRun {
                    db: &this.db,
                    federation: &this.federation,
                    cache: this.cache.as_ref(),
                    config: &this.config,
                    waker: &this.waker,
                };
                if let Err(e) = evaluation::run_for_user(&run, user_id).await {
                    tracing::error!(user_id = %user_id, error = ?e, "pipeline: failed for user");
                }
                drop(permit);

                let mut map = this
                    .state
                    .lock()
                    .expect("pipeline scheduler mutex poisoned");
                match map.get(&user_id) {
                    Some(RunState::Rerun) => {
                        map.insert(user_id, RunState::Running); // events arrived mid-run → loop
                    }
                    _ => {
                        map.remove(&user_id);
                        break;
                    }
                }
            }
        });
    }
}

/// Spawn the pipeline loop as a Tokio task.
///
/// Returns a future that runs forever (until the process exits). Spawn it with `tokio::spawn`.
/// `rx` comes from [`channel`]; the matching [`PipelineWaker`] is what producers call.
#[allow(clippy::too_many_arguments)]
pub fn create(
    db: PgPool,
    rx: mpsc::UnboundedReceiver<Uuid>,
    poll_interval: Duration,
    config: Config,
    concurrency: usize,
    federation: FederationClient,
    cache: Arc<dyn Cache>,
    waker: PipelineWaker,
) -> impl Future<Output = ()> {
    async move {
        run(
            db,
            rx,
            poll_interval,
            config,
            concurrency,
            federation,
            cache,
            waker,
        )
        .await
    }
}

#[allow(clippy::too_many_arguments)]
async fn run(
    db: PgPool,
    mut rx: mpsc::UnboundedReceiver<Uuid>,
    poll_interval: Duration,
    config: Config,
    concurrency: usize,
    federation: FederationClient,
    cache: Arc<dyn Cache>,
    waker: PipelineWaker,
) {
    tracing::info!(
        poll_interval_secs = poll_interval.as_secs(),
        concurrency,
        "tagging pipeline loop started"
    );

    let scheduler = Arc::new(Scheduler {
        db: db.clone(),
        federation,
        cache,
        config,
        waker,
        sem: Arc::new(Semaphore::new(concurrency.max(1))),
        state: Arc::new(Mutex::new(HashMap::new())),
    });

    // Startup recovery sweep: pick up everything dirty from before this process started.
    recovery_sweep(&db, &scheduler).await;

    loop {
        tokio::select! {
            maybe = rx.recv() => {
                match maybe {
                    Some(user_id) => scheduler.schedule(user_id),
                    None => break, // all wakers dropped — process shutting down
                }
            }
            _ = tokio::time::sleep(poll_interval) => {
                tracing::debug!("pipeline: recovery sweep");
                recovery_sweep(&db, &scheduler).await;
            }
        }
    }

    tracing::info!("tagging pipeline loop stopped");
}

/// Enqueue every user that currently has dirty pictures or a share awaiting (re)announcement.
async fn recovery_sweep(db: &PgPool, scheduler: &Arc<Scheduler>) {
    match PipelineRepository::find_users_with_dirty_pictures(db).await {
        Ok(users) => {
            for user_id in users {
                scheduler.schedule(user_id);
            }
        }
        Err(e) => tracing::error!(error = ?e, "pipeline recovery sweep error"),
    }
}

/// Used for testing only. Runs one full pipeline pass for a user with inline delivery.
pub async fn run_once_for_user(
    db: &PgPool,
    federation: &FederationClient,
    cache: &dyn Cache,
    config: &Config,
    waker: &PipelineWaker,
    user_id: Uuid,
) -> Result<(), AppError> {
    let run = PipelineRun {
        db,
        federation,
        cache,
        config,
        waker,
    };
    evaluation::run_for_user(&run, user_id).await
}
