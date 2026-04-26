use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use uuid::Uuid;

// User model
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub display_name: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

// Picture model
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Picture {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub picture_id: String,
    pub owner_username: Option<String>,
    pub owner_instance_domain: Option<String>,
    pub s3_key: String,
    pub s3_bucket: String,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub file_size: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub exif_data: Json<serde_json::Value>,
    pub metadata: Json<serde_json::Value>,
    pub deleted_at: Option<NaiveDateTime>,
    pub captured_at: Option<NaiveDateTime>,
    pub ingested_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

// Tag model
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Tag {
    pub id: Uuid,
    pub picture_id: Uuid,
    pub tag_path: String, // In practice, this would be ltree, but we'll store as string for simplicity
    pub is_virtual: bool,
    pub source: String, // In practice, this would be tag_source enum
    pub source_id: Option<Uuid>,
    pub assigned_at: NaiveDateTime,
}

// Share models
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct OutgoingShare {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub tag_path: String,
    pub recipient_username: String,
    pub recipient_instance: String,
    pub allow_share_back: bool,
    pub future: bool,
    pub status: String, // share_status enum
    pub created_at: NaiveDateTime,
    pub revoked_at: Option<NaiveDateTime>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct IncomingShare {
    pub id: Uuid,
    pub recipient_id: Uuid,
    pub sender_username: String,
    pub sender_instance: String,
    pub outgoing_share_id: Uuid,
    pub local_mapping_service_id: Option<Uuid>,
    pub status: String, // share_status enum
    pub created_at: NaiveDateTime,
    pub revoked_at: Option<NaiveDateTime>,
}

// Job model
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Job {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub job_type: String, // job_type enum
    pub status: String,   // job_status enum
    pub config: Json<serde_json::Value>,
    pub result: Json<serde_json::Value>,
    pub result_s3_keys: Vec<String>,
    pub error_message: Option<String>,
    pub retry_count: i32,
    pub max_retries: i32,
    pub idempotency_key: Option<String>,
    pub created_at: NaiveDateTime,
    pub started_at: Option<NaiveDateTime>,
    pub completed_at: Option<NaiveDateTime>,
}

// Federation message model
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct FederationMessage {
    pub id: Uuid,
    pub message_type: String, // federation_message_type enum
    pub direction: String,    // federation_direction enum
    pub sender_username: Option<String>,
    pub sender_instance: Option<String>,
    pub recipient_username: Option<String>,
    pub recipient_instance: Option<String>,
    pub outgoing_share_id: Option<Uuid>,
    pub incoming_share_id: Option<Uuid>,
    pub payload: Json<serde_json::Value>,
    pub status: String, // federation_status enum
    pub created_at: NaiveDateTime,
    pub sent_at: Option<NaiveDateTime>,
    pub delivered_at: Option<NaiveDateTime>,
    pub error_message: Option<String>,
    pub retry_count: i32,
}

// Hierarchy model
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Hierarchy {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub name: String,
    pub config: Json<serde_json::Value>,
    pub enabled: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

// Tagging service models
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct TaggingService {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub service_type: String, // service_type enum
    pub requires: Vec<String>,
    pub excludes: Vec<String>,
    pub enabled: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct SharedTagMappingService {
    pub id: Uuid,
    pub service_id: Uuid,
    pub incoming_share_id: Uuid,
    pub assign_tag: String,
    pub is_broken: bool,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct RuleTaggingService {
    pub id: Uuid,
    pub service_id: Uuid,
    pub predicate: String,
    pub assign_tag: String,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct SegmentationTaggingService {
    pub id: Uuid,
    pub service_id: Uuid,
    pub name: String,
    pub date_start: NaiveDateTime,
    pub date_end: NaiveDateTime,
    pub assign_tag: String,
    pub parent_segment_id: Option<Uuid>,
}
