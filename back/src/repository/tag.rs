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

    /// List tags for a specific picture owned by the given user.
    pub async fn list_for_picture<'e, E>(
        ex: E,
        local_user_id: Uuid,
        picture_id: Uuid,
    ) -> Result<Vec<Tag>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as!(
            Tag,
            r#"SELECT t.id, t.picture_id, t.tag_path::text as "tag_path!",
                      t.source as "source!: TagSource", t.source_id, t.assigned_at
               FROM tags t
               JOIN pictures p ON p.id = t.picture_id
               WHERE t.picture_id = $1 AND p.local_user_id = $2 AND p.deleted_at IS NULL"#,
            picture_id,
            local_user_id,
        )
        .fetch_all(ex)
        .await
        .map_err(map_sqlx_error)
    }

    /// Load tags for a batch of pictures. Used by the pipeline loop to load current tags
    /// for all dirty pictures in one query rather than N per-picture queries.
    pub async fn list_for_pictures<'e, E>(ex: E, picture_ids: &[Uuid]) -> Result<Vec<Tag>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        if picture_ids.is_empty() {
            return Ok(vec![]);
        }
        sqlx::query_as!(
            Tag,
            r#"SELECT id, picture_id, tag_path::text as "tag_path!",
                      source as "source!: TagSource", source_id, assigned_at
               FROM tags
               WHERE picture_id = ANY($1::uuid[])"#,
            picture_ids as &[Uuid],
        )
        .fetch_all(ex)
        .await
        .map_err(map_sqlx_error)
    }

    /// Add tags to all pictures in the batch. All pictures must belong to `local_user_id`.
    ///
    /// Runs as a single data-modifying CTE:
    /// 1. Removes existing stored tags that are *proper ancestors* of any tag being added
    ///    (they become redundant once a deeper descendant is stored).
    /// 2. Inserts only the *deepest* tags from the input — any tag that is a proper ancestor of
    ///    another tag in the same input list is silently dropped.
    ///
    /// Must be called within a transaction together with `batch_remove` so that the overall
    /// remove-then-add is atomic.
    pub async fn batch_assign<'e, E>(
        ex: E,
        local_user_id: Uuid,
        picture_ids: &[Uuid],
        tags: &[String],
    ) -> Result<(), AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        if tags.is_empty() || picture_ids.is_empty() {
            return Ok(());
        }
        // Data-modifying CTE: cleanup (remove proper ancestors) + insert (only deepest).
        // `tag_path @> t AND tag_path <> t` = strict ancestor of t → remove it (redundant).
        // NOT EXISTS (deeper descendant) = this tag is the deepest in the input list.
        sqlx::query!(
            r#"WITH cleanup AS (
                 DELETE FROM tags
                 WHERE picture_id = ANY($1::uuid[])
                   AND tag_path @> ANY($3::ltree[])
                   AND NOT (tag_path = ANY($3::ltree[]))
                   AND source = 'manual'::tag_source
                   AND picture_id IN (
                     SELECT id FROM pictures WHERE local_user_id = $2 AND deleted_at IS NULL
                   )
               )
               INSERT INTO tags (picture_id, tag_path, source)
               SELECT p.id, filtered.tag::ltree, 'manual'::tag_source
               FROM (
                 SELECT id FROM pictures
                 WHERE id = ANY($1::uuid[]) AND local_user_id = $2 AND deleted_at IS NULL
               ) AS p
               CROSS JOIN (
                 SELECT t AS tag FROM unnest($3::text[]) AS t
                 WHERE NOT EXISTS (
                   SELECT 1 FROM unnest($3::text[]) AS deeper
                   WHERE deeper <> t AND deeper::ltree <@ t::ltree
                 )
               ) AS filtered
               ON CONFLICT (picture_id, tag_path) DO NOTHING"#,
            picture_ids as &[Uuid],
            local_user_id,
            tags as &[String],
        )
        .execute(ex)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    /// Remove tags (and all their subtags) from all pictures in the batch.
    /// All pictures must belong to `local_user_id`.
    ///
    /// Only removes `source = 'manual'` tags — system-assigned tags (`incoming_share`, `rule`, etc.)
    /// are never touched by user operations.
    pub async fn batch_remove<'e, E>(
        ex: E,
        local_user_id: Uuid,
        picture_ids: &[Uuid],
        tags: &[String],
    ) -> Result<(), AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        if tags.is_empty() || picture_ids.is_empty() {
            return Ok(());
        }
        sqlx::query!(
            r#"DELETE FROM tags
               WHERE picture_id = ANY($1::uuid[])
                 AND tag_path <@ ANY($2::ltree[])
                 AND source = 'manual'::tag_source
                 AND picture_id IN (
                   SELECT id FROM pictures WHERE local_user_id = $3 AND deleted_at IS NULL
                 )"#,
            picture_ids as &[Uuid],
            tags as &[String],
            local_user_id,
        )
        .execute(ex)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    /// Assign a `/SharedToMe/…` tag to a received picture, linked to the incoming share that
    /// created it. Used exclusively by the share-acceptance and picture-announcement flows.
    ///
    /// Uses ON CONFLICT DO NOTHING so re-announcing the same picture is idempotent.
    pub async fn assign_incoming_share_tag<'e, E>(
        ex: E,
        picture_id: Uuid,
        tag_path_ltree: &str,
        incoming_share_id: Uuid,
    ) -> Result<(), AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query!(
            r#"INSERT INTO tags (picture_id, tag_path, source, source_id)
               VALUES ($1, $2::text::ltree, 'incoming_share'::tag_source, $3)
               ON CONFLICT (picture_id, tag_path) DO NOTHING"#,
            picture_id,
            tag_path_ltree,
            incoming_share_id,
        )
        .execute(ex)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    /// Remove all `incoming_share` tags assigned by the given share.
    /// Called on share revocation to clean up all `/SharedToMe/…` entries for that share.
    pub async fn remove_incoming_share_tags<'e, E>(
        ex: E,
        incoming_share_id: Uuid,
    ) -> Result<(), AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query!(
            r#"DELETE FROM tags WHERE source = 'incoming_share'::tag_source AND source_id = $1"#,
            incoming_share_id,
        )
        .execute(ex)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;
    use uuid::Uuid;

    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

    async fn seed_user(db: &PgPool) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query!(
            "INSERT INTO users (id, username, email, display_name) VALUES ($1, $2, $3, $4)",
            id,
            format!("u_{}", &id.to_string()[..8]),
            format!("{}@t.com", id),
            "T",
        )
        .execute(db)
        .await
        .unwrap();
        id
    }

    async fn seed_picture(db: &PgPool, user_id: Uuid) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query!(
            "INSERT INTO pictures (id, local_user_id) VALUES ($1, $2)",
            id,
            user_id,
        )
        .execute(db)
        .await
        .unwrap();
        id
    }

    // ── batch_assign ──────────────────────────────────────────────────────────

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn batch_assign_adds_tags(db: PgPool) {
        let user = seed_user(&db).await;
        let pic = seed_picture(&db, user).await;

        TagRepository::batch_assign(&db, user, &[pic], &["Photos.Travel".to_string()])
            .await
            .unwrap();

        let tags = TagRepository::list_for_picture(&db, user, pic)
            .await
            .unwrap();
        assert!(tags.iter().any(|t| t.tag_path == "Photos.Travel"));
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn batch_assign_is_idempotent(db: PgPool) {
        let user = seed_user(&db).await;
        let pic = seed_picture(&db, user).await;

        let tags = vec!["Photos.Travel".to_string()];
        TagRepository::batch_assign(&db, user, &[pic], &tags)
            .await
            .unwrap();
        TagRepository::batch_assign(&db, user, &[pic], &tags)
            .await
            .unwrap();

        let stored = TagRepository::list_for_picture(&db, user, pic)
            .await
            .unwrap();
        let count = stored
            .iter()
            .filter(|t| t.tag_path == "Photos.Travel")
            .count();
        assert_eq!(count, 1, "idempotent — no duplicate");
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn batch_assign_prunes_ancestor_when_deeper_added(db: PgPool) {
        let user = seed_user(&db).await;
        let pic = seed_picture(&db, user).await;

        // Add parent first
        TagRepository::batch_assign(&db, user, &[pic], &["Photos".to_string()])
            .await
            .unwrap();
        // Then add a child — parent should be pruned (becomes redundant)
        TagRepository::batch_assign(&db, user, &[pic], &["Photos.Travel".to_string()])
            .await
            .unwrap();

        let stored = TagRepository::list_for_picture(&db, user, pic)
            .await
            .unwrap();
        assert!(
            !stored.iter().any(|t| t.tag_path == "Photos"),
            "ancestor pruned"
        );
        assert!(stored.iter().any(|t| t.tag_path == "Photos.Travel"));
    }

    // ── batch_remove ──────────────────────────────────────────────────────────

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn batch_remove_removes_tag_and_subtags(db: PgPool) {
        let user = seed_user(&db).await;
        let pic = seed_picture(&db, user).await;

        TagRepository::batch_assign(&db, user, &[pic], &["Photos.Travel.Alps".to_string()])
            .await
            .unwrap();
        // Remove at Photos level — Alps is a subtag so it should also be removed
        TagRepository::batch_remove(&db, user, &[pic], &["Photos".to_string()])
            .await
            .unwrap();

        let stored = TagRepository::list_for_picture(&db, user, pic)
            .await
            .unwrap();
        assert!(stored.is_empty(), "subtags removed");
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn batch_remove_removes_tag_and_keep_parents(db: PgPool) {
        let user = seed_user(&db).await;
        let pic = seed_picture(&db, user).await;

        TagRepository::batch_assign(
            &db,
            user,
            &[pic],
            &["Photos.Travel.Alps.Grenoble".to_string()],
        )
        .await
        .unwrap();
        // Currently, deleting a tag does not keep the parent tags.
        TagRepository::batch_remove(&db, user, &[pic], &["Photos.Travel.Alps".to_string()])
            .await
            .unwrap();

        let stored = TagRepository::list_for_picture(&db, user, pic)
            .await
            .unwrap();
        assert!(stored.is_empty(), "parent tags kept");
        //assert!(stored.iter().any(|t| t.tag_path == "Photos.Travel"));
    }

    // ── assign_incoming_share_tag / remove_incoming_share_tags ────────────────

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn assign_and_remove_incoming_share_tag(db: PgPool) {
        let user = seed_user(&db).await;
        let pic = seed_picture(&db, user).await;
        let share_id = Uuid::new_v4();

        TagRepository::assign_incoming_share_tag(
            &db,
            pic,
            "SharedToMe.alice_AT_ex_DOT_com.Photos",
            share_id,
        )
        .await
        .unwrap();

        let tags = TagRepository::list_for_picture(&db, user, pic)
            .await
            .unwrap();
        assert!(tags.iter().any(|t| t.source_id == Some(share_id)));

        TagRepository::remove_incoming_share_tags(&db, share_id)
            .await
            .unwrap();

        let tags = TagRepository::list_for_picture(&db, user, pic)
            .await
            .unwrap();
        assert!(tags.is_empty());
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn assign_incoming_share_tag_is_idempotent(db: PgPool) {
        let user = seed_user(&db).await;
        let pic = seed_picture(&db, user).await;
        let share_id = Uuid::new_v4();

        TagRepository::assign_incoming_share_tag(
            &db,
            pic,
            "SharedToMe.alice_AT_ex_DOT_com.Photos",
            share_id,
        )
        .await
        .unwrap();
        // Replay is idempotent (ON CONFLICT DO NOTHING)
        TagRepository::assign_incoming_share_tag(
            &db,
            pic,
            "SharedToMe.alice_AT_ex_DOT_com.Photos",
            share_id,
        )
        .await
        .unwrap();

        let tags = TagRepository::list_for_picture(&db, user, pic)
            .await
            .unwrap();
        assert_eq!(tags.len(), 1);
    }
}
