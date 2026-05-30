use crate::clients::federation::{FederationClient, ShareAnnouncement};
use crate::domain::share::OutgoingShare;
use crate::infra::config::Config;
use crate::infra::error::AppError;
use crate::repository::share::OutgoingShareRepository;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn create_outgoing_share(
    db: &PgPool,
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
    let share = OutgoingShareRepository::create(
        db,
        owner_id,
        tag_path,
        recipient_username,
        recipient_instance,
        allow_share_back,
        future,
    )
    .await?;

    // `recipient_instance` is the global (WebFinger) domain; backend resolution happens inside.
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
                sender_instance: config.webfinger_host.clone(),
                recipient_username: recipient_username.to_string(),
                recipient_instance: recipient_instance.to_string(),
                outgoing_share_id: share.id,
                tag_path: tag_path.to_string(),
                allow_share_back,
                future,
                shareback_of,
            },
        )
        .await?;

    Ok(share)
}
