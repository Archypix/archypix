use crate::api::federation::models::{
    FederationAuthGrant, FederationAuthRequest, PicturesAnnouncement, PresignRequest,
    ShareAnnouncement, ShareRevokeRequest,
};
use crate::api::middleware::auth_federation::AuthFederation;
use crate::domain::share::ShareStatus;
use crate::infra::error::AppError;
use crate::infra::s3;
use crate::repository::picture::PictureRepository;
use crate::repository::share::{IncomingShareRepository, OutgoingShareRepository};
use crate::repository::user::UserRepository;
use crate::services::pictures::PictureVariant;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use chrono::Utc;
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
        share_id = %payload.incoming_share_id,
        "federation: revoke_share"
    );
    let share = IncomingShareRepository::get_by_id(&state.db, payload.incoming_share_id).await?;
    if share.sender_instance != auth.claims.sub {
        return Err(AppError::Unauthorized(
            "Sender instance does not match authenticated instance".to_string(),
        ));
    }

    IncomingShareRepository::set_status(&state.db, payload.incoming_share_id, ShareStatus::Revoked)
        .await?;

    Ok(Json(serde_json::json!({ "revoked": true })))
}

pub async fn announce_pictures(
    _auth: AuthFederation,
    _state: State<AppState>,
    Json(_payload): Json<PicturesAnnouncement>,
) -> Result<Json<serde_json::Value>, AppError> {
    // TODO: Implement
    Ok(Json(serde_json::json!({ "accepted": true })))
}

// No federation JWT required — the share_token is the sole proof of authorization.
// This avoids the federation handshake for every blob fetch, keeping presign requests cheap.
pub async fn presign_picture(
    State(state): State<AppState>,
    Json(payload): Json<PresignRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(
        owner = %payload.owner_username,
        owner_instance = %payload.owner_instance,
        picture_id = %payload.picture_id,
        variant = ?payload.variant,
        "federation: presign_picture"
    );
    if payload.owner_instance != state.config.global_domain {
        return Err(AppError::BadRequest("Invalid owner instance".to_string()));
    }

    let share_token = payload
        .share_token
        .ok_or_else(|| AppError::Unauthorized("share_token required".to_string()))?;

    let allowed =
        OutgoingShareRepository::has_active_share_for_token(&state.db, share_token).await?;
    if !allowed {
        return Err(AppError::Unauthorized(
            "share_token does not match any active share".to_string(),
        ));
    }

    let owner = UserRepository::find_by_username(&state.db, &payload.owner_username)
        .await?
        .ok_or(AppError::NotFound)?;

    let picture_id: Uuid = payload
        .picture_id
        .parse()
        .map_err(|_| AppError::BadRequest("Invalid picture_id".to_string()))?;

    let picture = PictureRepository::find_by_id(&state.db, picture_id)
        .await?
        .ok_or(AppError::NotFound)?;

    if picture.local_user_id != owner.id || !picture.is_owned() {
        return Err(AppError::NotFound);
    }

    let variant: PictureVariant = payload.variant.as_deref().unwrap_or("original").parse()?;
    let bucket = variant.bucket(&state.config);
    let key = s3::picture_key(picture.local_user_id, picture.id);
    let url = state.storage.presign_get(bucket, &key).await?;
    Ok(Json(serde_json::json!({ "url": url })))
}
