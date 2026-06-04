use crate::clients::federation::{FederationClient, ShareAnnouncement};
use crate::domain::share::{OutgoingShare, ShareStatus};
use crate::domain::tag::TagPath;
use crate::infra::config::Config;
use crate::infra::error::AppError;
use crate::infra::redis::{RedisClient, RedisKey};
use crate::repository::picture::PictureRepository;
use crate::repository::share::{IncomingShareRepository, OutgoingShareRepository};
use crate::repository::tag::TagRepository;
use crate::services::users::find_local_user_id;
use chrono::NaiveDateTime;
use sqlx::PgPool;
use tracing::trace;
use uuid::Uuid;

/// Minimal picture descriptor used to register a batch of received pictures.
/// Accepted by both the same-backend and cross-instance code paths so the
/// registration logic is not duplicated.
pub struct ReceivedPictureInfo {
    pub remote_picture_id: String,
    pub owner_username: String,
    pub owner_instance_domain: String,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub file_size: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub captured_at: Option<NaiveDateTime>,
}

/// Upsert received-picture rows and assign `/SharedToMe/…` tags for every picture in
/// `pictures`, all inside a single DB transaction.
///
/// Both `create_received` (ON CONFLICT DO UPDATE) and `assign_incoming_share_tag`
/// (ON CONFLICT DO NOTHING) are idempotent, so replaying the same announcement is safe.
pub async fn register_received_pictures(
    db: &PgPool,
    recipient_id: Uuid,
    incoming_share_id: Uuid,
    shared_tag: &TagPath,
    pictures: &[ReceivedPictureInfo],
) -> Result<usize, AppError> {
    if pictures.is_empty() {
        return Ok(0);
    }

    let mut tx = db
        .begin()
        .await
        .map_err(|e| AppError::InternalServerError(e.to_string()))?;

    for pic in pictures {
        let received = PictureRepository::create_received(
            &mut *tx,
            recipient_id,
            &pic.remote_picture_id,
            &pic.owner_username,
            &pic.owner_instance_domain,
            pic.filename.as_deref(),
            pic.mime_type.as_deref(),
            pic.file_size,
            pic.width,
            pic.height,
            pic.captured_at,
        )
        .await?;

        TagRepository::assign_incoming_share_tag(
            &mut *tx,
            received.id,
            shared_tag.as_ltree(),
            incoming_share_id,
        )
        .await?;
    }

    tx.commit()
        .await
        .map_err(|e| AppError::InternalServerError(e.to_string()))?;

    Ok(pictures.len())
}

pub async fn create_outgoing_share(
    db: &PgPool,
    redis: &RedisClient,
    federation: &FederationClient,
    config: &Config,
    owner_id: Uuid,
    sender_username: &str,
    tag_path: &str,
    recipient_username: &str,
    recipient_instance: &str,
    allow_share_back: bool,
    future: bool,
    shareback_of: Option<Uuid>,
) -> Result<OutgoingShare, AppError> {
    trace!(
        owner_id = %owner_id,
        sender = sender_username,
        tag_path,
        recipient = recipient_username,
        recipient_instance,
        "shares: create_outgoing_share"
    );

    let recipient_local_id =
        find_local_user_id(redis, db, config, recipient_username, recipient_instance).await?;

    let mut tx = db
        .begin()
        .await
        .map_err(|e| AppError::InternalServerError(e.to_string()))?;

    let share = OutgoingShareRepository::create(
        &mut *tx,
        owner_id,
        tag_path,
        recipient_username,
        recipient_instance,
        allow_share_back,
        future,
    )
    .await?;

    if let Some(recipient_id) = recipient_local_id {
        // Same-backend: create IncomingShare in the same transaction.
        IncomingShareRepository::create(
            &mut *tx,
            recipient_id,
            sender_username,
            &config.global_domain,
            share.id,
            Some(share.share_token),
        )
        .await?;
    } else {
        // Cross-instance share: announce via federation protocol.
        let token = federation
            .get_or_wait_federation_token(sender_username, recipient_username, recipient_instance)
            .await?;
        federation
            .announce_share(
                recipient_username,
                recipient_instance,
                &token,
                &ShareAnnouncement {
                    sender_username: sender_username.to_string(),
                    sender_instance: config.global_domain.clone(),
                    recipient_username: recipient_username.to_string(),
                    recipient_instance: recipient_instance.to_string(),
                    outgoing_share_id: share.id,
                    tag_path: tag_path.to_string(),
                    allow_share_back,
                    future,
                    shareback_of,
                    share_token: share.share_token,
                },
            )
            .await?;
    }

    tx.commit()
        .await
        .map_err(|e| AppError::InternalServerError(e.to_string()))?;

    Ok(share)
}

