use crate::domain::job::{Job, JobStatus, JobType};
use crate::infra::error::{AppError, map_sqlx_error};
use sqlx::PgPool;
use uuid::Uuid;

pub struct JobRepository;

impl JobRepository {
    /// Atomically claim the next pending job matching `job_types`.
    ///
    /// Uses a transaction with `SELECT … FOR UPDATE SKIP LOCKED` so that
    /// concurrent workers each claim a distinct job without blocking each other.
    ///
    /// Returns `None` when no eligible job is currently available.
    pub async fn claim_next(
        db: &PgPool,
        worker_id: &str,
        job_types: &[JobType],
    ) -> Result<Option<Job>, AppError> {
        let mut tx = db.begin().await.map_err(map_sqlx_error)?;

        // Find the next eligible pending job id.
        let job_id: Option<Uuid> = if job_types.is_empty() {
            sqlx::query_scalar!(
                "SELECT id FROM jobs WHERE status = 'pending' ORDER BY created_at LIMIT 1 FOR UPDATE SKIP LOCKED"
            )
                .fetch_optional(&mut *tx)
                .await
                .map_err(map_sqlx_error)?
        } else {
            // Convert job types to their postgres string representations for the ANY filter.
            let type_strs: Vec<String> = job_types
                .iter()
                .map(|t| match t {
                    JobType::GenThumbnail => "gen_thumbnail",
                    JobType::MlStyle => "ml_style",
                    JobType::MlPeople => "ml_people",
                    JobType::MlGroupLocation => "ml_group_location",
                    JobType::EditPicture => "edit_picture",
                })
                .map(|s| s.to_string())
                .collect();

            sqlx::query_scalar(
                "SELECT id FROM jobs WHERE status = 'pending' AND job_type::text = ANY($1) ORDER BY created_at LIMIT 1 FOR UPDATE SKIP LOCKED"
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

        // Claim the job by marking it as processing.
        let job = sqlx::query_as!(
            Job,
            r#"UPDATE jobs
               SET status     = 'processing',
                   started_at = (now() AT TIME ZONE 'utc'),
                   claimed_by = $2
               WHERE id = $1
               RETURNING
                   id, owner_id,
                   job_type    AS "job_type: JobType",
                   status      AS "status: JobStatus",
                   config      AS "config: _",
                   result      AS "result: _",
                   result_s3_keys,
                   error_message,
                   retry_count, max_retries,
                   idempotency_key,
                   picture_id, claimed_by,
                   created_at, started_at, completed_at"#,
            job_id,
            worker_id,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(Some(job))
    }

    /// Mark a job as completed and store the result JSON.
    pub async fn complete(
        db: &PgPool,
        job_id: Uuid,
        result: serde_json::Value,
    ) -> Result<Job, AppError> {
        sqlx::query_as!(
            Job,
            r#"UPDATE jobs
               SET status       = 'completed',
                   completed_at = (now() AT TIME ZONE 'utc'),
                   result       = $2
               WHERE id = $1
               RETURNING
                   id, owner_id,
                   job_type    AS "job_type: JobType",
                   status      AS "status: JobStatus",
                   config      AS "config: _",
                   result      AS "result: _",
                   result_s3_keys,
                   error_message,
                   retry_count, max_retries,
                   idempotency_key,
                   picture_id, claimed_by,
                   created_at, started_at, completed_at"#,
            job_id,
            result as serde_json::Value,
        )
        .fetch_one(db)
        .await
        .map_err(map_sqlx_error)
    }

    /// Mark a job as failed. If retries remain, reset it to `pending`; otherwise
    /// set it to `failed` permanently.
    pub async fn fail(db: &PgPool, job_id: Uuid, error: &str) -> Result<Job, AppError> {
        sqlx::query_as!(
            Job,
            r#"UPDATE jobs
               SET status        = CASE
                                       WHEN retry_count + 1 < max_retries THEN 'pending'::job_status
                                       ELSE 'failed'::job_status
                                   END,
                   retry_count   = retry_count + 1,
                   error_message = $2,
                   started_at    = CASE
                                       WHEN retry_count + 1 < max_retries THEN NULL
                                       ELSE started_at
                                   END,
                   claimed_by    = NULL,
                   completed_at  = CASE
                                       WHEN retry_count + 1 < max_retries THEN NULL
                                       ELSE (now() AT TIME ZONE 'utc')
                                   END
               WHERE id = $1
               RETURNING
                   id, owner_id,
                   job_type    AS "job_type: JobType",
                   status      AS "status: JobStatus",
                   config      AS "config: _",
                   result      AS "result: _",
                   result_s3_keys,
                   error_message,
                   retry_count, max_retries,
                   idempotency_key,
                   picture_id, claimed_by,
                   created_at, started_at, completed_at"#,
            job_id,
            error,
        )
        .fetch_one(db)
        .await
        .map_err(map_sqlx_error)
    }

    /// Enqueue a new job. Returns the created `Job`.
    /// If an idempotency conflict occurs, `map_sqlx_error` returns `AppError::Conflict`.
    pub async fn create(
        db: &PgPool,
        owner_id: Uuid,
        job_type: JobType,
        picture_id: Option<Uuid>,
        config: serde_json::Value,
        idempotency_key: Option<&str>,
    ) -> Result<Job, AppError> {
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
                   result_s3_keys,
                   error_message,
                   retry_count, max_retries,
                   idempotency_key,
                   picture_id, claimed_by,
                   created_at, started_at, completed_at"#,
            owner_id,
            job_type as JobType,
            picture_id,
            config as serde_json::Value,
            idempotency_key,
        )
        .fetch_one(db)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn find_by_id(db: &PgPool, id: Uuid) -> Result<Option<Job>, AppError> {
        sqlx::query_as!(
            Job,
            r#"SELECT id, owner_id,
                      job_type    AS "job_type: JobType",
                      status      AS "status: JobStatus",
                      config      AS "config: _",
                      result      AS "result: _",
                      result_s3_keys,
                      error_message,
                      retry_count, max_retries,
                      idempotency_key,
                      picture_id, claimed_by,
                      created_at, started_at, completed_at
               FROM   jobs
               WHERE  id = $1"#,
            id,
        )
        .fetch_optional(db)
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
                      result_s3_keys,
                      error_message,
                      retry_count, max_retries,
                      idempotency_key,
                      picture_id, claimed_by,
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
}
