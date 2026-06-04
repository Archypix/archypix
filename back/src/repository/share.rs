use crate::domain::share::{IncomingShare, OutgoingShare, ShareStatus};
use crate::infra::error::{AppError, map_sqlx_error};
use sqlx::{Executor, Postgres};
use uuid::Uuid;

// Non-macro query_as is used for queries that reference share_token / origin_share_token so that
// the build doesn't require those columns to exist in the compile-time database. Once the schema
// is recreated (docker compose down -v && up) these can be switched back to query_as! macros.

pub struct OutgoingShareRepository;

impl OutgoingShareRepository {
    pub async fn create<'e, E>(
        ex: E,
        owner_id: Uuid,
        tag_path: &str,
        recipient_username: &str,
        recipient_instance: &str,
        allow_share_back: bool,
        future: bool,
    ) -> Result<OutgoingShare, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as(
            r#"INSERT INTO outgoing_shares
                   (owner_id, tag_path, recipient_username, recipient_instance, allow_share_back, future)
               VALUES ($1, $2::text::ltree, $3, $4, $5, $6)
               RETURNING id, owner_id, tag_path::text as tag_path, recipient_username, recipient_instance,
                         allow_share_back, future, status, share_token, created_at, revoked_at"#,
        )
            .bind(owner_id)
            .bind(tag_path)
            .bind(recipient_username)
            .bind(recipient_instance)
            .bind(allow_share_back)
            .bind(future)
            .fetch_one(ex)
            .await
            .map_err(map_sqlx_error)
    }

    pub async fn get_by_id<'e, E>(ex: E, share_id: Uuid) -> Result<Option<OutgoingShare>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as(
            r#"SELECT id, owner_id, tag_path::text as tag_path, recipient_username, recipient_instance,
                      allow_share_back, future, status, share_token, created_at, revoked_at
               FROM outgoing_shares WHERE id = $1"#,
        )
            .bind(share_id)
            .fetch_optional(ex)
            .await
            .map_err(map_sqlx_error)
    }

    /// Check if a share token belongs to an active outgoing share. Used for transitive presign
    /// authorization: a recipient holds the token from the original sender's OutgoingShare.
    pub async fn has_active_share_for_token<'e, E>(
        ex: E,
        share_token: Uuid,
    ) -> Result<bool, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let exists: Option<bool> = sqlx::query_scalar(
            r#"SELECT EXISTS(
                   SELECT 1 FROM outgoing_shares
                   WHERE share_token = $1 AND status = 'active'::share_status
               )"#,
        )
        .bind(share_token)
        .fetch_one(ex)
        .await
        .map_err(map_sqlx_error)?;
        Ok(exists.unwrap_or(false))
    }

    pub async fn set_status<'e, E>(
        ex: E,
        share_id: Uuid,
        status: ShareStatus,
    ) -> Result<(), AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query(
            r#"UPDATE outgoing_shares
               SET status = $2,
                   revoked_at = CASE WHEN $2 = 'revoked'::share_status
                                     THEN now() AT TIME ZONE 'utc'
                                     ELSE revoked_at
                                END
               WHERE id = $1"#,
        )
        .bind(share_id)
        .bind(status)
        .execute(ex)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    pub async fn list_by_owner<'e, E>(ex: E, owner_id: Uuid) -> Result<Vec<OutgoingShare>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as(
            r#"SELECT id, owner_id, tag_path::text as tag_path, recipient_username, recipient_instance,
                      allow_share_back, future, status, share_token, created_at, revoked_at
               FROM outgoing_shares WHERE owner_id = $1 ORDER BY created_at DESC"#,
        )
            .bind(owner_id)
            .fetch_all(ex)
            .await
            .map_err(map_sqlx_error)
    }
}

pub struct IncomingShareRepository;

