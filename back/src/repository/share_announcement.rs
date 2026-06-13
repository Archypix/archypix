//! Sender-side tracking of per-picture presign tokens.
//!
//! Every `(outgoing_share, picture)` pair that has been announced to a recipient has one
//! row here, carrying a unique `picture_token`. The presign endpoint resolves a token
//! directly to a picture; the pipeline announcement step diffs current share coverage
//! against this table to decide what to announce / unannounce; revoking a share deletes
//! its rows (immediately invalidating every token it held).

use crate::infra::error::{AppError, map_sqlx_error};
use sqlx::{Executor, Postgres};
use uuid::Uuid;

/// A downstream recipient that must be told a now-deleted picture is unannounced.
#[derive(Debug, Clone)]
pub struct DownstreamAnnouncement {
    pub outgoing_share_id: Uuid,
    /// The id the downstream recipient stored as `remote_picture_id` (original owner's id).
    pub announce_id: String,
    pub recipient_username: String,
    pub recipient_instance: String,
}

pub struct ShareAnnouncementRepository;

impl ShareAnnouncementRepository {
    /// Insert a tracking row for one picture, generating its token. Idempotent: if the row
    /// already exists the existing token is returned unchanged.
    pub async fn insert<'e, E>(
        ex: E,
        outgoing_share_id: Uuid,
        picture_id: Uuid,
    ) -> Result<Uuid, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_scalar!(
            r#"INSERT INTO share_announcements (outgoing_share_id, picture_id)
               VALUES ($1, $2)
               ON CONFLICT (outgoing_share_id, picture_id) DO UPDATE
                   SET picture_token = share_announcements.picture_token
               RETURNING picture_token"#,
            outgoing_share_id,
            picture_id,
        )
        .fetch_one(ex)
        .await
        .map_err(map_sqlx_error)
    }

    /// Insert a tracking row with an explicit token (used for received/transitive pictures,
    /// where the forwarded token must equal the upstream `incoming_share` tag token). Idempotent
    /// on the `(share, picture)` key; on conflict the token is refreshed to `picture_token`.
    pub async fn insert_with_token<'e, E>(
        ex: E,
        outgoing_share_id: Uuid,
        picture_id: Uuid,
        picture_token: Uuid,
    ) -> Result<(), AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query!(
            r#"INSERT INTO share_announcements (outgoing_share_id, picture_id, picture_token)
               VALUES ($1, $2, $3)
               ON CONFLICT (outgoing_share_id, picture_id)
               DO UPDATE SET picture_token = EXCLUDED.picture_token"#,
            outgoing_share_id,
            picture_id,
            picture_token,
        )
        .execute(ex)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    /// Current coverage for a set of dirty pictures: the `(picture_id, outgoing_share_id)` pairs
    /// where a picture tag is at-or-under an active `future` share's tag. Loop prevention is
    /// applied inline — pictures whose original owner is the share recipient are excluded.
    pub async fn current_coverage<'e, E>(
        ex: E,
        owner_id: Uuid,
        picture_ids: &[Uuid],
    ) -> Result<Vec<(Uuid, Uuid)>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        if picture_ids.is_empty() {
            return Ok(vec![]);
        }
        let rows = sqlx::query!(
            r#"SELECT DISTINCT t.picture_id, os.id AS outgoing_share_id
               FROM tags t
               JOIN pictures p ON p.id = t.picture_id
               JOIN outgoing_shares os ON t.tag_path <@ os.tag_path
               WHERE t.picture_id = ANY($1::uuid[])
                 AND p.local_user_id = $2
                 AND p.deleted_at IS NULL
                 AND os.owner_id = $2
                 AND os.status = 'active'::share_status
                 AND os.future = true
                 AND (
                       p.owner_username IS DISTINCT FROM os.recipient_username
                    OR p.owner_instance_domain IS DISTINCT FROM os.recipient_instance
                 )"#,
            picture_ids as &[Uuid],
            owner_id,
        )
        .fetch_all(ex)
        .await
        .map_err(map_sqlx_error)?;
        Ok(rows
            .into_iter()
            .map(|r| (r.picture_id, r.outgoing_share_id))
            .collect())
    }

    /// Load the tracking rows for a set of dirty pictures across the given shares.
    /// Returns `(outgoing_share_id, picture_id, picture_token)`.
    pub async fn find_tracking_for_pictures<'e, E>(
        ex: E,
        share_ids: &[Uuid],
        picture_ids: &[Uuid],
    ) -> Result<Vec<(Uuid, Uuid, Uuid)>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        if share_ids.is_empty() || picture_ids.is_empty() {
            return Ok(vec![]);
        }
        let rows = sqlx::query!(
            r#"SELECT outgoing_share_id, picture_id, picture_token
               FROM share_announcements
               WHERE picture_id = ANY($1::uuid[])
                 AND outgoing_share_id = ANY($2::uuid[])"#,
            picture_ids as &[Uuid],
            share_ids as &[Uuid],
        )
        .fetch_all(ex)
        .await
        .map_err(map_sqlx_error)?;
        Ok(rows
            .into_iter()
            .map(|r| (r.outgoing_share_id, r.picture_id, r.picture_token))
            .collect())
    }

    /// Update the token of an existing tracking row (token-refresh path).
    pub async fn update_token<'e, E>(
        ex: E,
        outgoing_share_id: Uuid,
        picture_id: Uuid,
        new_token: Uuid,
    ) -> Result<(), AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query!(
            r#"UPDATE share_announcements SET picture_token = $3
               WHERE outgoing_share_id = $1 AND picture_id = $2"#,
            outgoing_share_id,
            picture_id,
            new_token,
        )
        .execute(ex)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    /// Delete the tracking row for one `(share, picture)` pair (picture left coverage).
    pub async fn delete<'e, E>(
        ex: E,
        outgoing_share_id: Uuid,
        picture_id: Uuid,
    ) -> Result<(), AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query!(
            r#"DELETE FROM share_announcements
               WHERE outgoing_share_id = $1 AND picture_id = $2"#,
            outgoing_share_id,
            picture_id,
        )
        .execute(ex)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    /// Delete all tracking rows for a set of pictures, across every share. Used by
    /// `cleanup_incoming_share` after the downstream unannounce tasks are enqueued.
    pub async fn delete_for_pictures<'e, E>(ex: E, picture_ids: &[Uuid]) -> Result<(), AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        if picture_ids.is_empty() {
            return Ok(());
        }
        sqlx::query!(
            r#"DELETE FROM share_announcements WHERE picture_id = ANY($1::uuid[])"#,
            picture_ids as &[Uuid],
        )
        .execute(ex)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    /// Delete every tracking row for a share. Used by `revoke_outgoing_share` — all of the
    /// share's tokens die at once.
    pub async fn delete_all_for_share<'e, E>(ex: E, outgoing_share_id: Uuid) -> Result<(), AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query!(
            r#"DELETE FROM share_announcements WHERE outgoing_share_id = $1"#,
            outgoing_share_id,
        )
        .execute(ex)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    /// Resolve a `picture_token` to the *owned* picture it grants access to. The presign
    /// endpoint's only authorization check: an unknown token (or one that only matches a relayed,
    /// non-owned tracking row) yields `None`. Filtering to owned pictures disambiguates the case
    /// where a relayer copied an upstream token into its own tracking row on the same instance.
    pub async fn find_picture_by_token<'e, E>(
        ex: E,
        picture_token: Uuid,
    ) -> Result<Option<Uuid>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_scalar!(
            r#"SELECT sa.picture_id
               FROM share_announcements sa
               JOIN pictures p ON p.id = sa.picture_id
               WHERE sa.picture_token = $1
                 AND p.remote_picture_id IS NULL
               LIMIT 1"#,
            picture_token,
        )
        .fetch_optional(ex)
        .await
        .map_err(map_sqlx_error)
    }

    /// Find downstream recipients of a set of deleted pictures, so `cleanup_incoming_share`
    /// can enqueue `UnannounceSharedPictures` tasks before deleting the tracking rows.
    pub async fn find_downstream_for_pictures<'e, E>(
        ex: E,
        picture_ids: &[Uuid],
    ) -> Result<Vec<DownstreamAnnouncement>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        if picture_ids.is_empty() {
            return Ok(vec![]);
        }
        // Must be called *before* the picture rows are deleted so `remote_picture_id` is
        // available; falls back to the local id text for owned pictures (remote_picture_id NULL).
        let rows = sqlx::query!(
            r#"SELECT sa.outgoing_share_id,
                      COALESCE(p.remote_picture_id, sa.picture_id::text) AS "announce_id!",
                      os.recipient_username, os.recipient_instance
               FROM share_announcements sa
               JOIN outgoing_shares os ON os.id = sa.outgoing_share_id
               LEFT JOIN pictures p ON p.id = sa.picture_id
               WHERE sa.picture_id = ANY($1::uuid[])"#,
            picture_ids as &[Uuid],
        )
        .fetch_all(ex)
        .await
        .map_err(map_sqlx_error)?;

        Ok(rows
            .into_iter()
            .map(|r| DownstreamAnnouncement {
                outgoing_share_id: r.outgoing_share_id,
                announce_id: r.announce_id,
                recipient_username: r.recipient_username,
                recipient_instance: r.recipient_instance,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::share::OutgoingShareRepository;
    use sqlx::PgPool;

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

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn insert_then_resolve_token(db: PgPool) {
        let owner = seed_user(&db).await;
        let pic = seed_picture(&db, owner).await;
        let share =
            OutgoingShareRepository::create(&db, owner, "Photos", "bob", "other.com", true, true)
                .await
                .unwrap();

        let token = ShareAnnouncementRepository::insert(&db, share.id, pic)
            .await
            .unwrap();
        let resolved = ShareAnnouncementRepository::find_picture_by_token(&db, token)
            .await
            .unwrap();
        assert_eq!(resolved, Some(pic));
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn insert_is_idempotent_keeps_token(db: PgPool) {
        let owner = seed_user(&db).await;
        let pic = seed_picture(&db, owner).await;
        let share =
            OutgoingShareRepository::create(&db, owner, "Photos", "bob", "other.com", true, true)
                .await
                .unwrap();

        let t1 = ShareAnnouncementRepository::insert(&db, share.id, pic)
            .await
            .unwrap();
        let t2 = ShareAnnouncementRepository::insert(&db, share.id, pic)
            .await
            .unwrap();
        assert_eq!(t1, t2, "token stable across re-insert");
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn delete_invalidates_token(db: PgPool) {
        let owner = seed_user(&db).await;
        let pic = seed_picture(&db, owner).await;
        let share =
            OutgoingShareRepository::create(&db, owner, "Photos", "bob", "other.com", true, true)
                .await
                .unwrap();

        let token = ShareAnnouncementRepository::insert(&db, share.id, pic)
            .await
            .unwrap();
        ShareAnnouncementRepository::delete(&db, share.id, pic)
            .await
            .unwrap();
        let resolved = ShareAnnouncementRepository::find_picture_by_token(&db, token)
            .await
            .unwrap();
        assert_eq!(resolved, None);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn delete_all_for_share_clears_every_token(db: PgPool) {
        let owner = seed_user(&db).await;
        let p1 = seed_picture(&db, owner).await;
        let p2 = seed_picture(&db, owner).await;
        let share =
            OutgoingShareRepository::create(&db, owner, "Photos", "bob", "other.com", true, true)
                .await
                .unwrap();

        let t1 = ShareAnnouncementRepository::insert(&db, share.id, p1)
            .await
            .unwrap();
        let t2 = ShareAnnouncementRepository::insert(&db, share.id, p2)
            .await
            .unwrap();
        ShareAnnouncementRepository::delete_all_for_share(&db, share.id)
            .await
            .unwrap();
        assert_eq!(
            ShareAnnouncementRepository::find_picture_by_token(&db, t1)
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            ShareAnnouncementRepository::find_picture_by_token(&db, t2)
                .await
                .unwrap(),
            None
        );
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn find_downstream_for_pictures_returns_recipients(db: PgPool) {
        let owner = seed_user(&db).await;
        let pic = seed_picture(&db, owner).await;
        let share =
            OutgoingShareRepository::create(&db, owner, "Photos", "carol", "carol.com", true, true)
                .await
                .unwrap();
        ShareAnnouncementRepository::insert(&db, share.id, pic)
            .await
            .unwrap();

        let downstream = ShareAnnouncementRepository::find_downstream_for_pictures(&db, &[pic])
            .await
            .unwrap();
        assert_eq!(downstream.len(), 1);
        assert_eq!(downstream[0].recipient_username, "carol");
        // Owned picture (no remote_picture_id) → announce id falls back to the local id text.
        assert_eq!(downstream[0].announce_id, pic.to_string());
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn update_token_changes_resolution(db: PgPool) {
        let owner = seed_user(&db).await;
        let pic = seed_picture(&db, owner).await;
        let share =
            OutgoingShareRepository::create(&db, owner, "Photos", "bob", "other.com", true, true)
                .await
                .unwrap();

        let old = ShareAnnouncementRepository::insert(&db, share.id, pic)
            .await
            .unwrap();
        let new = Uuid::new_v4();
        ShareAnnouncementRepository::update_token(&db, share.id, pic, new)
            .await
            .unwrap();
        assert_eq!(
            ShareAnnouncementRepository::find_picture_by_token(&db, old)
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            ShareAnnouncementRepository::find_picture_by_token(&db, new)
                .await
                .unwrap(),
            Some(pic)
        );
    }
}
