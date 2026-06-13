//! Pipeline announcement step — the single picture-announcement path.
//!
//! Two entry points, both diffing share coverage against the `share_announcements` tracking table
//! and enqueuing `AnnounceSharedPictures` / `UnannounceSharedPictures` tasks:
//!
//! - [`process_first_announcements`] — for each `pending_first_announcement` share, announce its
//!   current coverage (ignoring `future`) and flip it to `active`, in one transaction. This is the
//!   *initial* announce, triggered by share acceptance.
//! - [`process_batch`] — the *ongoing* diff for `active` + `future = true` shares over a batch of
//!   reconciled (dirty) pictures: announce new coverage, unannounce lost coverage, and refresh
//!   tokens after a partial upstream revocation.
//!
//! Loop prevention (owner == recipient) is applied by the coverage query / inline check.

use crate::clients::federation::models::AnnouncedPicture;
use crate::domain::picture::Picture;
use crate::domain::share::ShareStatus;
use crate::infra::config::Config;
use crate::infra::error::AppError;
use crate::infra::error::map_sqlx_error;
use crate::infra::tasks::{InternalTask, TaskQueue};
use crate::repository::picture::PictureRepository;
use crate::repository::share::OutgoingShareRepository;
use crate::repository::share_announcement::ShareAnnouncementRepository;
use crate::repository::tag::TagRepository;
use crate::repository::user::UserRepository;
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// The id the recipient stores as `remote_picture_id` — the original owner's picture id.
fn announce_id(p: &Picture) -> String {
    p.remote_picture_id
        .clone()
        .unwrap_or_else(|| p.id.to_string())
}

/// Resolve the token for a freshly-covered picture and record it in `share_announcements` (inside
/// `tx`): an owned picture gets a newly generated token; a received picture forwards its active
/// upstream token (returns `None` — skip — when no active upstream token exists). The single
/// place where new tracking rows are created, shared by both announcement entry points.
async fn record_new_token(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    share_id: Uuid,
    picture: &Picture,
) -> Result<Option<Uuid>, AppError> {
    if picture.is_owned() {
        Ok(Some(
            ShareAnnouncementRepository::insert(&mut **tx, share_id, picture.id).await?,
        ))
    } else {
        let Some(upstream) =
            TagRepository::find_active_picture_token(&mut **tx, picture.id).await?
        else {
            return Ok(None);
        };
        ShareAnnouncementRepository::insert_with_token(&mut **tx, share_id, picture.id, upstream)
            .await?;
        Ok(Some(upstream))
    }
}

/// Initial announcement: for every `pending_first_announcement` share owned by `user_id`, announce
/// its current coverage (ignoring `future`) and transition it to `active`. Tracking rows and the
/// status flip are written in a single transaction per share; the delivery task is enqueued after
/// commit.
pub async fn process_first_announcements(
    db: &PgPool,
    task_queue: &TaskQueue,
    config: &Config,
    user_id: Uuid,
) -> Result<(), AppError> {
    let shares =
        OutgoingShareRepository::list_pending_first_announcement_by_owner(db, user_id).await?;
    if shares.is_empty() {
        return Ok(());
    }
    let sender_username = UserRepository::find_by_id(db, user_id)
        .await?
        .map(|u| u.username)
        .unwrap_or_default();

    for share in shares {
        let pictures =
            PictureRepository::list_by_tag_for_user(db, share.owner_id, &share.tag_path).await?;

        let mut items: Vec<AnnouncedPicture> = Vec::new();
        let mut tx = db.begin().await.map_err(map_sqlx_error)?;
        for p in &pictures {
            // Loop prevention: never announce a picture back to its own owner.
            if p.owner_username.as_deref() == Some(&share.recipient_username)
                && p.owner_instance_domain.as_deref() == Some(&share.recipient_instance)
            {
                continue;
            }
            if let Some(token) = record_new_token(&mut tx, share.id, p).await? {
                items.push(AnnouncedPicture::from_picture(
                    p,
                    token,
                    &sender_username,
                    &config.global_domain,
                ));
            }
        }
        // Flip to Active in the same transaction as the tracking rows it just wrote.
        OutgoingShareRepository::set_status(&mut *tx, share.id, ShareStatus::Active).await?;
        tx.commit().await.map_err(map_sqlx_error)?;

        if !items.is_empty() {
            let is_same_backend = share.recipient_instance == config.global_domain;
            task_queue.enqueue(InternalTask::AnnounceSharedPictures {
                outgoing_share_id: share.id,
                sender_username: sender_username.clone(),
                recipient_username: share.recipient_username.clone(),
                recipient_instance: share.recipient_instance.clone(),
                tag_path: share.tag_path.clone(),
                pictures: items,
                is_same_backend,
            });
        }
    }
    Ok(())
}

