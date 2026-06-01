//! In-process background task queue for lightweight DB-only jobs.
//!
//! Heavy compute tasks (thumbnail generation, ML inference) run in external
//! worker processes via the `/api/worker/*` endpoints. This queue handles
//! tasks that need direct database access and are not worth externalising:
//! - Tag-rename cascades (updates every affected row in the `tags` table)
//! - Tagging-pipeline evaluation (runs the pipeline evaluator on picture events)
//!
//! # Design
//! An unbounded `mpsc` channel decouples enqueue from execution.
//! A semaphore caps concurrency. Each task is spawned as a Tokio task
//! and holds a permit for its duration.

use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::{Semaphore, mpsc};
use uuid::Uuid;

// ── Task definitions ──────────────────────────────────────────────────────────

/// Variants of work that can be submitted to the in-process task queue.
#[derive(Debug)]
pub enum InternalTask {
    /// Cascade a tag rename across tags, shares, segmentation configs, and hierarchies.
    TagRename {
        user_id: Uuid,
        old_tag: String,
        new_tag: String,
    },
    /// Re-run the tagging pipeline for the given pictures in response to an event.
    RunTaggingPipeline {
        user_id: Uuid,
        /// Pipeline event labels that triggered the run (e.g. `["ingest"]`).
        event_labels: Vec<String>,
        picture_ids: Vec<Uuid>,
    },
}

// ── Queue handle ──────────────────────────────────────────────────────────────

/// Cheaply-cloneable handle for submitting tasks to the in-process queue.
/// Clone this into `AppState`; call `enqueue` anywhere in the application.
#[derive(Clone)]
pub struct TaskQueue {
    sender: mpsc::UnboundedSender<InternalTask>,
}

impl TaskQueue {
    /// Submit a task. Returns immediately; execution is asynchronous.
    /// Errors silently if the runner has been dropped (should never happen in practice).
    pub fn enqueue(&self, task: InternalTask) {
        if self.sender.send(task).is_err() {
            tracing::error!("task queue: receiver dropped — task lost");
        }
    }
}

// ── Queue constructor ─────────────────────────────────────────────────────────

/// Create a `(TaskQueue, runner_future)` pair.
///
/// Spawn `runner_future` with `tokio::spawn` immediately after creation so that
/// submitted tasks are actually executed. The runner runs until the `TaskQueue`
/// (and all its clones) are dropped.
///
/// * `db` — a shared Postgres pool passed to each task handler.
/// * `concurrency` — maximum number of tasks running in parallel.
pub fn create(
    db: PgPool,
    concurrency: usize,
) -> (TaskQueue, impl std::future::Future<Output = ()>) {
    let (tx, rx) = mpsc::unbounded_channel::<InternalTask>();
    let runner = TaskRunner {
        db,
        rx,
        sem: Arc::new(Semaphore::new(concurrency)),
    };
    (TaskQueue { sender: tx }, runner.run())
}

// ── Runner (private) ──────────────────────────────────────────────────────────

struct TaskRunner {
    db: PgPool,
    rx: mpsc::UnboundedReceiver<InternalTask>,
    sem: Arc<Semaphore>,
}

impl TaskRunner {
    async fn run(mut self) {
        tracing::info!("in-process task runner started");
        while let Some(task) = self.rx.recv().await {
            let permit = self
                .sem
                .clone()
                .acquire_owned()
                .await
                .expect("semaphore closed");
            let db = self.db.clone();
            tokio::spawn(async move {
                execute_task(db, task).await;
                drop(permit);
            });
        }
        tracing::info!("in-process task runner stopped");
    }
}

async fn execute_task(db: PgPool, task: InternalTask) {
    match task {
        InternalTask::TagRename {
            user_id,
            ref old_tag,
            ref new_tag,
        } => {
            tracing::info!(
                user_id = %user_id,
                old_tag = %old_tag,
                new_tag = %new_tag,
                "in-process task: tag rename"
            );
            if let Err(e) = run_tag_rename(&db, user_id, old_tag, new_tag).await {
                tracing::error!(
                    user_id = %user_id,
                    old_tag = %old_tag,
                    new_tag = %new_tag,
                    error = ?e,
                    "tag rename task failed"
                );
            }
        }
        InternalTask::RunTaggingPipeline {
            user_id,
            ref event_labels,
            ref picture_ids,
        } => {
            tracing::debug!(
                user_id = %user_id,
                labels = ?event_labels,
                pictures = picture_ids.len(),
                "in-process task: tagging pipeline (not yet implemented)"
            );
            // TODO: wire the domain pipeline evaluator here (roadmap item).
        }
    }
}

// ── Tag rename implementation ─────────────────────────────────────────────────

/// Cascade a tag rename across all affected rows in the database.
///
/// This is intentionally a best-effort transactional update: if any step fails
/// the error is logged and partial state may remain, which the UI surfaces to
/// the user so they can retry.
async fn run_tag_rename(
    db: &PgPool,
    user_id: Uuid,
    old_tag: &str,
    new_tag: &str,
) -> Result<(), sqlx::Error> {
    // Replace old_tag prefix with new_tag in all tags rows owned by this user.
    // Using text replace on the ltree path for prefix substitution.
    // Update every tag that is exactly old_tag or is a descendant (old_tag.foo.bar …).
    // We keep $2/$3 as text throughout to avoid Postgres type-inference conflicts between
    // replace() (needs text) and the ltree operators (needs ltree).
    sqlx::query!(
        r#"
        UPDATE tags
        SET tag_path = (replace(tags.tag_path::text, $2, $3))::ltree
        FROM pictures p
        WHERE tags.picture_id = p.id
          AND p.local_user_id  = $1
          AND (
              tags.tag_path::text = $2
              OR starts_with(tags.tag_path::text, $2 || '.')
          )
        "#,
        user_id,
        old_tag,
        new_tag,
    )
    .execute(db)
    .await?;

    tracing::info!(
        user_id = %user_id,
        old_tag = %old_tag,
        new_tag = %new_tag,
        "tag rename cascade complete"
    );
    Ok(())
}