/// Accept an incoming share on behalf of `acceptor_username`.
///
/// - Same-backend: queries the sender's pictures under the shared tag and directly creates
///   received-picture rows + SharedToMe tags for the acceptor in a single transaction.
/// - Cross-instance: sends a federation accept message to the sender's backend; the sender
///   will asynchronously call `/api/federation/pictures/announce` with the current pictures.
pub async fn accept_incoming_share(
    db: &PgPool,
    redis: &RedisClient,
    federation: &FederationClient,
    config: &Config,
    acceptor_id: Uuid,
    acceptor_username: &str,
    share_id: Uuid,
) -> Result<usize, AppError> {
    trace!(share_id = %share_id, acceptor = acceptor_username, "shares: accept_incoming_share");

    let incoming = IncomingShareRepository::get_by_id(db, share_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if incoming.recipient_id != acceptor_id {
        return Err(AppError::NotFound);
    }

    match incoming.status {
        ShareStatus::Pending => {}           // normal path
        ShareStatus::Active => return Ok(0), // already accepted — idempotent
        ShareStatus::Revoked | ShareStatus::Tombstoned => return Err(AppError::NotFound),
    }

    // Transition to Active immediately — this is Bob's consent. For the cross-instance
    // path, pictures arrive asynchronously via `announce_pictures`; the share must already
    // be Active by then.
    IncomingShareRepository::set_status(db, incoming.id, ShareStatus::Active).await?;

    if let Some(sender_id) = find_local_user_id(
        redis,
        db,
        config,
        &incoming.sender_username,
        &incoming.sender_instance,
    )
    .await?
    {
        // ── Same-backend path ─────────────────────────────────────────────────
        let outgoing = OutgoingShareRepository::get_by_id(db, incoming.outgoing_share_id)
            .await?
            .ok_or(AppError::NotFound)?;

        let pictures =
            PictureRepository::list_by_tag_and_owner(db, sender_id, &outgoing.tag_path).await?;

        let shared_tag = TagPath::shared_to_me(
            &incoming.sender_username,
            &incoming.sender_instance,
            &TagPath::from_ltree(&outgoing.tag_path),
        );

        let pics: Vec<ReceivedPictureInfo> = pictures
            .iter()
            .map(|p| ReceivedPictureInfo {
                remote_picture_id: p.id.to_string(),
                owner_username: incoming.sender_username.clone(),
                owner_instance_domain: incoming.sender_instance.clone(),
                filename: p.filename.clone(),
                mime_type: p.mime_type.clone(),
                file_size: p.file_size,
                width: p.width,
                height: p.height,
                captured_at: p.captured_at,
            })
            .collect();

        let count =
            register_received_pictures(db, acceptor_id, incoming.id, &shared_tag, &pics).await?;
        OutgoingShareRepository::set_status(db, incoming.outgoing_share_id, ShareStatus::Active)
            .await?;
        Ok(count)
    } else {
        // ── Cross-instance path ───────────────────────────────────────────────
        federation
            .send_share_accept(
                acceptor_username,
                &incoming.sender_username,
                &incoming.sender_instance,
                incoming.outgoing_share_id,
            )
            .await?;
        Ok(0)
    }
}

/// Revoke an outgoing share owned by `owner_id`.
///
/// - Marks the `OutgoingShare` as `revoked`.
/// - Same-backend: directly revokes the matching `IncomingShare` (removes tags, deletes
///   unreachable received pictures, invalidates the share-token Redis cache).
/// - Cross-instance: sends a federation revocation message to the recipient's backend.
pub async fn revoke_outgoing_share(
    db: &PgPool,
    redis: &RedisClient,
    federation: &FederationClient,
    config: &Config,
    owner_id: Uuid,
    owner_username: &str,
    share_id: Uuid,
) -> Result<(), AppError> {
    trace!(share_id = %share_id, owner_id = %owner_id, "shares: revoke_outgoing_share");

    let share = OutgoingShareRepository::get_by_id(db, share_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if share.owner_id != owner_id {
        return Err(AppError::NotFound);
    }
    if share.status == ShareStatus::Revoked {
        return Ok(()); // idempotent
    }

    // Mark the outgoing share as revoked first so no new picture announcements go out.
    OutgoingShareRepository::set_status(db, share_id, ShareStatus::Revoked).await?;

    if let Some(_recipient_id) = find_local_user_id(
        redis,
        db,
        config,
        &share.recipient_username,
        &share.recipient_instance,
    )
    .await?
    {
        // ── Same-backend path ─────────────────────────────────────────────────
        // The IncomingShare may not exist yet (e.g. share created and immediately revoked).
        if let Some(incoming) =
            IncomingShareRepository::find_by_outgoing_share(db, share_id, &config.global_domain)
                .await?
        {
            if incoming.status != ShareStatus::Revoked && incoming.status != ShareStatus::Tombstoned
            {
                let mut tx = db
                    .begin()
                    .await
                    .map_err(|e| AppError::InternalServerError(e.to_string()))?;

                TagRepository::remove_incoming_share_tags(&mut *tx, incoming.id).await?;
                PictureRepository::delete_received_without_share_tags(
                    &mut *tx,
                    incoming.recipient_id,
                    owner_username,
                    &config.global_domain,
                )
                .await?;
                IncomingShareRepository::set_status(&mut *tx, incoming.id, ShareStatus::Revoked)
                    .await?;

                tx.commit()
                    .await
                    .map_err(|e| AppError::InternalServerError(e.to_string()))?;

                let _ = redis
                    .del(RedisKey::IncomingShareToken(
                        incoming.recipient_id,
                        &incoming.sender_username,
                        &incoming.sender_instance,
                    ))
                    .await;
            }
        }
    } else {
        // ── Cross-instance path ───────────────────────────────────────────────
        federation
            .send_revocation(
                owner_username,
                &share.recipient_username,
                &share.recipient_instance,
                share.id,
            )
            .await?;
    }

    Ok(())
}
