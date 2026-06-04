// FederationAuthGrant is re-exported from the client because the same struct is used both
// to issue a grant (outbound, in auth_request handler) and to receive one (inbound, in
// auth_grant handler). All other inbound types are defined locally so API and wire formats
// can diverge independently.
pub use crate::clients::federation::FederationAuthGrant;

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Inbound request from a remote instance asking for a federation token.
#[derive(Debug, Deserialize)]
pub struct FederationAuthRequest {
    pub requester_instance: String,
    pub username: String,
    pub scope: String,
    pub nonce: String,
}

/// Inbound share announcement from the sender's backend.
#[derive(Debug, Deserialize)]
pub struct ShareAnnouncement {
    pub sender_username: String,
    pub sender_instance: String,
    pub recipient_username: String,
    pub recipient_instance: String,
    pub outgoing_share_id: Uuid,
    pub tag_path: String,
    pub allow_share_back: bool,
    pub future: bool,
    pub shareback_of: Option<Uuid>,
    pub share_token: Uuid,
}

/// Sent by the sender (Alice) to the recipient's (Bob's) backend to revoke a share.
/// Keyed by Alice's `outgoing_share_id`; Bob looks up the matching IncomingShare himself.
#[derive(Debug, Deserialize)]
pub struct ShareRevokeRequest {
    pub outgoing_share_id: Uuid,
}

/// Sent by the recipient (Bob) to the sender (Alice) to accept a share.
/// Alice will respond by announcing all current pictures under the shared tag.
#[derive(Debug, Deserialize)]
pub struct ShareAcceptRequest {
    pub outgoing_share_id: Uuid,
}

/// Sent by the recipient (Bob) to the sender (Alice) to reject a share.
/// Alice will tombstone her OutgoingShare.
#[derive(Debug, Deserialize)]
pub struct ShareRejectRequest {
    pub outgoing_share_id: Uuid,
}

/// A single picture entry inside an inbound pictures announcement.
#[derive(Debug, Deserialize)]
pub struct AnnouncedPicture {
    pub picture_id: String,
    pub owner_username: String,
    pub owner_instance_domain: String,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub file_size: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub captured_at: Option<NaiveDateTime>,
}

/// Inbound pictures announcement from the sender's backend after share acceptance.
#[derive(Debug, Deserialize)]
pub struct PicturesAnnouncement {
    pub outgoing_share_id: Uuid,
    pub tag_path: String,
    pub sender_username: String,
    pub sender_instance: String,
    pub pictures: Vec<AnnouncedPicture>,
}

/// One picture to presign inside a batch presign request.
#[derive(Debug, Deserialize)]
pub struct PresignPictureItem {
    pub picture_id: String,
    pub variant: Option<String>,
}

/// Batch presign request. Auth: `share_token` only — no federation JWT required.
#[derive(Debug, Deserialize)]
pub struct PresignRequest {
    pub owner_username: String,
    /// Global (WebFinger) domain of the owner's instance.
    pub owner_instance: String,
    pub share_token: Uuid,
    pub pictures: Vec<PresignPictureItem>,
}

#[derive(Debug, Serialize)]
pub struct PresignResultItem {
    pub picture_id: String,
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct PresignResponse {
    pub urls: Vec<PresignResultItem>,
}
