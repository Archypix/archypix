use crate::api::federation::models::{
    FederationAuthGrant, FederationAuthRequest, PicturesAnnouncement, PresignRequest,
    ShareAnnouncement, ShareRevokeRequest,
};
use crate::api::middleware::auth_federation::AuthFederation;
use crate::domain::share::ShareStatus;
use crate::infra::error::AppError;
use crate::repository::picture::PictureRepository;
use crate::repository::share::{IncomingShareRepository, OutgoingShareRepository};
use crate::repository::user::UserRepository;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use chrono::Utc;
use tracing::info;
use uuid::Uuid;

pub async fn auth_request(
    State(state): State<AppState>,
    Json(payload): Json<FederationAuthRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !payload.callback_url.contains(&payload.requester_instance) {
        return Err(AppError::BadRequest(
            "Callback URL does not match requester instance".to_string(),
        ));
    }

    let token = state
        .federation
        .issue_federation_token(&payload.requester_instance)?;
    let expires_at = Utc::now().timestamp() + state.config.federation_jwt_ttl_secs;

    state
        .federation
        .send_auth_grant(
            &payload.callback_url,
            &FederationAuthGrant {
                issuer_instance: state.config.host.clone(),
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
    _auth: AuthFederation,
    State(state): State<AppState>,
    Json(payload): Json<ShareAnnouncement>,
) -> Result<Json<serde_json::Value>, AppError> {
    let recipient = UserRepository::find_by_username(&state.db, &payload.recipient_username)
        .await?
        .ok_or(AppError::NotFound)?;

    let incoming = IncomingShareRepository::create(
        &state.db,
        recipient.id,
        &payload.sender_username,
        &payload.sender_instance,
        payload.outgoing_share_id,
    )
    .await?;

    info!(
        "Incoming share {} stored from {}@{}",
        incoming.id, payload.sender_username, payload.sender_instance
    );
    Ok(Json(serde_json::json!({ "accepted": true })))
}

pub async fn revoke_share(
    _auth: AuthFederation,
    State(state): State<AppState>,
    Json(payload): Json<ShareRevokeRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    IncomingShareRepository::set_status(&state.db, payload.incoming_share_id, ShareStatus::Revoked)
        .await?;
    Ok(Json(serde_json::json!({ "revoked": true })))
}

pub async fn announce_pictures(
    _auth: AuthFederation,
    _state: State<AppState>,
    Json(_payload): Json<PicturesAnnouncement>,
) -> Result<Json<serde_json::Value>, AppError> {
    Ok(Json(serde_json::json!({ "accepted": true })))
}

pub async fn presign_picture(
    auth: AuthFederation,
    State(state): State<AppState>,
    Json(payload): Json<PresignRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if payload.owner_instance != state.config.webfinger_host {
        return Err(AppError::BadRequest("Invalid owner instance".to_string()));
    }

    let owner = UserRepository::find_by_username(&state.db, &payload.owner_username)
        .await?
        .ok_or(AppError::NotFound)?;

    let allowed = OutgoingShareRepository::has_active_share_for_instance(
        &state.db,
        owner.id,
        &auth.claims.sub,
    )
    .await?;

    if !allowed {
        return Err(AppError::Unauthorized(
            "No active share for requesting instance".to_string(),
        ));
    }

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

    let url = state
        .storage
        .presign_get(&state.config.s3_bucket_originals, &picture.s3_key_original)
        .await?;
    Ok(Json(serde_json::json!({ "url": url })))
}
