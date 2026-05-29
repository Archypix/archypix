use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use uuid::Uuid;

// ============================================================================
// Enums (mirror PostgreSQL enum types)
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "share_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum ShareStatus {
    Active,
    Revoked,
    Tombstoned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "tag_source", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum TagSource {
    Manual,
    Rule,
    Segment,
    ShareMapping,
    IncomingShare,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "job_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "job_type", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum JobType {
    GenThumbnail,
    MlStyle,
    MlPeople,
    MlGroupLocation,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "service_type", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ServiceType {
    SharedTagMapping,
    Rule,
    Segmentation,
}

// Used only in hierarchy JSONB config, not a direct column type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SafeDeleteMode {
    SingleBranch,
    FullDelete,
}

// User model
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub display_name: String,
    pub is_admin: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct UserCredential {
    pub user_id: Uuid,
    pub password_hash: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct RefreshToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: NaiveDateTime,
    pub revoked_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

// Picture model
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Picture {
    pub id: Uuid,
    pub local_user_id: Uuid,
    pub remote_picture_id: Option<String>, // only set for received pictures
    pub owner_username: Option<String>,
    pub owner_instance_domain: Option<String>,
    pub s3_key_original: String,
    pub s3_key_small: Option<String>,
    pub s3_key_medium: Option<String>,
    pub s3_key_large: Option<String>,
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
    pub tag_path: String, // ltree stored as text via ::text cast
    pub source: TagSource,
    pub source_id: Option<Uuid>,
    pub assigned_at: NaiveDateTime,
}

// Share models
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct OutgoingShare {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub tag_path: String, // ltree stored as text via ::text cast
    pub recipient_username: String,
    pub recipient_instance: String,
    pub allow_share_back: bool,
    pub future: bool,
    pub status: ShareStatus,
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
    pub status: ShareStatus,
    pub created_at: NaiveDateTime,
    pub revoked_at: Option<NaiveDateTime>,
}

// Job model
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Job {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub job_type: JobType,
    pub status: JobStatus,
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
    pub service_type: ServiceType,
    pub requires: Vec<String>, // LTREE[] read as text[]
    pub excludes: Vec<String>, // LTREE[] read as text[]
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
