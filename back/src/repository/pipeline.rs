//! Pipeline-specific repository queries.
//!
//! These are kept separate from the general picture/tagging repositories because
//! they operate on a projection of `pictures` that the pipeline needs, and on
//! bulk tag-assignment logic specific to pipeline output.

use crate::infra::error::{AppError, map_sqlx_error};
use chrono::NaiveDateTime;
use sqlx::{Executor, PgPool, Postgres};
use std::collections::HashMap;
use uuid::Uuid;

/// Minimal picture projection used by the pipeline evaluator.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PipelinePicture {
    pub id: Uuid,
    pub captured_at: Option<NaiveDateTime>,
    pub gps_lat: Option<f64>,
    pub gps_lng: Option<f64>,
    pub filename: Option<String>,
}

/// A tag to assign as output of the pipeline, with its source.
pub struct PipelineTagAssignment {
    /// Ltree-formatted tag path (e.g. `Photos.Travel.Alps`).
    pub tag_path: String,
    /// Postgres `tag_source` enum value as a string (`"rule"`, `"segment"`, `"share_mapping"`).
    pub source: String,
    /// ID of the tagging service that produced this tag.
    pub source_id: Uuid,
}

pub struct PipelineRepository;

impl PipelineRepository {
    /// Return the IDs of users who have at least one enabled tagging service
    /// and at least one dirty picture (never processed, or processed before the
    /// service was last invalidated).
    pub async fn find_users_with_dirty_pictures(db: &PgPool) -> Result<Vec<Uuid>, AppError> {
        let rows = sqlx::query_scalar!(
            r#"SELECT DISTINCT p.local_user_id
               FROM pictures p
               WHERE p.deleted_at IS NULL
                 AND EXISTS (
                   SELECT 1 FROM tagging_services ts
                   WHERE ts.owner_id = p.local_user_id
                     AND ts.enabled = true
                     AND (
                       p.last_pipeline_run_at IS NULL
                       OR p.last_pipeline_run_at < ts.last_invalidated_at
                     )
                 )"#,
        )
        .fetch_all(db)
        .await
        .map_err(map_sqlx_error)?;
        Ok(rows)
    }

    /// Return all dirty pictures for a specific user.
    ///
    /// A picture is dirty if any enabled service for that user has a
    /// `last_invalidated_at` newer than the picture's `last_pipeline_run_at`.
    pub async fn find_dirty_for_user<'e, E>(
        ex: E,
        user_id: Uuid,
    ) -> Result<Vec<PipelinePicture>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as!(
            PipelinePicture,
            r#"SELECT p.id, p.captured_at, p.gps_lat, p.gps_lng, p.filename
               FROM pictures p
               WHERE p.local_user_id = $1
                 AND p.deleted_at IS NULL
                 AND EXISTS (
                   SELECT 1 FROM tagging_services ts
                   WHERE ts.owner_id = $1
                     AND ts.enabled = true
                     AND (
                       p.last_pipeline_run_at IS NULL
                       OR p.last_pipeline_run_at < ts.last_invalidated_at
                     )
                 )"#,
            user_id,
        )
        .fetch_all(ex)
        .await
        .map_err(map_sqlx_error)
    }

    /// Set `last_pipeline_run_at = run_at` on successfully processed pictures.
    pub async fn mark_run<'e, E>(
        ex: E,
        picture_ids: &[Uuid],
        run_at: NaiveDateTime,
    ) -> Result<(), AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        if picture_ids.is_empty() {
            return Ok(());
        }
        sqlx::query!(
            r#"UPDATE pictures SET last_pipeline_run_at = $2 WHERE id = ANY($1::uuid[])"#,
            picture_ids as &[Uuid],
            run_at,
        )
        .execute(ex)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    /// Reset `last_pipeline_run_at = NULL` on pictures that need re-evaluation.
    /// Called after manual tag changes.
    pub async fn invalidate<'e, E>(ex: E, picture_ids: &[Uuid]) -> Result<(), AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        if picture_ids.is_empty() {
            return Ok(());
        }
        sqlx::query!(
            r#"UPDATE pictures SET last_pipeline_run_at = NULL WHERE id = ANY($1::uuid[])"#,
            picture_ids as &[Uuid],
        )
        .execute(ex)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    /// For each picture in the batch, return the set of `incoming_share_id` values
    /// from tags with `source = 'incoming_share'`. Used by the SharedTagMapping evaluator.
    pub async fn find_incoming_share_ids<'e, E>(
        ex: E,
        picture_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<Uuid>>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        if picture_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows = sqlx::query!(
            r#"SELECT t.picture_id, t.source_id as "source_id!"
               FROM tags t
               WHERE t.picture_id = ANY($1::uuid[])
                 AND t.source = 'incoming_share'
                 AND t.source_id IS NOT NULL"#,
            picture_ids as &[Uuid],
        )
        .fetch_all(ex)
        .await
        .map_err(map_sqlx_error)?;

        let mut map: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
        for row in rows {
            map.entry(row.picture_id).or_default().push(row.source_id);
        }
        Ok(map)
    }

    /// Reconcile the pipeline tags of a single picture to `produced` in one atomic step.
    ///
    /// Pipeline tags (`rule`/`segment`/`share_mapping`) are live: this run's `produced` set is
    /// the complete desired pipeline output for the picture. Any stored pipeline tag not in it
    /// is removed (always-on removal — covers gating changes, edited rules, and tags left by
    /// now-disabled or deleted services), and the produced tags are inserted idempotently.
    ///
    /// `manual` and `incoming_share` tags are never touched. Because tags are stored per-source,
    /// each source owns its rows: removing one source's tag never disturbs another's, so no
    /// ancestor re-derivation is needed.
    ///
    /// Passing an empty `produced` is valid and clears all pipeline tags from the picture.
    pub async fn reconcile_pipeline_tags<'e, E>(
        ex: E,
        picture_id: Uuid,
        produced: &[PipelineTagAssignment],
    ) -> Result<(), AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let tags: Vec<&str> = produced.iter().map(|a| a.tag_path.as_str()).collect();
        let sources: Vec<&str> = produced.iter().map(|a| a.source.as_str()).collect();
        let source_ids: Vec<Uuid> = produced.iter().map(|a| a.source_id).collect();

        sqlx::query!(
            r#"WITH produced AS (
                 SELECT t.tag::ltree AS tag_path, t.src::tag_source AS source, t.sid AS source_id
                 FROM unnest($2::text[], $3::text[], $4::uuid[]) AS t(tag, src, sid)
               ),
               cleanup AS (
                 DELETE FROM tags
                 WHERE picture_id = $1
                   AND source IN ('rule'::tag_source, 'segment'::tag_source, 'share_mapping'::tag_source)
                   AND NOT EXISTS (
                     SELECT 1 FROM produced p
                     WHERE p.tag_path = tags.tag_path
                       AND p.source = tags.source
                       AND p.source_id = tags.source_id
                   )
               )
               INSERT INTO tags (picture_id, tag_path, source, source_id)
               SELECT $1, p.tag_path, p.source, p.source_id FROM produced p
               ON CONFLICT (picture_id, tag_path, source, source_id) WHERE source <> 'manual' DO NOTHING"#,
            picture_id,
            &tags as &[&str],
            &sources as &[&str],
            &source_ids as &[Uuid],
        )
        .execute(ex)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }
}
