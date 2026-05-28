use crate::database::models::Tag;
use crate::infrastructure::error::{AppError, map_sqlx_error};
use sqlx::PgPool;
use uuid::Uuid;

pub struct TagRepository;

impl TagRepository {
    pub async fn list_by_owner(pool: &PgPool, owner_id: Uuid) -> Result<Vec<String>, AppError> {
        let tags = sqlx::query_scalar!(
            r#"
            SELECT DISTINCT t.tag_path::text
            FROM tags t
            JOIN pictures p ON p.id = t.picture_id
            WHERE p.owner_id = $1
              AND p.deleted_at IS NULL
            "#,
            owner_id
        )
        .fetch_all(pool)
        .await
        .map_err(map_sqlx_error)?
        .into_iter()
        .flatten()
        .collect();

        Ok(tags)
    }

    pub async fn assign_tags(
        pool: &PgPool,
        picture_id: Uuid,
        tags: &[String],
    ) -> Result<Vec<Tag>, AppError> {
        let mut assigned = Vec::new();
        for tag in tags {
            let tag = sqlx::query_as!(
                Tag,
                r#"
                INSERT INTO tags (picture_id, tag_path, is_virtual, source)
                VALUES ($1, $2::text::ltree, false, 'manual'::tag_source)
                ON CONFLICT (picture_id, tag_path) DO NOTHING
                RETURNING id, picture_id, tag_path::text as "tag_path!", is_virtual,
                          source::text as "source!", source_id, assigned_at
                "#,
                picture_id,
                tag
            )
            .fetch_optional(pool)
            .await
            .map_err(map_sqlx_error)?;

            if let Some(tag) = tag {
                assigned.push(tag);
            }
        }
        Ok(assigned)
    }

    pub async fn remove_tags(
        pool: &PgPool,
        picture_id: Uuid,
        tags: &[String],
    ) -> Result<(), AppError> {
        for tag in tags {
            sqlx::query!(
                r#"
                DELETE FROM tags
                WHERE picture_id = $1
                  AND tag_path = $2::text::ltree
                "#,
                picture_id,
                tag
            )
            .execute(pool)
            .await
            .map_err(map_sqlx_error)?;
        }
        Ok(())
    }
}
