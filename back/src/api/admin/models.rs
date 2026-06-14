use crate::domain::job::{JobStatus, JobType};
use crate::repository::admin::{
    AdminJob, ConsistencyStats, ErroredShare, FederationInstance, InstanceStats, UserStats,
    UserWithStorage,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Request models ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub email: String,
    pub display_name: String,
    pub password: String,
    pub is_admin: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub display_name: Option<String>,
    pub is_admin: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ListJobsQuery {
    pub status: Option<JobStatus>,
    #[serde(rename = "type")]
    pub job_type: Option<JobType>,
    pub user_id: Option<Uuid>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

// ── Response models ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AdminUserResponse {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub display_name: String,
    pub is_admin: bool,
    pub storage_bytes: i64,
}

impl From<UserWithStorage> for AdminUserResponse {
    fn from(u: UserWithStorage) -> Self {
        Self {
            id: u.id,
            username: u.username,
            email: u.email,
            display_name: u.display_name,
            is_admin: u.is_admin,
            storage_bytes: u.storage_bytes,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct InstanceHealthResponse {
    pub global_domain: String,
    pub back_domain: String,
    pub db_connected: bool,
    pub redis_connected: bool,
    pub last_worker_activity_at: Option<String>,
}

pub type InstanceStatsResponse = InstanceStats;
pub type UserStatsResponse = UserStats;
pub type AdminJobResponse = AdminJob;
pub type ErroredShareResponse = ErroredShare;
pub type FederationInstanceResponse = FederationInstance;
pub type ConsistencyResponse = ConsistencyStats;
