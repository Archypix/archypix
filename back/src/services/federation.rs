use crate::clients::federation::{AnnouncedPicture, FederationClient, PicturesAnnouncement};
use crate::domain::share::ShareStatus;
use crate::domain::tag::TagPath;
use crate::infra::config::Config;
use crate::infra::error::AppError;
use crate::infra::redis::Cache;
use crate::infra::s3::{self, Storage};
use crate::repository::picture::PictureRepository;
use crate::repository::share::{IncomingShareRepository, OutgoingShareRepository};
use crate::repository::user::UserRepository;
use crate::services::pictures::PictureVariant;
use crate::services::shares::{
    ReceivedPictureInfo, cleanup_incoming_share, register_received_pictures,
};
use crate::services::users::find_local_user_id;
use sqlx::PgPool;
use tracing::warn;
use uuid::Uuid;

pub struct PresignItem {
    pub picture_id: String,
    pub variant: Option<String>,
}

/// Validate and record an inbound share announcement from a remote instance.
pub async fn receive_share_announcement(
    db: &PgPool,
    config: &Config,
    authenticated_instance: &str,
    sender_username: &str,
    sender_instance: &str,
    recipient_username: &str,
    recipient_instance: &str,
    outgoing_share_id: Uuid,
    share_token: Uuid,
) -> Result<Uuid, AppError> {
    if recipient_instance != config.global_domain {
        warn!(
            sender_instance,
            recipient_instance, "federation: announce_share rejected — invalid recipient instance"
        );
        return Err(AppError::BadRequest(
            "Invalid recipient instance".to_string(),
        ));
    }
    if sender_instance != authenticated_instance {
        warn!(
            authenticated_instance,
            sender_instance, "federation: announce_share rejected — sender instance mismatch"
        );
        return Err(AppError::Unauthorized(
            "Sender instance does not match authenticated instance".to_string(),
        ));
    }
    let recipient = UserRepository::find_by_username(db, recipient_username)
        .await?
        .ok_or(AppError::NotFound)?;
    let incoming = IncomingShareRepository::create(
        db,
        recipient.id,
        sender_username,
        sender_instance,
        outgoing_share_id,
        Some(share_token),
    )
    .await?;
    Ok(incoming.id)
}

