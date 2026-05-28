use crate::api::federation::models::{
    FederationAuthGrant, FederationAuthRequest, PicturesAnnouncement, PresignRequest,
    ShareAnnouncement, ShareRevokeRequest,
};
use crate::api::middleware::auth_federation::AuthFederation;
use crate::database::picture::PictureRepository;
use crate::database::shares::{IncomingShareRepository, OutgoingShareRepository};
use crate::database::user::UserRepository;
use crate::infrastructure::error::AppError;
use crate::infrastructure::state::AppState;
use crate::services::federation::FederationService;
use crate::services::storage::StorageService;
use axum::Json;
use axum::extract::State;
use chrono::Utc;
use std::time::Duration;
use tracing::info;

pub async fn auth_request(
    State(state): State<AppState>,
    Json(payload): Json<FederationAuthRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !payload.callback_url.contains(&payload.requester_instance) {
        return Err(AppError::BadRequest(
            "Callback URL does not match requester instance".to_string(),
        ));
    }

    let federation = FederationService::new(
        state.http.clone(),
        state.config.clone(),
        state.jwt.clone(),
        state.redis.clone(),
    );

    let token = federation.issue_federation_token(&payload.requester_instance)?;
    let expires_at = Utc::now().timestamp() + state.config.federation_jwt_ttl_secs;

    let grant = FederationAuthGrant {
        issuer_instance: state.config.host.clone(),
        token,
        expires_at,
        scope: payload.scope,
        nonce: payload.nonce,
    };

    let response = state
        .http
        .post(&payload.callback_url)
        .json(&grant)
        .send()
        .await
        .map_err(|err| AppError::InternalServerError(err.to_string()))?;

    if !response.status().is_success() {
        return Err(AppError::InternalServerError(format!(
            "Callback rejected grant: {}",
            response.status()
        )));
    }

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

    let federation = FederationService::new(
        state.http.clone(),
        state.config.clone(),
        state.jwt.clone(),
        state.redis.clone(),
    );
    federation
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
        .ok_or_else(|| AppError::NotFound)?;

    let incoming = IncomingShareRepository::create(
        &state.db,
        recipient.id,
        &payload.sender_username,
        &payload.sender_instance,
        payload.outgoing_share_id,
    )
    .await?;

    info!("Incoming share {} stored", incoming.id);

    Ok(Json(serde_json::json!({ "accepted": true })))
}

pub async fn revoke_share(
    _auth: AuthFederation,
    State(state): State<AppState>,
    Json(payload): Json<ShareRevokeRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    IncomingShareRepository::set_status(&state.db, payload.incoming_share_id, "revoked").await?;
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

    let requester_instance = auth.claims.sub;
    let allowed = OutgoingShareRepository::has_active_share_for_instance(
        &state.db,
        owner.id,
        &requester_instance,
    )
    .await?;

    if !allowed {
        return Err(AppError::Unauthorized("Share not allowed".to_string()));
    }

    let picture =
        PictureRepository::find_owned_by_picture_id(&state.db, owner.id, &payload.picture_id)
            .await?
            .ok_or(AppError::NotFound)?;

    let storage = StorageService::new(
        state.s3.clone(),
        state.config.s3_bucket.clone(),
        Duration::from_secs(state.config.s3_presign_ttl_secs),
    );
    let url = storage
        .presign_get_in_bucket(&picture.s3_bucket, &picture.s3_key)
        .await?;

    Ok(Json(serde_json::json!({ "url": url })))
}
