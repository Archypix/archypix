/// Re-export shared federation protocol types from the client.
pub use crate::clients::federation::{FederationAuthGrant, ShareAnnouncement};

use serde::Deserialize;

/// Inbound request from a remote instance asking for a federation token.
#[derive(Debug, Deserialize)]
pub struct FederationAuthRequest {
    pub requester_instance: String,
    pub callback_url: String,
    pub scope: String,
    pub nonce: String,
}

#[derive(Debug, Deserialize)]
pub struct ShareRevokeRequest {
    pub incoming_share_id: uuid::Uuid,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PicturesAnnouncement {
    pub outgoing_share_id: uuid::Uuid,
    pub picture_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct PresignRequest {
    pub owner_username: String,
    pub owner_instance: String,
    pub picture_id: String,
    pub variant: Option<String>,
}
