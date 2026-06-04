use crate::api::federation::models::{
    FederationAuthGrant, FederationAuthRequest, PicturesAnnouncement, PresignRequest,
    PresignResponse, PresignResultItem, ShareAcceptRequest, ShareAnnouncement, ShareRevokeRequest,
};
use crate::api::middleware::auth_federation::AuthFederation;
// Client types used only in accept_share (outbound announcement construction).
use crate::clients::federation::{
    AnnouncedPicture as ClientAnnouncedPicture, PicturesAnnouncement as ClientPicturesAnnouncement,
};
use crate::domain::share::ShareStatus;
use crate::domain::tag::TagPath;
use crate::infra::error::AppError;
use crate::infra::redis::RedisKey;
use crate::infra::s3;
use crate::repository::picture::PictureRepository;
use crate::repository::share::{IncomingShareRepository, OutgoingShareRepository};
use crate::repository::tag::TagRepository;
use crate::repository::user::UserRepository;
use crate::services::pictures::PictureVariant;
use crate::services::shares::{ReceivedPictureInfo, register_received_pictures};
use crate::services::users::find_local_user_id;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use chrono::Utc;
use std::os::macos::raw::stat;
use tracing::{debug, warn};
use uuid::Uuid;

pub async fn auth_request(
    State(state): State<AppState>,
    Json(payload): Json<FederationAuthRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(
        user = %payload.username,
        token_type = "federation",
        requester_instance = %payload.requester_instance,
        "federation: auth_request"
    );
    let token = state
        .federation
        .issue_federation_token(&payload.requester_instance)?;
    let expires_at = Utc::now().timestamp() + state.config.federation_jwt_ttl_secs;

    state
        .federation
        .send_auth_grant(
            &payload.username,
            &payload.requester_instance,
            &FederationAuthGrant {
                issuer_instance: state.config.global_domain.clone(),
                token,
                expires_at,
                scope: payload.scope,
                nonce: payload.nonce,
            },
        )
        .await?;

    Ok(Json(serde_json::json!({ "accepted": true })))
}

pub async fn auth_grant(
    State(state): State<AppState>,
    Json(payload): Json<FederationAuthGrant>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(user = "-", token_type = "federation", issuer_instance = %payload.issuer_instance, "federation: auth_grant");
    let ttl = payload.expires_at - Utc::now().timestamp();
    if ttl <= 0 {
        return Err(AppError::BadRequest("Token already expired".to_string()));
    }
    state
        .federation
        .store_federation_token(&payload.issuer_instance, &payload.token, ttl)
        .await?;
    Ok(Json(serde_json::json!({ "stored": true })))
}

pub async fn announce_share(
    auth: AuthFederation,
    State(state): State<AppState>,
    Json(payload): Json<ShareAnnouncement>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(
        user = %auth.claims.sub,
        token_type = "federation",
        sender = %payload.sender_username,
        sender_instance = %payload.sender_instance,
        recipient = %payload.recipient_username,
        tag_path = %payload.tag_path,
        "federation: announce_share"
    );
    let recipient = UserRepository::find_by_username(&state.db, &payload.recipient_username)
        .await?
        .ok_or(AppError::NotFound)?;

    if payload.recipient_instance != state.config.global_domain {
        warn!(
            user = %auth.claims.sub,
            token_type = "federation",
            recipient_instance = %payload.recipient_instance,
            "federation: announce_share rejected — invalid recipient instance"
        );
        return Err(AppError::BadRequest(
            "Invalid recipient instance".to_string(),
        ));
    }
    if payload.sender_instance != auth.claims.sub {
        warn!(
            user = %auth.claims.sub,
            token_type = "federation",
            sender_instance = %payload.sender_instance,
            "federation: announce_share rejected — sender instance mismatch"
        );
        return Err(AppError::Unauthorized(
            "Sender instance does not match authenticated instance".to_string(),
        ));
    }

    let incoming = IncomingShareRepository::create(
        &state.db,
        recipient.id,
        &payload.sender_username,
        &payload.sender_instance,
        payload.outgoing_share_id,
        Some(payload.share_token),
    )
    .await?;

    debug!(
        user = %auth.claims.sub,
        token_type = "federation",
        share_id = %incoming.id,
        sender = %payload.sender_username,
        sender_instance = %payload.sender_instance,
        "federation: incoming share stored"
    );
    Ok(Json(serde_json::json!({ "accepted": true })))
}

