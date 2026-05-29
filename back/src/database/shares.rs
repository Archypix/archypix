use crate::database::models::{IncomingShare, OutgoingShare, ShareStatus};
use crate::infrastructure::error::{AppError, map_sqlx_error};
use sqlx::{Executor, Postgres};
use uuid::Uuid;

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
        sqlx::query_as!(
            OutgoingShare,
            r#"
            INSERT INTO outgoing_shares (
                owner_id, tag_path, recipient_username, recipient_instance, allow_share_back, future
            )
            VALUES ($1, $2::text::ltree, $3, $4, $5, $6)
            RETURNING id, owner_id, tag_path::text as "tag_path!", recipient_username, recipient_instance,
                      allow_share_back, future, status as "status!: ShareStatus", created_at, revoked_at
            "#,
            owner_id,
            tag_path,
            recipient_username,
            recipient_instance,
            allow_share_back,
            future
        )
            .fetch_one(ex)
            .await
            .map_err(map_sqlx_error)
    }

    pub async fn has_active_share_for_instance<'e, E>(
        ex: E,
        owner_id: Uuid,
        recipient_instance: &str,
    ) -> Result<bool, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let exists = sqlx::query_scalar!(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM outgoing_shares
                WHERE owner_id = $1
                  AND recipient_instance = $2
                  AND status = 'active'::share_status
            ) as "exists!"
            "#,
            owner_id,
            recipient_instance
        )
        .fetch_one(ex)
        .await
        .map_err(map_sqlx_error)?;
        Ok(exists)
    }

    pub async fn list_by_owner<'e, E>(ex: E, owner_id: Uuid) -> Result<Vec<OutgoingShare>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as!(
            OutgoingShare,
            r#"
            SELECT id, owner_id, tag_path::text as "tag_path!", recipient_username, recipient_instance,
                   allow_share_back, future, status as "status!: ShareStatus", created_at, revoked_at
            FROM outgoing_shares
            WHERE owner_id = $1
            ORDER BY created_at DESC
            "#,
            owner_id
        )
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
    ) -> Result<IncomingShare, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as!(
            IncomingShare,
            r#"
            INSERT INTO incoming_shares (
                recipient_id, sender_username, sender_instance, outgoing_share_id
            )
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (recipient_id, sender_username, sender_instance, outgoing_share_id)
            DO UPDATE SET status = incoming_shares.status
            RETURNING id, recipient_id, sender_username, sender_instance, outgoing_share_id,
                      local_mapping_service_id, status as "status!: ShareStatus", created_at, revoked_at
            "#,
            recipient_id,
            sender_username,
            sender_instance,
            outgoing_share_id
        )
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
        sqlx::query_as!(
            IncomingShare,
            r#"
            SELECT id, recipient_id, sender_username, sender_instance, outgoing_share_id,
                   local_mapping_service_id, status as "status!: ShareStatus", created_at, revoked_at
            FROM incoming_shares
            WHERE recipient_id = $1
            ORDER BY created_at DESC
            "#,
            recipient_id
        )
            .fetch_all(ex)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn set_status<'e, E>(
        ex: E,
        incoming_share_id: Uuid,
        status: ShareStatus,
    ) -> Result<(), AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query!(
            r#"
            UPDATE incoming_shares
            SET status = $2
            WHERE id = $1
            "#,
            incoming_share_id,
            status as ShareStatus
        )
        .execute(ex)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }
}
