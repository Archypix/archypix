//! In-process task delivery for share announce / unannounce: same-backend operations run directly
//! against the DB; cross-instance ones post to the recipient's federation endpoints. Called by the
//! task runner (`infra::tasks`) for `AnnounceSharedPictures` / `UnannounceSharedPictures`.

use crate::clients::federation::FederationClient;
use crate::clients::federation::models::{
    PicturesAnnouncementRequest, PicturesUnannouncementRequest,
};
use crate::domain::share::ShareStatus;
use crate::domain::tag::TagPath;
use crate::infra::config::Config;
use crate::infra::error::AppError;
use crate::infra::tasks::InternalTask;
use crate::repository::share::IncomingShareRepository;
use crate::services::shares::registration::{
    register_received_pictures, unregister_announced_pictures,
};
use std::sync::Arc;
use tokio::sync::Notify;

/// Deliver an `AnnounceSharedPictures` task: same-backend registers directly, cross-instance
/// posts to the recipient's `/pictures/announce`.
pub async fn deliver_announce_task(
    db: &sqlx::PgPool,
    federation: &FederationClient,
    config: &Config,
    pipeline_notify: &Arc<Notify>,
    task: InternalTask,
) -> Result<(), AppError> {
    let InternalTask::AnnounceSharedPictures {
        outgoing_share_id,
        sender_username,
        recipient_username,
        recipient_instance,
        tag_path,
        pictures,
        is_same_backend,
    } = task
    else {
        return Ok(());
    };

    if is_same_backend {
        let Some(incoming) = IncomingShareRepository::find_by_outgoing_share(
            db,
            outgoing_share_id,
            &config.global_domain,
        )
        .await?
        else {
            return Ok(());
        };
        if incoming.status != ShareStatus::Active {
            return Ok(());
        }
        let shared_tag = TagPath::shared_to_me(
            &sender_username,
            &config.global_domain,
            &TagPath::from_ltree(&tag_path),
        );
        register_received_pictures(
            db,
            incoming.recipient_id,
            incoming.id,
            &shared_tag,
            &pictures,
        )
        .await?;
        pipeline_notify.notify_one();
    } else {
        federation
            .announce_pictures_to_backend(
                &sender_username,
                &recipient_username,
                &recipient_instance,
                &PicturesAnnouncementRequest {
                    outgoing_share_id,
                    tag_path,
                    sender_username: sender_username.clone(),
                    sender_instance: config.global_domain.clone(),
                    pictures,
                },
            )
            .await?;
    }
    Ok(())
}

/// Deliver an `UnannounceSharedPictures` task: same-backend removes tags directly, cross-instance
/// posts to the recipient's `/pictures/unannounce`.
pub async fn deliver_unannounce_task(
    db: &sqlx::PgPool,
    federation: &FederationClient,
    config: &Config,
    pipeline_notify: &Arc<Notify>,
    task: InternalTask,
) -> Result<(), AppError> {
    let InternalTask::UnannounceSharedPictures {
        outgoing_share_id,
        sender_username,
        recipient_username,
        recipient_instance,
        picture_ids,
        is_same_backend,
    } = task
    else {
        return Ok(());
    };

    if is_same_backend {
        let Some(incoming) = IncomingShareRepository::find_by_outgoing_share(
            db,
            outgoing_share_id,
            &config.global_domain,
        )
        .await?
        else {
            return Ok(());
        };
        unregister_announced_pictures(db, &incoming, &picture_ids).await?;
        pipeline_notify.notify_one();
    } else {
        federation
            .unannounce_pictures_to_backend(
                &sender_username,
                &recipient_username,
                &recipient_instance,
                &PicturesUnannouncementRequest {
                    outgoing_share_id,
                    sender_username: sender_username.clone(),
                    sender_instance: config.global_domain.clone(),
                    picture_ids,
                },
            )
            .await?;
    }
    Ok(())
}