/// Ongoing announce/unannounce diff for a batch of reconciled (dirty) picture ids, over the user's
/// `active` + `future = true` shares.
pub async fn process_batch(
    db: &PgPool,
    task_queue: &TaskQueue,
    config: &Config,
    user_id: Uuid,
    dirty_ids: &[Uuid],
) -> Result<(), AppError> {
    if dirty_ids.is_empty() {
        return Ok(());
    }

    // Active future shares.
    let shares = OutgoingShareRepository::list_active_future_by_owner(db, user_id).await?;
    if shares.is_empty() {
        return Ok(());
    }
    let share_ids: Vec<Uuid> = shares.iter().map(|s| s.id).collect();
    let share_by_id: HashMap<Uuid, _> = shares.iter().map(|s| (s.id, s)).collect();

    // Coverage, tracking, upstream tokens, and picture metadata (all read before the write tx).
    let coverage = ShareAnnouncementRepository::current_coverage(db, user_id, dirty_ids).await?;
    let tracking =
        ShareAnnouncementRepository::find_tracking_for_pictures(db, &share_ids, dirty_ids).await?;
    let upstream = TagRepository::active_picture_tokens_for(db, dirty_ids).await?;

    let mut tracking_map: HashMap<(Uuid, Uuid), Uuid> = HashMap::new();
    for (share_id, pic, token) in &tracking {
        tracking_map.insert((*share_id, *pic), *token);
    }
    let coverage_set: HashSet<(Uuid, Uuid)> =
        coverage.iter().map(|(pic, share)| (*share, *pic)).collect();

    let mut involved: HashSet<Uuid> = HashSet::new();
    for (pic, _) in &coverage {
        involved.insert(*pic);
    }
    for (_, pic, _) in &tracking {
        involved.insert(*pic);
    }
    let involved_ids: Vec<Uuid> = involved.into_iter().collect();
    let metas = PictureRepository::list_by_ids(db, &involved_ids).await?;
    let meta_by_id: HashMap<Uuid, Picture> = metas.into_iter().map(|p| (p.id, p)).collect();

    let sender_username = UserRepository::find_by_id(db, user_id)
        .await?
        .map(|u| u.username)
        .unwrap_or_default();

    // Accumulate announce / unannounce work per share, applying all tracking mutations in one tx.
    let mut announce_by_share: HashMap<Uuid, Vec<AnnouncedPicture>> = HashMap::new();
    let mut unannounce_by_share: HashMap<Uuid, Vec<String>> = HashMap::new();
    let mut tx = db.begin().await.map_err(map_sqlx_error)?;

    // ── New coverage / token refresh ──────────────────────────────────────────
    for (pic, share) in &coverage {
        let Some(meta) = meta_by_id.get(pic) else {
            continue;
        };
        let desired = upstream.get(pic).copied(); // Some => received picture
        let token = match tracking_map.get(&(*share, *pic)) {
            None => match record_new_token(&mut tx, *share, meta).await? {
                Some(t) => t,
                None => continue, // received picture without an active upstream token
            },
            Some(existing) => match desired {
                // Already announced — re-announce only on a token change (received picture whose
                // upstream token moved). Owned pictures have no upstream token → stable.
                Some(up) if up != *existing => {
                    ShareAnnouncementRepository::update_token(&mut *tx, *share, *pic, up).await?;
                    up
                }
                _ => continue,
            },
        };
        announce_by_share
            .entry(*share)
            .or_default()
            .push(AnnouncedPicture::from_picture(
                meta,
                token,
                &sender_username,
                &config.global_domain,
            ));
    }

    // ── Lost coverage → unannounce ────────────────────────────────────────────
    for (share, pic, _token) in &tracking {
        if coverage_set.contains(&(*share, *pic)) {
            continue;
        }
        // No longer covered (tag removed or picture deleted) — delete tracking, unannounce.
        ShareAnnouncementRepository::delete(&mut *tx, *share, *pic).await?;
        let announce = meta_by_id
            .get(pic)
            .map(announce_id)
            .unwrap_or_else(|| pic.to_string());
        unannounce_by_share
            .entry(*share)
            .or_default()
            .push(announce);
    }

    tx.commit().await.map_err(map_sqlx_error)?;

    // ── Enqueue tasks (after commit) ──────────────────────────────────────────
    for (share_id, pictures) in announce_by_share {
        let Some(share) = share_by_id.get(&share_id) else {
            continue;
        };
        let is_same_backend = share.recipient_instance == config.global_domain;
        task_queue.enqueue(InternalTask::AnnounceSharedPictures {
            outgoing_share_id: share_id,
            sender_username: sender_username.clone(),
            recipient_username: share.recipient_username.clone(),
            recipient_instance: share.recipient_instance.clone(),
            tag_path: share.tag_path.clone(),
            pictures,
            is_same_backend,
        });
    }
    for (share_id, picture_ids) in unannounce_by_share {
        let Some(share) = share_by_id.get(&share_id) else {
            continue;
        };
        let is_same_backend = share.recipient_instance == config.global_domain;
        task_queue.enqueue(InternalTask::UnannounceSharedPictures {
            outgoing_share_id: share_id,
            sender_username: sender_username.clone(),
            recipient_username: share.recipient_username.clone(),
            recipient_instance: share.recipient_instance.clone(),
            picture_ids,
            is_same_backend,
        });
    }

    Ok(())
}
