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
                         deleted_at, captured_at, ingested_at, updated_at"#,
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
                      deleted_at, captured_at, ingested_at, updated_at
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
                          p.deleted_at, p.captured_at, p.ingested_at, p.updated_at
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
                         deleted_at, captured_at, ingested_at, updated_at"#,
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
}