pub async fn revoke_share(
    auth: AuthFederation,
    State(state): State<AppState>,
    Json(payload): Json<ShareRevokeRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(
        user = %auth.claims.sub,
        token_type = "federation",
        outgoing_share_id = %payload.outgoing_share_id,
        "federation: revoke_share"
    );
    // The authenticated instance IS the sender; use it to look up the IncomingShare so
    // Alice does not need to know Bob's internal IncomingShare UUID.
    let share = IncomingShareRepository::find_by_outgoing_share(
        &state.db,
        payload.outgoing_share_id,
        &auth.claims.sub,
    )
    .await?
    .ok_or(AppError::NotFound)?;

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| AppError::InternalServerError(e.to_string()))?;

    // 1. Remove /SharedToMe/… tags assigned by this share.
    TagRepository::remove_incoming_share_tags(&mut *tx, share.id).await?;

    // 2. Delete received-picture rows from this sender that have no remaining incoming_share
    //    tags. Manual tags do not save a picture — it is unreachable once the share is revoked.
    //    Pictures covered by a different still-active share from the same sender are kept.
    let deleted = PictureRepository::delete_received_without_share_tags(
        &mut *tx,
        share.recipient_id,
        &share.sender_username,
        &share.sender_instance,
    )
    .await?;

    // 3. Mark the share as revoked.
    IncomingShareRepository::set_status(&mut *tx, share.id, ShareStatus::Revoked).await?;

    tx.commit()
        .await
        .map_err(|e| AppError::InternalServerError(e.to_string()))?;

    // Fix #1: Invalidate the cached share token so presign requests fail immediately rather
    // than succeeding until the Redis TTL expires.
    let _ = state
        .redis
        .del(RedisKey::IncomingShareToken(
            share.recipient_id,
            &share.sender_username,
            &share.sender_instance,
        ))
        .await;

    debug!(
        user = %auth.claims.sub,
        token_type = "federation",
        outgoing_share_id = %payload.outgoing_share_id,
        deleted_pictures = deleted,
        "federation: share revoked"
    );
    Ok(Json(
        serde_json::json!({ "revoked": true, "pictures_deleted": deleted }),
    ))
}

