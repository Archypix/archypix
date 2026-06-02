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
                         blurhash, gps_lat, gps_lng, gps_alt, orientation, thumbnails_generated_at"#,
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
                      blurhash, gps_lat, gps_lng, gps_alt, orientation, thumbnails_generated_at
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
                          p.blurhash, p.gps_lat, p.gps_lng, p.gps_alt, p.orientation, p.thumbnails_generated_at
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
                         blurhash, gps_lat, gps_lng, gps_alt, orientation, thumbnails_generated_at"#,
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
    ) -> Result<Picture, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as!(
            Picture,
            r#"UPDATE pictures
               SET width       = COALESCE($2, width),
                   height      = COALESCE($3, height),
                   captured_at = COALESCE($4, captured_at),
                   gps_lat     = COALESCE($5, gps_lat),
                   gps_lng     = COALESCE($6, gps_lng),
                   gps_alt     = COALESCE($7, gps_alt),
                   orientation = COALESCE($8, orientation),
                   blurhash    = COALESCE($9, blurhash),
                   exif_data   = CASE WHEN $10::jsonb IS NOT NULL
                                      THEN exif_data || $10::jsonb
                                      ELSE exif_data
                                 END,
                   thumbnails_generated_at = COALESCE(thumbnails_generated_at, now() AT TIME ZONE 'utc')
               WHERE id = $1
               RETURNING id, local_user_id, remote_picture_id, owner_username, owner_instance_domain,
                         filename, mime_type, file_size, width, height,
                         exif_data as "exif_data: _", metadata as "metadata: _",
                         deleted_at, captured_at, ingested_at, updated_at,
                         blurhash, gps_lat, gps_lng, gps_alt, orientation, thumbnails_generated_at"#,
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
        )
            .fetch_one(ex)
            .await
            .map_err(map_sqlx_error)
    }

    /// Mark that thumbnails have been generated for the first time.
    /// No-op if already set (COALESCE keeps the original timestamp).
    pub async fn set_thumbnails_generated<'e, E>(ex: E, id: Uuid) -> Result<(), AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query!(
            r#"UPDATE pictures
               SET thumbnails_generated_at = COALESCE(thumbnails_generated_at, now() AT TIME ZONE 'utc')
               WHERE id = $1"#,
            id,
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
    pub async fn apply_exif_overrides(
        db: &PgPool,
        id: Uuid,
        overrides: &ExifOverrides,
    ) -> Result<(), AppError> {
        // Build the JSONB patch for camera/lens fields stored inside exif_data.
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
        .execute(db)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }
}
