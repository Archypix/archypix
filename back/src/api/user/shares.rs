use crate::api::middleware::auth_user::AuthUser;
use crate::domain::share::ShareStatus;
use crate::infra::error::AppError;
use crate::repository::share::{IncomingShareRepository, OutgoingShareRepository};
use crate::services;
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};
use tracing::debug;

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

#[derive(Debug, Serialize)]
pub struct IncomingShareResponse {
    pub id: uuid::Uuid,
    pub sender_username: String,
    pub sender_instance: String,
    pub outgoing_share_id: uuid::Uuid,
    pub status: ShareStatus,
}

pub async fn create_outgoing(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<CreateOutgoingRequest>,
) -> Result<Json<ShareResponse>, AppError> {
    debug!(
        user = %auth.claims.sub,
        token_type = auth.token_type(),
        tag_path = %payload.tag_path,
        recipient = %payload.recipient_username,
        "create_outgoing_share"
    );
    let share = services::shares::create_outgoing_share(
        &state.db,
        &state.redis,
        &state.federation,
        &state.config,
        auth.user_id()?,
        &auth.claims.sub,
        &payload.tag_path,
        &payload.recipient_username,
        &payload.recipient_instance,
        payload.allow_share_back.unwrap_or(true),
        payload.future.unwrap_or(true),
        payload.shareback_of,
    )
    .await?;
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
    debug!(user = %auth.claims.sub, token_type = auth.token_type(), "list_outgoing_shares");
    let shares = OutgoingShareRepository::list_by_owner(&state.db, auth.user_id()?).await?;
    Ok(Json(
        shares
            .into_iter()
            .map(|s| ShareResponse {
                id: s.id,
                tag_path: s.tag_path,
                recipient_username: s.recipient_username,
                recipient_instance: s.recipient_instance,
                status: s.status,
            })
            .collect(),
    ))
}

pub async fn list_incoming(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<IncomingShareResponse>>, AppError> {
    debug!(user = %auth.claims.sub, token_type = auth.token_type(), "list_incoming_shares");
    let shares = IncomingShareRepository::list_by_recipient(&state.db, auth.user_id()?).await?;
    Ok(Json(
        shares
            .into_iter()
            .map(|s| IncomingShareResponse {
                id: s.id,
                sender_username: s.sender_username,
                sender_instance: s.sender_instance,
                outgoing_share_id: s.outgoing_share_id,
                status: s.status,
            })
            .collect(),
    ))
}

pub async fn accept_incoming(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(share_id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(user = %auth.claims.sub, token_type = auth.token_type(), share_id = %share_id, "accept_incoming_share");
    let registered = services::shares::accept_incoming_share(
        &state.db,
        &state.redis,
        &state.federation,
        &state.config,
        auth.user_id()?,
        &auth.claims.sub,
        share_id,
    )
    .await?;
    Ok(Json(
        serde_json::json!({ "accepted": true, "pictures_registered": registered }),
    ))
}

pub async fn revoke_outgoing(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(share_id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(user = %auth.claims.sub, token_type = auth.token_type(), share_id = %share_id, "revoke_outgoing_share");
    services::shares::revoke_outgoing_share(
        &state.db,
        &state.redis,
        &state.federation,
        &state.config,
        auth.user_id()?,
        &auth.claims.sub,
        share_id,
    )
    .await?;
    Ok(Json(serde_json::json!({ "revoked": true })))
}

pub async fn reject_incoming(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(share_id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(user = %auth.claims.sub, token_type = auth.token_type(), share_id = %share_id, "reject_incoming_share");
    services::shares::reject_incoming_share(
        &state.db,
        &state.redis,
        &state.federation,
        &state.config,
        auth.user_id()?,
        &auth.claims.sub,
        share_id,
    )
    .await?;
    Ok(Json(serde_json::json!({ "rejected": true })))
}