/// Called by the recipient (Bob) on the sender's (Alice's) backend to accept a share.
///
/// Alice looks up her OutgoingShare, gathers all pictures currently under the shared tag,
/// and announces them to Bob's backend.
///
/// NOTE: the picture announcement to Bob is currently made synchronously in this handler.
/// Under load or when the shared tag contains many pictures this may be slow; a future
/// improvement is to queue the announcement via the in-process TaskQueue and return 202.
pub async fn accept_share(
    auth: AuthFederation,
    State(state): State<AppState>,
    Json(payload): Json<ShareAcceptRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(
        user = %auth.claims.sub,
        token_type = "federation",
        outgoing_share_id = %payload.outgoing_share_id,
        "federation: accept_share"
    );

    let share = OutgoingShareRepository::get_by_id(&state.db, payload.outgoing_share_id)
        .await?
        .ok_or(AppError::NotFound)?;

    // Reject if the share was revoked or tombstoned before the accept arrived.
    // Allow both Pending (normal path) and Active (idempotent retry — re-announce pictures).
    match share.status {
        ShareStatus::Pending | ShareStatus::Active => {}
        ShareStatus::Revoked | ShareStatus::Tombstoned => return Err(AppError::NotFound),
    }

    // Verify the authenticated instance is the intended recipient.
    if share.recipient_instance != auth.claims.sub {
        warn!(
            outgoing_share_id = %payload.outgoing_share_id,
            recipient_instance = %share.recipient_instance,
            authenticated = %auth.claims.sub,
            "federation: accept_share rejected — instance mismatch"
        );
        return Err(AppError::Unauthorized(
            "Authenticated instance is not the share recipient".to_string(),
        ));
    }

    OutgoingShareRepository::set_status(&state.db, share.id, ShareStatus::Active).await?;

    // Look up Alice (the owner) by owner_id to get her username for the announcement.
    let owner = UserRepository::find_by_id(&state.db, share.owner_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let pictures =
        PictureRepository::list_by_tag_and_owner(&state.db, owner.id, &share.tag_path).await?;

    if pictures.is_empty() {
        return Ok(Json(serde_json::json!({ "announced": 0 })));
    }

    let announced: Vec<ClientAnnouncedPicture> = pictures
        .iter()
        .map(|p| ClientAnnouncedPicture {
            picture_id: p.id.to_string(),
            owner_username: owner.username.clone(),
            owner_instance_domain: state.config.global_domain.clone(),
            filename: p.filename.clone(),
            mime_type: p.mime_type.clone(),
            file_size: p.file_size,
            width: p.width,
            height: p.height,
            captured_at: p.captured_at,
        })
        .collect();

    let count = announced.len();
    let announcement = ClientPicturesAnnouncement {
        outgoing_share_id: share.id,
        tag_path: share.tag_path.clone(),
        sender_username: owner.username.clone(),
        sender_instance: state.config.global_domain.clone(),
        pictures: announced,
    };

    state
        .federation
        .announce_pictures_to_backend(
            &owner.username,
            &share.recipient_username,
            &share.recipient_instance,
            &announcement,
        )
        .await?;

    debug!(
        outgoing_share_id = %share.id,
        count,
        "federation: pictures announced after share accept"
    );
    Ok(Json(serde_json::json!({ "announced": count })))
}

/// Called by the sender (Alice) on the recipient's (Bob's) backend to deliver shared pictures.
///
/// For each picture: upserts a received-picture row and assigns the `/SharedToMe/…` tag.
/// All pictures are processed in a single transaction.
pub async fn announce_pictures(
    auth: AuthFederation,
    State(state): State<AppState>,
    Json(payload): Json<PicturesAnnouncement>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(
        user = %auth.claims.sub,
        token_type = "federation",
        outgoing_share_id = %payload.outgoing_share_id,
        picture_count = payload.pictures.len(),
        "federation: announce_pictures"
    );

    if payload.sender_instance != auth.claims.sub {
        return Err(AppError::Unauthorized(
            "Sender instance does not match authenticated instance".to_string(),
        ));
    }

    // Find Bob's IncomingShare so we know the recipient and can use the share ID as source_id.
    let incoming_share = IncomingShareRepository::find_by_outgoing_share(
        &state.db,
        payload.outgoing_share_id,
        &payload.sender_instance,
    )
    .await?
    .ok_or(AppError::NotFound)?;

    if incoming_share.status != ShareStatus::Active {
        return Err(AppError::NotFound);
    }

    // Build the /SharedToMe/… tag path once for the whole batch.
    let shared_tag = TagPath::shared_to_me(
        &payload.sender_username,
        &payload.sender_instance,
        &TagPath::from_ltree(&payload.tag_path),
    );

    let pics: Vec<ReceivedPictureInfo> = payload
        .pictures
        .iter()
        .map(|p| ReceivedPictureInfo {
            remote_picture_id: p.picture_id.clone(),
            owner_username: p.owner_username.clone(),
            owner_instance_domain: p.owner_instance_domain.clone(),
            filename: p.filename.clone(),
            mime_type: p.mime_type.clone(),
            file_size: p.file_size,
            width: p.width,
            height: p.height,
            captured_at: p.captured_at,
        })
        .collect();

    let registered = register_received_pictures(
        &state.db,
        incoming_share.recipient_id,
        incoming_share.id,
        &shared_tag,
        &pics,
    )
    .await?;

    debug!(
        outgoing_share_id = %payload.outgoing_share_id,
        registered,
        "federation: pictures registered"
    );
    Ok(Json(serde_json::json!({ "registered": registered })))
}

/// Batch presign endpoint — no federation JWT required; `share_token` is the sole proof of
/// authorization. Processes all requested pictures in a single response.
pub async fn presign_picture(
    State(state): State<AppState>,
    Json(payload): Json<PresignRequest>,
) -> Result<Json<PresignResponse>, AppError> {
    debug!(
        owner = %payload.owner_username,
        owner_instance = %payload.owner_instance,
        picture_count = payload.pictures.len(),
        "federation: presign_picture"
    );

    let allowed =
        OutgoingShareRepository::has_active_share_for_token(&state.db, payload.share_token).await?;
    if !allowed {
        return Err(AppError::Unauthorized(
            "share_token does not match any active share".to_string(),
        ));
    }

    let owner_id = find_local_user_id(
        &state.redis,
        &state.db,
        &state.config,
        &payload.owner_username,
        &payload.owner_instance,
    )
    .await?
    .ok_or(AppError::NotFound)?;

    let mut urls = Vec::with_capacity(payload.pictures.len());
    for item in &payload.pictures {
        let picture_id: Uuid = item
            .picture_id
            .parse()
            .map_err(|_| AppError::BadRequest("Invalid picture_id".to_string()))?;

        let picture = PictureRepository::find_by_id(&state.db, picture_id)
            .await?
            .ok_or(AppError::NotFound)?;

        if picture.local_user_id != owner_id || !picture.is_owned() {
            return Err(AppError::NotFound);
        }

        let variant: PictureVariant = item.variant.as_deref().unwrap_or("original").parse()?;
        let bucket = variant.bucket(&state.config);
        let key = s3::picture_key(picture.local_user_id, picture.id);
        let url = state.storage.presign_get(bucket, &key).await?;

        urls.push(PresignResultItem {
            picture_id: item.picture_id.clone(),
            url,
        });
    }

    Ok(Json(PresignResponse { urls }))
}
