/// Re-export shared federation protocol types from the client.
pub use crate::clients::federation::{FederationAuthGrant, ShareAnnouncement};

use serde::Deserialize;

/// Inbound request from a remote instance asking for a federation token.
///
/// `requester_instance` is the requester's global (WebFinger) domain.
/// `username` is a user on the requester's instance; this instance uses it to
/// resolve the requester's backend domain via WebFinger before sending the grant.
#[derive(Debug, Deserialize)]
pub struct FederationAuthRequest {
    pub requester_instance: String,
    pub username: String,
    pub scope: String,
    pub nonce: String,
}

#[derive(Debug, Deserialize)]
pub struct ShareRevokeRequest {
    pub incoming_share_id: uuid::Uuid,
}

#[derive(Debug, Deserialize)]
pub struct PicturesAnnouncement {
    pub outgoing_share_id: uuid::Uuid,
    pub picture_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct PresignRequest {
    pub owner_username: String,
    /// Global (WebFinger) domain of the owner's instance.
    pub owner_instance: String,
    pub picture_id: String,
    pub variant: Option<String>,
}
