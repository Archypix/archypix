use crate::api::middleware::auth_user::AuthUser;
use crate::database::models::ShareStatus;
use crate::database::shares::{IncomingShareRepository, OutgoingShareRepository};
use crate::infrastructure::error::AppError;
use crate::infrastructure::state::AppState;
use crate::services::federation::FederationService;
use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct CreateOutgoingRequest {
    pub tag_path: String,
    pub recipient_username: String,
    pub recipient_instance: String,
    pub allow_share_back: Option<bool>,
    pub future: Option<bool>,
    pub shareback_of: Option<uuid::Uuid>,
}

#[derive(Debug, Serialize)]
pub struct ShareResponse {
    pub id: uuid::Uuid,
    pub tag_path: String,
    pub recipient_username: String,
    pub recipient_instance: String,
    pub status: ShareStatus,
}

pub async fn create_outgoing(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<CreateOutgoingRequest>,
) -> Result<Json<ShareResponse>, AppError> {
    let user_id = auth
        .claims
        .uid
        .ok_or_else(|| AppError::Unauthorized("Missing user id".to_string()))?;

    let allow_share_back = payload.allow_share_back.unwrap_or(true);
    let future = payload.future.unwrap_or(true);

    let share = OutgoingShareRepository::create(
        &state.db,
        user_id,
        &payload.tag_path,
        &payload.recipient_username,
        &payload.recipient_instance,
        allow_share_back,
        future,
    )
    .await?;

    let federation = FederationService::new(
        state.http.clone(),
        state.config.clone(),
        state.jwt.clone(),
        state.redis.clone(),
    );
    let backend_domain = federation
        .resolve_backend_domain(&payload.recipient_username, &payload.recipient_instance)
        .await?;
    let token = federation
        .get_or_wait_federation_token(&backend_domain)
        .await?;

    let announcement = serde_json::json!({
        "sender_username": auth.claims.sub,
        "sender_instance": state.config.webfinger_host,
        "recipient_username": payload.recipient_username,
        "recipient_instance": payload.recipient_instance,
        "outgoing_share_id": share.id,
        "tag_path": payload.tag_path,
        "allow_share_back": allow_share_back,
        "future": future,
        "shareback_of": payload.shareback_of
    });

    let url = format!(
        "{}://{}/api/federation/shares/announce",
        state.config.federation_scheme, backend_domain
    );
    state
        .http
        .post(url)
        .bearer_auth(token)
        .json(&announcement)
        .send()
        .await
        .map_err(|err| AppError::InternalServerError(err.to_string()))?
        .error_for_status()
        .map_err(|err| AppError::InternalServerError(err.to_string()))?;

    Ok(Json(ShareResponse {
        id: share.id,
        tag_path: share.tag_path,
        recipient_username: share.recipient_username,
        recipient_instance: share.recipient_instance,
        status: share.status,
    }))
}

pub async fn list_outgoing(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<ShareResponse>>, AppError> {
    let user_id = auth
        .claims
        .uid
        .ok_or_else(|| AppError::Unauthorized("Missing user id".to_string()))?;
    let shares = OutgoingShareRepository::list_by_owner(&state.db, user_id).await?;
    Ok(Json(
        shares
            .into_iter()
            .map(|share| ShareResponse {
                id: share.id,
                tag_path: share.tag_path,
                recipient_username: share.recipient_username,
                recipient_instance: share.recipient_instance,
                status: share.status,
            })
            .collect(),
    ))
}

pub async fn list_incoming(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let user_id = auth
        .claims
        .uid
        .ok_or_else(|| AppError::Unauthorized("Missing user id".to_string()))?;
    let shares = IncomingShareRepository::list_by_recipient(&state.db, user_id).await?;
    Ok(Json(
        shares
            .into_iter()
            .map(|share| {
                serde_json::json!({
                    "id": share.id,
                    "sender_username": share.sender_username,
                    "sender_instance": share.sender_instance,
                    "outgoing_share_id": share.outgoing_share_id,
                    "status": share.status,
                })
            })
            .collect(),
    ))
}

pub async fn accept_incoming(
    _auth: AuthUser,
    State(state): State<AppState>,
    Path(share_id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    IncomingShareRepository::set_status(&state.db, share_id, ShareStatus::Active).await?;
    Ok(Json(serde_json::json!({ "accepted": true })))
}

pub async fn reject_incoming(
    _auth: AuthUser,
    State(state): State<AppState>,
    Path(share_id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    IncomingShareRepository::set_status(&state.db, share_id, ShareStatus::Tombstoned).await?;
    Ok(Json(serde_json::json!({ "rejected": true })))
}
