use crate::domain::tag::{Tag, TagSource};
use crate::infra::error::{AppError, map_sqlx_error};
use sqlx::{Executor, Postgres};
use uuid::Uuid;

pub struct TagRepository;

impl TagRepository {
    pub async fn list_paths_by_user<'e, E>(
        ex: E,
        local_user_id: Uuid,
    ) -> Result<Vec<String>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_scalar!(
            r#"SELECT DISTINCT t.tag_path::text
               FROM tags t
               JOIN pictures p ON p.id = t.picture_id
               WHERE p.local_user_id = $1 AND p.deleted_at IS NULL"#,
            local_user_id
        )
        .fetch_all(ex)
        .await
        .map_err(map_sqlx_error)
        .map(|rows| rows.into_iter().flatten().collect())
    }

    pub async fn assign<'e, E>(
        ex: E,
        picture_id: Uuid,
        tags: &[String],
    ) -> Result<Vec<Tag>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        if tags.is_empty() {
            return Ok(Vec::new());
        }
        sqlx::query_as!(
            Tag,
            r#"INSERT INTO tags (picture_id, tag_path, source)
               SELECT $1, unnest($2::text[])::ltree, 'manual'::tag_source
               ON CONFLICT (picture_id, tag_path) DO NOTHING
               RETURNING id, picture_id, tag_path::text as "tag_path!",
                         source as "source!: TagSource", source_id, assigned_at"#,
            picture_id,
            tags
        )
        .fetch_all(ex)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn remove<'e, E>(ex: E, picture_id: Uuid, tags: &[String]) -> Result<(), AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        if tags.is_empty() {
            return Ok(());
        }
        sqlx::query!(
            r#"DELETE FROM tags WHERE picture_id = $1 AND tag_path::text = ANY($2::text[])"#,
            picture_id,
            tags
        )
        .execute(ex)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }
}
