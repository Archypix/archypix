use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "federation_message_type", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum FederationMessageType {
    ShareAnnouncement,
    ShareRevocation,
    PictureUpdate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "federation_direction", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum FederationDirection {
    Inbound,
    Outbound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "federation_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum FederationStatus {
    Pending,
    Sent,
    Delivered,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct FederationMessage {
    pub id: Uuid,
    pub message_type: FederationMessageType,
    pub direction: FederationDirection,
    pub sender_username: Option<String>,
    pub sender_instance: Option<String>,
    pub recipient_username: Option<String>,
    pub recipient_instance: Option<String>,
    pub outgoing_share_id: Option<Uuid>,
    pub incoming_share_id: Option<Uuid>,
    pub payload: Json<serde_json::Value>,
    pub idempotency_key: Option<String>,
    pub status: FederationStatus,
    pub created_at: NaiveDateTime,
    pub sent_at: Option<NaiveDateTime>,
    pub delivered_at: Option<NaiveDateTime>,
    pub error_message: Option<String>,
    pub retry_count: i32,
}

/// Resolved mapping from a user identity to the owning backend domain.
#[derive(Debug, Clone)]
pub struct BackendMapping {
    pub username: String,
    pub instance_domain: String,
    pub backend_domain: String,
}
