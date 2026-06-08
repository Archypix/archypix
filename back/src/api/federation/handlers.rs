use crate::api::federation::models::{
    FederationAuthGrant, FederationAuthRequest, PicturesAnnouncement, PresignRequest,
    PresignResponse, PresignResultItem, ShareAcceptRequest, ShareAnnouncement, ShareRejectRequest,
    ShareRevokeRequest,
};
use crate::api::middleware::auth_federation::AuthFederation;
use crate::infra::error::AppError;
use crate::services::federation::{self as fed, PresignItem};
use crate::services::shares::ReceivedPictureInfo;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use chrono::Utc;
use tracing::debug;

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
    debug!(
        user = "-",
        token_type = "federation",
        issuer_instance = %payload.issuer_instance,
        "federation: auth_grant"
    );
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
    let incoming_id = fed::receive_share_announcement(
        &state.db,
        &state.config,
        &auth.claims.sub,
        &payload.sender_username,
        &payload.sender_instance,
        &payload.recipient_username,
        &payload.recipient_instance,
        payload.outgoing_share_id,
        payload.share_token,
    )
    .await?;
    debug!(
        user = %auth.claims.sub,
        token_type = "federation",
        share_id = %incoming_id,
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
    let deleted = fed::receive_share_revoke(
        &state.db,
        state.cache.as_ref(),
        &auth.claims.sub,
        payload.outgoing_share_id,
    )
    .await?;
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

pub async fn reject_share(
    auth: AuthFederation,
    State(state): State<AppState>,
    Json(payload): Json<ShareRejectRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(
        user = %auth.claims.sub,
        token_type = "federation",
        outgoing_share_id = %payload.outgoing_share_id,
        "federation: reject_share"
    );
    fed::receive_share_reject(&state.db, &auth.claims.sub, payload.outgoing_share_id).await?;
    Ok(Json(serde_json::json!({ "rejected": true })))
}

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
    let announced = fed::receive_share_accept(
        &state.db,
        &state.federation,
        &state.config,
        &auth.claims.sub,
        payload.outgoing_share_id,
    )
    .await?;
    debug!(
        outgoing_share_id = %payload.outgoing_share_id,
        announced,
        "federation: pictures announced after share accept"
    );
    Ok(Json(serde_json::json!({ "announced": announced })))
}

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
    let pictures: Vec<ReceivedPictureInfo> = payload
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
    let registered = fed::receive_pictures_announcement(
        &state.db,
        &auth.claims.sub,
        &payload.sender_username,
        &payload.sender_instance,
        payload.outgoing_share_id,
        &payload.tag_path,
        &pictures,
    )
    .await?;
    debug!(
        outgoing_share_id = %payload.outgoing_share_id,
        registered,
        "federation: pictures registered"
    );
    Ok(Json(serde_json::json!({ "registered": registered })))
}

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
    let items: Vec<PresignItem> = payload
        .pictures
        .iter()
        .map(|p| PresignItem {
            picture_id: p.picture_id.clone(),
            variant: p.variant.clone(),
        })
        .collect();
    let results = fed::presign_batch_for_token(
        &state.db,
        state.cache.as_ref(),
        state.storage.as_ref(),
        &state.config,
        payload.share_token,
        &payload.owner_username,
        &payload.owner_instance,
        &items,
    )
    .await?;
    Ok(Json(PresignResponse {
        urls: results
            .into_iter()
            .map(|(id, url)| PresignResultItem {
                picture_id: id,
                url,
            })
            .collect(),
    }))
}