impl IncomingShareRepository {
    pub async fn create<'e, E>(
        ex: E,
        recipient_id: Uuid,
        sender_username: &str,
        sender_instance: &str,
        outgoing_share_id: Uuid,
        origin_share_token: Option<Uuid>,
    ) -> Result<IncomingShare, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as(
            r#"INSERT INTO incoming_shares
                   (recipient_id, sender_username, sender_instance, outgoing_share_id, origin_share_token)
               VALUES ($1, $2, $3, $4, $5)
               ON CONFLICT (recipient_id, sender_username, sender_instance, outgoing_share_id)
               DO UPDATE SET status = incoming_shares.status,
                             origin_share_token = COALESCE($5, incoming_shares.origin_share_token)
               RETURNING id, recipient_id, sender_username, sender_instance, outgoing_share_id,
                         local_mapping_service_id, status, origin_share_token, created_at, revoked_at"#,
        )
            .bind(recipient_id)
            .bind(sender_username)
            .bind(sender_instance)
            .bind(outgoing_share_id)
            .bind(origin_share_token)
            .fetch_one(ex)
            .await
            .map_err(map_sqlx_error)
    }

    pub async fn list_by_recipient<'e, E>(
        ex: E,
        recipient_id: Uuid,
    ) -> Result<Vec<IncomingShare>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as(
            r#"SELECT id, recipient_id, sender_username, sender_instance, outgoing_share_id,
                      local_mapping_service_id, status, origin_share_token, created_at, revoked_at
               FROM incoming_shares WHERE recipient_id = $1 ORDER BY created_at DESC"#,
        )
        .bind(recipient_id)
        .fetch_all(ex)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn set_status<'e, E>(
        ex: E,
        share_id: Uuid,
        status: ShareStatus,
    ) -> Result<(), AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query(
            r#"UPDATE incoming_shares
               SET status = $2,
                   revoked_at = CASE WHEN $2 = 'revoked'::share_status
                                     THEN now() AT TIME ZONE 'utc'
                                     ELSE revoked_at
                                END
               WHERE id = $1"#,
        )
        .bind(share_id)
        .bind(status)
        .execute(ex)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    pub async fn get_by_id<'e, E>(ex: E, share_id: Uuid) -> Result<Option<IncomingShare>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as(
            r#"SELECT id, recipient_id, sender_username, sender_instance, outgoing_share_id,
                      local_mapping_service_id, status, origin_share_token, created_at, revoked_at
               FROM incoming_shares WHERE id = $1"#,
        )
        .bind(share_id)
        .fetch_optional(ex)
        .await
        .map_err(map_sqlx_error)
    }

    /// Find the incoming share for a given outgoing_share_id from a specific sender instance.
    /// Used by the recipient backend when the sender announces pictures after share acceptance.
    pub async fn find_by_outgoing_share<'e, E>(
        ex: E,
        outgoing_share_id: Uuid,
        sender_instance: &str,
    ) -> Result<Option<IncomingShare>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as(
            r#"SELECT id, recipient_id, sender_username, sender_instance, outgoing_share_id,
                      local_mapping_service_id, status, origin_share_token, created_at, revoked_at
               FROM incoming_shares
               WHERE outgoing_share_id = $1 AND sender_instance = $2"#,
        )
        .bind(outgoing_share_id)
        .bind(sender_instance)
        .fetch_optional(ex)
        .await
        .map_err(map_sqlx_error)
    }

    /// Return the `origin_share_token` of an active incoming share from the given sender.
    /// Used to authorize federation presign requests for received cross-instance pictures.
    pub async fn find_token_by_sender<'e, E>(
        ex: E,
        local_user_id: Uuid,
        sender_username: &str,
        sender_instance: &str,
    ) -> Result<Option<Uuid>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_scalar(
            r#"SELECT origin_share_token
               FROM incoming_shares
               WHERE recipient_id = $1
                 AND sender_username = $2
                 AND sender_instance = $3
                 AND status = 'active'::share_status
                 AND origin_share_token IS NOT NULL
               ORDER BY created_at DESC
               LIMIT 1"#,
        )
        .bind(local_user_id)
        .bind(sender_username)
        .bind(sender_instance)
        .fetch_optional(ex)
        .await
        .map_err(map_sqlx_error)
        .map(|opt| opt.flatten())
    }
}
