use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct FederationAuthRequest {
    pub requester_instance: String,
    pub callback_url: String,
    pub scope: String,
    pub nonce: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FederationAuthGrant {
    pub issuer_instance: String,
    pub token: String,
    pub expires_at: i64,
    pub scope: String,
    pub nonce: String,
}

#[derive(Debug, Deserialize)]
pub struct ShareAnnouncement {
    pub sender_username: String,
    pub sender_instance: String,
    pub recipient_username: String,
    pub recipient_instance: String,
    pub outgoing_share_id: uuid::Uuid,
    pub tag_path: String,
    pub allow_share_back: bool,
    pub future: bool,
    pub shareback_of: Option<uuid::Uuid>,
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
