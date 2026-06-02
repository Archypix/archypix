use crate::domain::job::{Job, JobConfig, JobStatus, JobType};
use crate::infra::error::{AppError, map_sqlx_error};
use sqlx::{Executor, PgPool, Postgres};
use uuid::Uuid;

pub struct JobRepository;

/// Column list shared by every query that returns a full `Job` row.
/// Must stay in sync with the `Job` struct field order.
macro_rules! job_columns {
    () => {
        r#"id, owner_id,
           job_type     AS "job_type: JobType",
           status       AS "status: JobStatus",
           config       AS "config: _",
           result       AS "result: _",
           error_message,
           retry_count, max_retries,
           idempotency_key,
           picture_id, claimed_by, claim_token,
           created_at, started_at, completed_at"#
    };
}

impl JobRepository {
    /// Atomically claim the next pending job matching any of `job_types`.
    ///
    /// Generates a fresh `claim_token` UUID for this claim; the worker must echo
    /// it back in `complete` / `fail` to prevent stale workers from corrupting
    /// re-claimed jobs.
    pub async fn claim_next(
        db: &PgPool,
        worker_id: &str,
        job_types: &[JobType],
    ) -> Result<Option<Job>, AppError> {
        let mut tx = db.begin().await.map_err(map_sqlx_error)?;

        let job_id: Option<Uuid> = if job_types.is_empty() {
            sqlx::query_scalar!(
                "SELECT id FROM jobs WHERE status = 'pending' \
                 ORDER BY created_at LIMIT 1 FOR UPDATE SKIP LOCKED"
            )
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_sqlx_error)?
        } else {
            let type_strs: Vec<String> = job_types.iter().map(|t| t.to_string()).collect();
            sqlx::query_scalar(
                "SELECT id FROM jobs WHERE status = 'pending' \
                 AND job_type::text = ANY($1) \
                 ORDER BY created_at LIMIT 1 FOR UPDATE SKIP LOCKED",
            )
            .bind(&type_strs)
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_sqlx_error)?
        };

        let Some(job_id) = job_id else {
            tx.rollback().await.ok();
            return Ok(None);
        };

        let claim_token = Uuid::new_v4();

        let job = sqlx::query_as!(
            Job,
            r#"UPDATE jobs
               SET status      = 'processing',
                   started_at  = (now() AT TIME ZONE 'utc'),
                   claimed_by  = $2,
                   claim_token = $3
               WHERE id = $1
               RETURNING
                   id, owner_id,
                   job_type    AS "job_type: JobType",
                   status      AS "status: JobStatus",
                   config      AS "config: _",
                   result      AS "result: _",
                   error_message,
                   retry_count, max_retries,
                   idempotency_key,
                   picture_id, claimed_by, claim_token,
                   created_at, started_at, completed_at"#,
            job_id,
            worker_id,
            claim_token,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(Some(job))
    }

    /// Mark a job as completed and store the result JSON.
    ///
    /// Returns `None` when the job is not in `processing` state or the
    /// `claim_token` does not match — this prevents stale workers (reset by the
    /// watchdog) from overwriting results of a re-claimed job.
    pub async fn complete<'e, E>(
        ex: E,
        job_id: Uuid,
        claim_token: Uuid,
        result: serde_json::Value,
    ) -> Result<Option<Job>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as!(
            Job,
            r#"UPDATE jobs
               SET status       = 'completed',
                   completed_at = (now() AT TIME ZONE 'utc'),
                   result       = $3
               WHERE id         = $1
                 AND claim_token = $2
                 AND status     = 'processing'
               RETURNING
                   id, owner_id,
                   job_type    AS "job_type: JobType",
                   status      AS "status: JobStatus",
                   config      AS "config: _",
                   result      AS "result: _",
                   error_message,
                   retry_count, max_retries,
                   idempotency_key,
                   picture_id, claimed_by, claim_token,
                   created_at, started_at, completed_at"#,
            job_id,
            claim_token,
            result as serde_json::Value,
        )
        .fetch_optional(ex)
        .await
        .map_err(map_sqlx_error)
    }

    /// Mark a job as failed.
    ///
    /// Returns `None` when the `claim_token` does not match or the job is no
    /// longer in `processing` state (same guard as `complete`).
    ///
    /// When `permanent` is `true`, the job transitions directly to `failed`
    /// regardless of remaining retries.  When `false`, the retry counter is
    /// checked: if retries remain the job resets to `pending`.
    pub async fn fail<'e, E>(
        ex: E,
        job_id: Uuid,
        claim_token: Uuid,
        error: &str,
        permanent: bool,
    ) -> Result<Option<Job>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as!(
            Job,
            r#"UPDATE jobs
               SET status        = CASE
                                       WHEN $4 OR retry_count + 1 >= max_retries
                                           THEN 'failed'::job_status
                                       ELSE 'pending'::job_status
                                   END,
                   retry_count   = retry_count + 1,
                   error_message = $3,
                   started_at    = CASE
                                       WHEN $4 OR retry_count + 1 >= max_retries THEN started_at
                                       ELSE NULL
                                   END,
                   claimed_by    = NULL,
                   claim_token   = NULL,
                   completed_at  = CASE
                                       WHEN $4 OR retry_count + 1 >= max_retries
                                           THEN (now() AT TIME ZONE 'utc')
                                       ELSE NULL
                                   END
               WHERE id          = $1
                 AND claim_token = $2
                 AND status      = 'processing'
               RETURNING
                   id, owner_id,
                   job_type    AS "job_type: JobType",
                   status      AS "status: JobStatus",
                   config      AS "config: _",
                   result      AS "result: _",
                   error_message,
                   retry_count, max_retries,
                   idempotency_key,
                   picture_id, claimed_by, claim_token,
                   created_at, started_at, completed_at"#,
            job_id,
            claim_token,
            error,
            permanent,
        )
        .fetch_optional(ex)
        .await
        .map_err(map_sqlx_error)
    }

    /// Enqueue a new job.
    ///
    /// `job_type` is derived from `config` via `JobConfig::job_type()` so the
    /// DB column and the JSONB discriminant can never disagree.
    /// Idempotency conflict returns `AppError::Conflict`.
    pub async fn create<'e, E>(
        ex: E,
        owner_id: Uuid,
        picture_id: Option<Uuid>,
        config: &JobConfig,
        idempotency_key: Option<&str>,
    ) -> Result<Job, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let job_type = config.job_type();
        let config_value = serde_json::to_value(config)
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        sqlx::query_as!(
            Job,
            r#"INSERT INTO jobs (owner_id, job_type, picture_id, config, idempotency_key)
               VALUES ($1, $2, $3, $4, $5)
               RETURNING
                   id, owner_id,
                   job_type    AS "job_type: JobType",
                   status      AS "status: JobStatus",
                   config      AS "config: _",
                   result      AS "result: _",
                   error_message,
                   retry_count, max_retries,
                   idempotency_key,
                   picture_id, claimed_by, claim_token,
                   created_at, started_at, completed_at"#,
            owner_id,
            job_type as JobType,
            picture_id,
            config_value as serde_json::Value,
            idempotency_key,
        )
        .fetch_one(ex)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn find_by_id<'e, E>(ex: E, id: Uuid) -> Result<Option<Job>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as!(
            Job,
            r#"SELECT id, owner_id,
                      job_type    AS "job_type: JobType",
                      status      AS "status: JobStatus",
                      config      AS "config: _",
                      result      AS "result: _",
                      error_message,
                      retry_count, max_retries,
                      idempotency_key,
                      picture_id, claimed_by, claim_token,
                      created_at, started_at, completed_at
               FROM   jobs
               WHERE  id = $1"#,
            id,
        )
        .fetch_optional(ex)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn list_by_picture(
        db: &PgPool,
        picture_id: Uuid,
        owner_id: Uuid,
    ) -> Result<Vec<Job>, AppError> {
        sqlx::query_as!(
            Job,
            r#"SELECT id, owner_id,
                      job_type    AS "job_type: JobType",
                      status      AS "status: JobStatus",
                      config      AS "config: _",
                      result      AS "result: _",
                      error_message,
                      retry_count, max_retries,
                      idempotency_key,
                      picture_id, claimed_by, claim_token,
                      created_at, started_at, completed_at
               FROM   jobs
               WHERE  picture_id = $1
                 AND  owner_id   = $2
               ORDER BY created_at DESC"#,
            picture_id,
            owner_id,
        )
        .fetch_all(db)
        .await
        .map_err(map_sqlx_error)
    }

    /// Reset jobs stuck in `processing` for longer than `timeout_secs`.
    ///
    /// Clears `claimed_by` and `claim_token` so a fresh worker gets a new token
    /// when it re-claims the job — preventing the original (late) worker from
    /// completing the retried run.
    pub async fn reset_stale(db: &PgPool, timeout_secs: i64) -> Result<u64, AppError> {
        let result = sqlx::query!(
            r#"UPDATE jobs
               SET status        = CASE
                                       WHEN retry_count + 1 < max_retries THEN 'pending'::job_status
                                       ELSE 'failed'::job_status
                                   END,
                   retry_count   = retry_count + 1,
                   error_message = 'Worker timed out without reporting a result',
                   claimed_by    = NULL,
                   claim_token   = NULL,
                   started_at    = CASE
                                       WHEN retry_count + 1 < max_retries THEN NULL
                                       ELSE started_at
                                   END,
                   completed_at  = CASE
                                       WHEN retry_count + 1 < max_retries THEN NULL
                                       ELSE (now() AT TIME ZONE 'utc')
                                   END
               WHERE status     = 'processing'
                 AND started_at < (now() AT TIME ZONE 'utc') - ($1 * INTERVAL '1 second')"#,
            timeout_secs as f64,
        )
        .execute(db)
        .await
        .map_err(map_sqlx_error)?;
        Ok(result.rows_affected())
    }
}