/// Alice received Bob's accept notification; transitions the OutgoingShare to Active and
/// announces all pictures currently under the shared tag to Bob's backend.
///
/// NOTE: the picture announcement is made synchronously here. Under load or when the shared
/// tag contains many pictures this may be slow; a future improvement is to queue the
/// announcement via the in-process TaskQueue and return 202.
pub async fn receive_share_accept(
    db: &PgPool,
    federation: &FederationClient,
    config: &Config,
    authenticated_instance: &str,
    outgoing_share_id: Uuid,
) -> Result<usize, AppError> {
    let share = OutgoingShareRepository::get_by_id(db, outgoing_share_id)
        .await?
        .ok_or(AppError::NotFound)?;

    match share.status {
        ShareStatus::Pending | ShareStatus::Active => {}
        ShareStatus::Revoked | ShareStatus::Tombstoned => return Err(AppError::NotFound),
    }

    if share.recipient_instance != authenticated_instance {
        warn!(
            %outgoing_share_id,
            recipient_instance = %share.recipient_instance,
            authenticated = authenticated_instance,
            "federation: accept_share rejected — instance mismatch"
        );
        return Err(AppError::Unauthorized(
            "Authenticated instance is not the share recipient".to_string(),
        ));
    }

    OutgoingShareRepository::set_status(db, share.id, ShareStatus::Active).await?;

    let owner = UserRepository::find_by_id(db, share.owner_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let pictures = PictureRepository::list_by_tag_and_owner(db, owner.id, &share.tag_path).await?;

    if pictures.is_empty() {
        return Ok(0);
    }

    let count = pictures.len();
    let announced: Vec<AnnouncedPicture> = pictures
        .iter()
        .map(|p| AnnouncedPicture {
            picture_id: p.id.to_string(),
            owner_username: owner.username.clone(),
            owner_instance_domain: config.global_domain.clone(),
            filename: p.filename.clone(),
            mime_type: p.mime_type.clone(),
            file_size: p.file_size,
            width: p.width,
            height: p.height,
            captured_at: p.captured_at,
        })
        .collect();

    federation
        .announce_pictures_to_backend(
            &owner.username,
            &share.recipient_username,
            &share.recipient_instance,
            &PicturesAnnouncement {
                outgoing_share_id: share.id,
                tag_path: share.tag_path.clone(),
                sender_username: owner.username.clone(),
                sender_instance: config.global_domain.clone(),
                pictures: announced,
            },
        )
        .await?;

    Ok(count)
}

/// Received a share revocation from the sender; clean up the matching IncomingShare.
pub async fn receive_share_revoke(
    db: &PgPool,
    cache: &dyn Cache,
    authenticated_instance: &str,
    outgoing_share_id: Uuid,
) -> Result<u64, AppError> {
    let share = IncomingShareRepository::find_by_outgoing_share(
        db,
        outgoing_share_id,
        authenticated_instance,
    )
    .await?
    .ok_or(AppError::NotFound)?;
    cleanup_incoming_share(db, cache, &share, ShareStatus::Revoked).await
}

/// Received a share rejection from the recipient; tombstone the OutgoingShare.
pub async fn receive_share_reject(
    db: &PgPool,
    authenticated_instance: &str,
    outgoing_share_id: Uuid,
) -> Result<(), AppError> {
    let share = OutgoingShareRepository::get_by_id(db, outgoing_share_id)
        .await?
        .ok_or(AppError::NotFound)?;

    if share.recipient_instance != authenticated_instance {
        warn!(
            %outgoing_share_id,
            recipient_instance = %share.recipient_instance,
            authenticated = authenticated_instance,
            "federation: reject_share rejected — instance mismatch"
        );
        return Err(AppError::Unauthorized(
            "Authenticated instance is not the share recipient".to_string(),
        ));
    }

    match share.status {
        ShareStatus::Tombstoned => {}
        ShareStatus::Revoked => return Err(AppError::NotFound),
        ShareStatus::Pending | ShareStatus::Active => {
            OutgoingShareRepository::set_status(db, share.id, ShareStatus::Tombstoned).await?;
        }
    }

    Ok(())
}

/// Received a batch of pictures from a sender; register them under the active IncomingShare.
pub async fn receive_pictures_announcement(
    db: &PgPool,
    authenticated_instance: &str,
    sender_username: &str,
    sender_instance: &str,
    outgoing_share_id: Uuid,
    tag_path: &str,
    pictures: &[ReceivedPictureInfo],
) -> Result<usize, AppError> {
    if sender_instance != authenticated_instance {
        return Err(AppError::Unauthorized(
            "Sender instance does not match authenticated instance".to_string(),
        ));
    }

    let incoming =
        IncomingShareRepository::find_by_outgoing_share(db, outgoing_share_id, sender_instance)
            .await?
            .ok_or(AppError::NotFound)?;

    if incoming.status != ShareStatus::Active {
        return Err(AppError::NotFound);
    }

    let shared_tag = TagPath::shared_to_me(
        sender_username,
        sender_instance,
        &TagPath::from_ltree(tag_path),
    );

    register_received_pictures(
        db,
        incoming.recipient_id,
        incoming.id,
        &shared_tag,
        pictures,
    )
    .await
}

/// Validate a share_token and presign a batch of owned pictures for a remote recipient.
pub async fn presign_batch_for_token(
    db: &PgPool,
    cache: &dyn Cache,
    storage: &dyn Storage,
    config: &Config,
    share_token: Uuid,
    owner_username: &str,
    owner_instance: &str,
    items: &[PresignItem],
) -> Result<Vec<(String, String)>, AppError> {
    let allowed = OutgoingShareRepository::has_active_share_for_token(db, share_token).await?;
    if !allowed {
        return Err(AppError::Unauthorized(
            "share_token does not match any active share".to_string(),
        ));
    }

    let owner_id = find_local_user_id(cache, db, config, owner_username, owner_instance)
        .await?
        .ok_or(AppError::NotFound)?;

    let mut results = Vec::with_capacity(items.len());
    for item in items {
        let picture_id: Uuid = item
            .picture_id
            .parse()
            .map_err(|_| AppError::BadRequest("Invalid picture_id".to_string()))?;
        let picture = PictureRepository::find_by_id(db, picture_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if picture.local_user_id != owner_id || !picture.is_owned() {
            return Err(AppError::NotFound);
        }
        let variant: PictureVariant = item.variant.as_deref().unwrap_or("original").parse()?;
        let key = s3::picture_key(picture.local_user_id, picture.id);
        let url = storage.presign_get(variant.bucket(config), &key).await?;
        results.push((item.picture_id.clone(), url));
    }
    Ok(results)
}
