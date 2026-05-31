use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "share_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum ShareStatus {
    Active,
    Revoked,
    Tombstoned,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct OutgoingShare {
    pub id: Uuid,
    pub owner_id: Uuid,
    /// ltree stored as text.
    pub tag_path: String,
    pub recipient_username: String,
    pub recipient_instance: String,
    pub allow_share_back: bool,
    pub future: bool,
    pub status: ShareStatus,
    /// Opaque token used to authorize presign requests for transitive federation shares.
    pub share_token: Uuid,
    pub created_at: NaiveDateTime,
    pub revoked_at: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct IncomingShare {
    pub id: Uuid,
    pub recipient_id: Uuid,
    pub sender_username: String,
    pub sender_instance: String,
    pub outgoing_share_id: Uuid,
    pub local_mapping_service_id: Option<Uuid>,
    pub status: ShareStatus,
    /// Share token from the upstream sender, forwarded here for transitive presign authorization.
    pub origin_share_token: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub revoked_at: Option<NaiveDateTime>,
}

impl OutgoingShare {
    /// True when announcing this share to `recipient_instance` would loop back to
    /// the picture's original owner (federation loop prevention).
    pub fn would_loop_to(&self, owner_instance: &str) -> bool {
        self.recipient_instance == owner_instance
    }
}
