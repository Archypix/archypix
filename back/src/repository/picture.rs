use crate::domain::job::ExifOverrides;
use crate::domain::picture::Picture;
use crate::infra::error::{AppError, map_sqlx_error};
use chrono::NaiveDateTime;
use serde::Deserialize;
use sqlx::{Executor, PgPool, Postgres};
use uuid::Uuid;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PictureSortField {
    CapturedAt,
    #[default]
    IngestedAt,
    UpdatedAt,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    Asc,
    #[default]
    Desc,
}

#[derive(Debug, Clone)]
pub struct PictureListFilter {
    pub page: i64,
    pub page_size: i64,
    pub sort: PictureSortField,
    pub order: SortOrder,
    pub tag: Option<String>,
    pub owned_only: bool,
    pub shared_with_me: bool,
    pub include_deleted: bool,
    pub captured_after: Option<NaiveDateTime>,
    pub captured_before: Option<NaiveDateTime>,
}

pub struct PictureRepository;

impl PictureRepository {
    pub async fn create<'e, E>(
        ex: E,
        id: Uuid,
        local_user_id: Uuid,
        filename: Option<&str>,
        mime_type: Option<&str>,
        file_size: Option<i64>,
        width: Option<i32>,
        height: Option<i32>,
        exif_data: Option<serde_json::Value>,
        captured_at: Option<NaiveDateTime>,
    ) -> Result<Picture, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let exif_json = exif_data.unwrap_or_else(|| serde_json::json!({}));
        sqlx::query_as!(
            Picture,
            r#"INSERT INTO pictures (id, local_user_id, filename, mime_type, file_size, width, height, exif_data, metadata, captured_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, '{}'::jsonb, $9)
               RETURNING id, local_user_id, remote_picture_id, owner_username, owner_instance_domain,
                         filename, mime_type, file_size, width, height,
                         exif_data as "exif_data: _", metadata as "metadata: _",
                         deleted_at, captured_at, ingested_at, updated_at,
                         blurhash, gps_lat, gps_lng, gps_alt, orientation, thumbnails_generated_at,
                         file_hash"#,
            id,
            local_user_id,
            filename,
            mime_type,
            file_size,
            width,
            height,
            serde_json::Value::from(exif_json) as serde_json::Value,
            captured_at,
        )
            .fetch_one(ex)
            .await
            .map_err(map_sqlx_error)
    }

    /// Create a received (non-owned) picture row on behalf of a recipient user.
    ///
    /// `remote_picture_id` is the sender's picture UUID (stored as string for cross-instance compat).
    /// Deduplication is handled by the `uq_received_picture` unique index: on conflict the existing
    /// row is returned unchanged.
    pub async fn create_received<'e, E>(
        ex: E,
        recipient_id: Uuid,
        remote_picture_id: &str,
        owner_username: &str,
        owner_instance_domain: &str,
        filename: Option<&str>,
        mime_type: Option<&str>,
        file_size: Option<i64>,
        width: Option<i32>,
        height: Option<i32>,
        captured_at: Option<NaiveDateTime>,
    ) -> Result<Picture, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as!(
            Picture,
            r#"INSERT INTO pictures
                   (local_user_id, remote_picture_id, owner_username, owner_instance_domain,
                    filename, mime_type, file_size, width, height, exif_data, metadata, captured_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, '{}'::jsonb, '{}'::jsonb, $10)
               ON CONFLICT (local_user_id, remote_picture_id)
               WHERE remote_picture_id IS NOT NULL
               DO UPDATE SET
                   filename  = COALESCE(EXCLUDED.filename,  pictures.filename),
                   mime_type = COALESCE(EXCLUDED.mime_type, pictures.mime_type),
                   file_size = COALESCE(EXCLUDED.file_size, pictures.file_size),
                   width     = COALESCE(EXCLUDED.width,     pictures.width),
                   height    = COALESCE(EXCLUDED.height,    pictures.height),
                   captured_at = COALESCE(EXCLUDED.captured_at, pictures.captured_at)
               RETURNING id, local_user_id, remote_picture_id, owner_username, owner_instance_domain,
                         filename, mime_type, file_size, width, height,
                         exif_data as "exif_data: _", metadata as "metadata: _",
                         deleted_at, captured_at, ingested_at, updated_at,
                         blurhash, gps_lat, gps_lng, gps_alt, orientation, thumbnails_generated_at,
                         file_hash"#,
            recipient_id,
            remote_picture_id,
            owner_username,
            owner_instance_domain,
            filename,
            mime_type,
            file_size,
            width,
            height,
            captured_at,
        )
            .fetch_one(ex)
            .await
            .map_err(map_sqlx_error)
    }

    /// Delete received-picture rows from `sender` for `recipient_id` that have no remaining
    /// `incoming_share` tags.
    ///
    /// Called after `TagRepository::remove_incoming_share_tags` during share revocation.
    ///
    /// A revoked picture is unreachable regardless of any local tags Bob may have added —
    /// the sender's presign endpoint will reject requests once the share token is invalid.
    /// Manual tags are therefore not a reason to keep the row.
    ///
    /// Pictures received from the same sender via a *different, still-active* share survive:
    /// they retain `incoming_share` tags from that other share, so the `NOT EXISTS` check
    /// excludes them.
    ///
    /// Returns the number of deleted rows.
    pub async fn delete_received_without_share_tags<'e, E>(
        ex: E,
        recipient_id: Uuid,
        sender_username: &str,
        sender_instance: &str,
    ) -> Result<u64, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let result = sqlx::query!(
            r#"DELETE FROM pictures
               WHERE local_user_id = $1
                 AND owner_username = $2
                 AND owner_instance_domain = $3
                 AND remote_picture_id IS NOT NULL
                 AND NOT EXISTS (
                     SELECT 1 FROM tags
                     WHERE tags.picture_id = pictures.id
                       AND tags.source = 'incoming_share'::tag_source
                 )"#,
            recipient_id,
            sender_username,
            sender_instance,
        )
        .execute(ex)
        .await
        .map_err(map_sqlx_error)?;
        Ok(result.rows_affected())
    }

    /// From `candidate_picture_ids`, return those that still carry at least one
    /// `incoming_share` source tag (i.e. survived a share's tag cleanup). Used by
    /// `cleanup_incoming_share` to mark survivors dirty for token refresh.
    pub async fn find_with_any_incoming_share_tag<'e, E>(
        ex: E,
        recipient_id: Uuid,
        candidate_picture_ids: &[Uuid],
    ) -> Result<Vec<Uuid>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        if candidate_picture_ids.is_empty() {
            return Ok(vec![]);
        }
        sqlx::query_scalar!(
            r#"SELECT DISTINCT p.id
               FROM pictures p
               JOIN tags t ON t.picture_id = p.id
               WHERE p.id = ANY($1::uuid[])
                 AND p.local_user_id = $2
                 AND t.source = 'incoming_share'::tag_source"#,
            candidate_picture_ids as &[Uuid],
            recipient_id,
        )
        .fetch_all(ex)
        .await
        .map_err(map_sqlx_error)
    }

    /// Map a set of `remote_picture_id` strings to the recipient's local picture ids.
    /// Used by per-picture unannounce to resolve the sender's announce ids locally.
    pub async fn find_ids_by_remote_ids<'e, E>(
        ex: E,
        recipient_id: Uuid,
        remote_ids: &[String],
    ) -> Result<Vec<Uuid>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        if remote_ids.is_empty() {
            return Ok(vec![]);
        }
        sqlx::query_scalar!(
            r#"SELECT id FROM pictures
               WHERE local_user_id = $1
                 AND remote_picture_id = ANY($2::text[])"#,
            recipient_id,
            remote_ids as &[String],
        )
        .fetch_all(ex)
        .await
        .map_err(map_sqlx_error)
    }

    /// Delete the received pictures in `picture_ids` that have no remaining `incoming_share`
    /// tag. Returns the deleted ids. Used by per-picture unannounce.
    pub async fn delete_orphans_among<'e, E>(
        ex: E,
        recipient_id: Uuid,
        picture_ids: &[Uuid],
    ) -> Result<Vec<Uuid>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        if picture_ids.is_empty() {
            return Ok(vec![]);
        }
        sqlx::query_scalar!(
            r#"DELETE FROM pictures
               WHERE id = ANY($1::uuid[])
                 AND local_user_id = $2
                 AND remote_picture_id IS NOT NULL
                 AND NOT EXISTS (
                     SELECT 1 FROM tags
                     WHERE tags.picture_id = pictures.id
                       AND tags.source = 'incoming_share'::tag_source
                 )
               RETURNING id"#,
            picture_ids as &[Uuid],
            recipient_id,
        )
        .fetch_all(ex)
        .await
        .map_err(map_sqlx_error)
    }

    /// List all active owned pictures that carry a tag under `tag_path_ltree` (inclusive).
    ///
    /// Used by Alice's backend to enumerate pictures to announce when a share is accepted.
    pub async fn list_by_tag_and_owner<'e, E>(
        ex: E,
        owner_id: Uuid,
        tag_path_ltree: &str,
    ) -> Result<Vec<Picture>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as!(
            Picture,
            r#"SELECT DISTINCT p.id, p.local_user_id, p.remote_picture_id, p.owner_username,
                      p.owner_instance_domain, p.filename, p.mime_type, p.file_size,
                      p.width, p.height, p.exif_data as "exif_data: _", p.metadata as "metadata: _",
                      p.deleted_at, p.captured_at, p.ingested_at, p.updated_at,
                      p.blurhash, p.gps_lat, p.gps_lng, p.gps_alt, p.orientation,
                      p.thumbnails_generated_at, p.file_hash
               FROM pictures p
               JOIN tags t ON t.picture_id = p.id
               WHERE p.local_user_id = $1
                 AND p.remote_picture_id IS NULL
                 AND p.deleted_at IS NULL
                 AND t.tag_path <@ $2::text::ltree"#,
            owner_id,
            tag_path_ltree,
        )
        .fetch_all(ex)
        .await
        .map_err(map_sqlx_error)
    }

    /// Load a batch of picture rows by id (order unspecified). Used by the pipeline
    /// announcement step to build announcement payloads for the pictures it announces.
    pub async fn list_by_ids<'e, E>(ex: E, ids: &[Uuid]) -> Result<Vec<Picture>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        sqlx::query_as!(
            Picture,
            r#"SELECT id, local_user_id, remote_picture_id, owner_username, owner_instance_domain,
                      filename, mime_type, file_size, width, height,
                      exif_data as "exif_data: _", metadata as "metadata: _",
                      deleted_at, captured_at, ingested_at, updated_at,
                      blurhash, gps_lat, gps_lng, gps_alt, orientation, thumbnails_generated_at,
                      file_hash
               FROM pictures WHERE id = ANY($1::uuid[])"#,
            ids as &[Uuid],
        )
        .fetch_all(ex)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn find_by_id<'e, E>(ex: E, id: Uuid) -> Result<Option<Picture>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as!(
            Picture,
            r#"SELECT id, local_user_id, remote_picture_id, owner_username, owner_instance_domain,
                      filename, mime_type, file_size, width, height,
                      exif_data as "exif_data: _", metadata as "metadata: _",
                      deleted_at, captured_at, ingested_at, updated_at,
                      blurhash, gps_lat, gps_lng, gps_alt, orientation, thumbnails_generated_at,
                      file_hash
               FROM pictures WHERE id = $1"#,
            id
        )
        .fetch_optional(ex)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn list(
        db: &PgPool,
        local_user_id: Uuid,
        filter: &PictureListFilter,
    ) -> Result<(Vec<Picture>, i64), AppError> {
        let sort_col = match filter.sort {
            PictureSortField::CapturedAt => "p.captured_at",
            PictureSortField::IngestedAt => "p.ingested_at",
            PictureSortField::UpdatedAt => "p.updated_at",
        };
        let sort_dir = match filter.order {
            SortOrder::Asc => "ASC",
            SortOrder::Desc => "DESC",
        };
        let offset = (filter.page - 1) * filter.page_size;

        let total: i64 = {
            let mut q = sqlx::QueryBuilder::<Postgres>::new(
                "SELECT COUNT(*) FROM pictures p WHERE p.local_user_id = ",
            );
            q.push_bind(local_user_id);
            Self::push_filters(&mut q, filter);
            q.build_query_scalar()
                .fetch_one(db)
                .await
                .map_err(map_sqlx_error)?
        };

        let items: Vec<Picture> = {
            let mut q = sqlx::QueryBuilder::<Postgres>::new(
                r#"SELECT p.id, p.local_user_id, p.remote_picture_id, p.owner_username,
                          p.owner_instance_domain, p.filename, p.mime_type, p.file_size,
                          p.width, p.height, p.exif_data, p.metadata,
                          p.deleted_at, p.captured_at, p.ingested_at, p.updated_at,
                          p.blurhash, p.gps_lat, p.gps_lng, p.gps_alt, p.orientation,
                          p.thumbnails_generated_at, p.file_hash
                   FROM pictures p WHERE p.local_user_id = "#,
            );
            q.push_bind(local_user_id);
            Self::push_filters(&mut q, filter);
            q.push(format!(" ORDER BY {} {} LIMIT ", sort_col, sort_dir));
            q.push_bind(filter.page_size);
            q.push(" OFFSET ");
            q.push_bind(offset);
            q.build_query_as()
                .fetch_all(db)
                .await
                .map_err(map_sqlx_error)?
        };

        Ok((items, total))
    }

    fn push_filters(q: &mut sqlx::QueryBuilder<Postgres>, filter: &PictureListFilter) {
        if !filter.include_deleted {
            q.push(" AND p.deleted_at IS NULL");
        }
        if filter.owned_only {
            q.push(" AND p.remote_picture_id IS NULL");
        }
        if filter.shared_with_me {
            q.push(" AND p.remote_picture_id IS NOT NULL");
        }
        if let Some(v) = filter.captured_after {
            q.push(" AND p.captured_at >= ").push_bind(v);
        }
        if let Some(v) = filter.captured_before {
            q.push(" AND p.captured_at <= ").push_bind(v);
        }
        if let Some(ref tag) = filter.tag {
            q.push(
                " AND EXISTS (SELECT 1 FROM tags t WHERE t.picture_id = p.id AND t.tag_path <@ ",
            )
            .push_bind(tag.clone())
            .push("::ltree)");
        }
    }

    pub async fn update_metadata<'e, E>(
        ex: E,
        id: Uuid,
        mime_type: Option<&str>,
        file_size: Option<i64>,
        width: Option<i32>,
        height: Option<i32>,
        exif_data: Option<serde_json::Value>,
        captured_at: Option<NaiveDateTime>,
    ) -> Result<Picture, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as!(
            Picture,
            r#"UPDATE pictures
               SET mime_type = COALESCE($2, mime_type),
                   file_size = COALESCE($3, file_size),
                   width = COALESCE($4, width),
                   height = COALESCE($5, height),
                   exif_data = COALESCE($6, exif_data),
                   captured_at = COALESCE($7, captured_at)
               WHERE id = $1
               RETURNING id, local_user_id, remote_picture_id, owner_username, owner_instance_domain,
                         filename, mime_type, file_size, width, height,
                         exif_data as "exif_data: _", metadata as "metadata: _",
                         deleted_at, captured_at, ingested_at, updated_at,
                         blurhash, gps_lat, gps_lng, gps_alt, orientation, thumbnails_generated_at,
                         file_hash"#,
            id,
            mime_type,
            file_size,
            width,
            height,
            exif_data as Option<serde_json::Value>,
            captured_at,
        )
            .fetch_one(ex)
        .await
        .map_err(map_sqlx_error)
    }

    /// Update a picture's worker-extracted data after initial thumbnail generation.
    /// Only updates fields that the worker provides (COALESCE keeps existing values).
    pub async fn update_from_worker<'e, E>(
        ex: E,
        id: Uuid,
        width: Option<i32>,
        height: Option<i32>,
        captured_at: Option<NaiveDateTime>,
        gps_lat: Option<f64>,
        gps_lng: Option<f64>,
        gps_alt: Option<i32>,
        orientation: Option<i16>,
        blurhash: Option<&str>,
        exif_data_patch: Option<serde_json::Value>,
        file_size: Option<i64>,
        file_hash: Option<&str>,
    ) -> Result<Picture, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as!(
            Picture,
            r#"UPDATE pictures
               SET width       = COALESCE($2,  width),
                   height      = COALESCE($3,  height),
                   captured_at = COALESCE($4,  captured_at),
                   gps_lat     = COALESCE($5,  gps_lat),
                   gps_lng     = COALESCE($6,  gps_lng),
                   gps_alt     = COALESCE($7,  gps_alt),
                   orientation = COALESCE($8,  orientation),
                   blurhash    = COALESCE($9,  blurhash),
                   exif_data   = CASE WHEN $10::jsonb IS NOT NULL
                                      THEN exif_data || $10::jsonb
                                      ELSE exif_data
                                 END,
                   file_size   = COALESCE($11, file_size),
                   file_hash   = COALESCE($12, file_hash),
                   thumbnails_generated_at = COALESCE(thumbnails_generated_at, now() AT TIME ZONE 'utc')
               WHERE id = $1
               RETURNING id, local_user_id, remote_picture_id, owner_username, owner_instance_domain,
                         filename, mime_type, file_size, width, height,
                         exif_data as "exif_data: _", metadata as "metadata: _",
                         deleted_at, captured_at, ingested_at, updated_at,
                         blurhash, gps_lat, gps_lng, gps_alt, orientation, thumbnails_generated_at,
                         file_hash"#,
            id,
            width,
            height,
            captured_at,
            gps_lat,
            gps_lng,
            gps_alt,
            orientation,
            blurhash,
            exif_data_patch as Option<serde_json::Value>,
            file_size,
            file_hash,
        )
            .fetch_one(ex)
            .await
            .map_err(map_sqlx_error)
    }

    /// Update picture metadata set by the worker after any job completes, for cases
    /// where no EXIF is returned (edit_picture, non-initial gen_thumbnail).
    ///
    /// `set_thumbnails` controls whether `thumbnails_generated_at` is stamped; the
    /// other fields are always applied via COALESCE (existing value kept when `None`).
    pub async fn update_after_processing<'e, E>(
        ex: E,
        id: Uuid,
        set_thumbnails: bool,
        blurhash: Option<&str>,
        file_size: Option<i64>,
        file_hash: Option<&str>,
    ) -> Result<(), AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query!(
            r#"UPDATE pictures
               SET thumbnails_generated_at = CASE WHEN $2
                                                  THEN COALESCE(thumbnails_generated_at, now() AT TIME ZONE 'utc')
                                                  ELSE thumbnails_generated_at
                                             END,
                   blurhash  = COALESCE($3, blurhash),
                   file_size = COALESCE($4, file_size),
                   file_hash = COALESCE($5, file_hash)
               WHERE id = $1"#,
            id,
            set_thumbnails,
            blurhash,
            file_size,
            file_hash,
        )
            .execute(ex)
            .await
            .map_err(map_sqlx_error)?;
        Ok(())
    }

    /// Apply user-requested EXIF overrides to the picture row.
    ///
    /// Only non-`None` fields in `overrides` are written; existing values are kept for
    /// fields left as `None`. Camera/lens metadata (brand, model, focal length, etc.) are
    /// merged into the `exif_data` JSONB column.
    pub async fn apply_exif_overrides<'e, E>(
        ex: E,
        id: Uuid,
        overrides: &ExifOverrides,
    ) -> Result<(), AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let mut patch = serde_json::Map::new();
        if let Some(ref v) = overrides.camera_brand {
            patch.insert("camera_brand".to_string(), serde_json::json!(v));
        }
        if let Some(ref v) = overrides.camera_model {
            patch.insert("camera_model".to_string(), serde_json::json!(v));
        }
        if let Some(v) = overrides.focal_length_mm {
            patch.insert("focal_length_mm".to_string(), serde_json::json!(v));
        }
        if let Some(v) = overrides.f_number {
            patch.insert("f_number".to_string(), serde_json::json!(v));
        }
        if let Some(v) = overrides.iso_speed {
            patch.insert("iso_speed".to_string(), serde_json::json!(v));
        }
        if let Some(v) = overrides.exposure_time_num {
            patch.insert("exposure_time_num".to_string(), serde_json::json!(v));
        }
        if let Some(v) = overrides.exposure_time_den {
            patch.insert("exposure_time_den".to_string(), serde_json::json!(v));
        }
        let exif_patch = if patch.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(patch))
        };

        sqlx::query!(
            r#"UPDATE pictures
               SET captured_at = COALESCE($2, captured_at),
                   gps_lat     = COALESCE($3, gps_lat),
                   gps_lng     = COALESCE($4, gps_lng),
                   gps_alt     = COALESCE($5, gps_alt),
                   orientation = COALESCE($6, orientation),
                   exif_data   = CASE WHEN $7::jsonb IS NOT NULL
                                      THEN exif_data || $7::jsonb
                                      ELSE exif_data
                                 END
               WHERE id = $1"#,
            id,
            overrides.captured_at,
            overrides.gps_lat,
            overrides.gps_lng,
            overrides.gps_alt,
            overrides.orientation,
            exif_patch as Option<serde_json::Value>,
        )
        .execute(ex)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }
}
