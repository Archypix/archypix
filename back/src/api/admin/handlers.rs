use crate::api::admin::models::{
    AdminJobResponse, AdminUserResponse, ConsistencyResponse, CreateUserRequest,
    ErroredShareResponse, FederationInstanceResponse, InstanceHealthResponse,
    InstanceStatsResponse, ListJobsQuery, UpdateUserRequest, UserStatsResponse,
};
use crate::api::middleware::auth_admin::AuthAdmin;
use crate::infra::error::AppError;
use crate::infra::redis::{RedisKey, cache_get_json, cache_set_json_ex};
use crate::repository::admin::AdminRepository;
use crate::repository::share::{IncomingShareRepository, OutgoingShareRepository};
use crate::repository::user::UserRepository;
use crate::services;
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, Query, State};
use tracing::debug;
use uuid::Uuid;

const INSTANCE_STATS_TTL: u64 = 60;
const USER_STATS_TTL: u64 = 120;

// ── User management ───────────────────────────────────────────────────────────

pub async fn list_users(
    auth: AuthAdmin,
    State(state): State<AppState>,
) -> Result<Json<Vec<AdminUserResponse>>, AppError> {
    debug!(user = %auth.claims.sub, token_type = "admin", "admin: list_users");
    let users = AdminRepository::list_users_with_storage(&state.db).await?;
    Ok(Json(
        users.into_iter().map(AdminUserResponse::from).collect(),
    ))
}

pub async fn create_user(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Json(payload): Json<CreateUserRequest>,
) -> Result<Json<AdminUserResponse>, AppError> {
    debug!(user = %auth.claims.sub, token_type = "admin", username = %payload.username, "admin: create_user");
    let user = services::users::create_user(
        &state.db,
        &payload.username,
        &payload.email,
        &payload.display_name,
        &payload.password,
        payload.is_admin.unwrap_or(false),
    )
    .await?;
    Ok(Json(AdminUserResponse {
        id: user.id,
        username: user.username,
        email: user.email,
        display_name: user.display_name,
        is_admin: user.is_admin,
        storage_bytes: 0,
    }))
}

pub async fn update_user(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    Json(payload): Json<UpdateUserRequest>,
) -> Result<Json<AdminUserResponse>, AppError> {
    debug!(user = %auth.claims.sub, token_type = "admin", target_user_id = %user_id, "admin: update_user");
    let user = UserRepository::update(
        &state.db,
        user_id,
        payload.display_name.as_deref(),
        payload.is_admin,
    )
    .await?;
    // Storage is not available without an extra query here; return 0 for update responses.
    Ok(Json(AdminUserResponse {
        id: user.id,
        username: user.username,
        email: user.email,
        display_name: user.display_name,
        is_admin: user.is_admin,
        storage_bytes: 0,
    }))
}

pub async fn delete_user(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(user = %auth.claims.sub, token_type = "admin", target_user_id = %user_id, "admin: delete_user");
    UserRepository::delete(&state.db, user_id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

// ── Instance health ───────────────────────────────────────────────────────────

pub async fn get_instance(
    auth: AuthAdmin,
    State(state): State<AppState>,
) -> Result<Json<InstanceHealthResponse>, AppError> {
    debug!(user = %auth.claims.sub, token_type = "admin", "admin: get_instance");

    let db_connected = sqlx::query_scalar!("SELECT 1 AS ping")
        .fetch_one(&state.db)
        .await
        .is_ok();

    let redis_connected = state
        .cache
        .get_str(RedisKey::UploadSession(Uuid::nil()))
        .await
        .is_ok();

    let last_worker_activity_at = AdminRepository::instance_stats(&state.db)
        .await
        .ok()
        .and_then(|s| {
            s.last_worker_activity_at
                .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        });

    Ok(Json(InstanceHealthResponse {
        global_domain: state.config.global_domain.clone(),
        back_domain: state.config.back_domain.clone(),
        db_connected,
        redis_connected,
        last_worker_activity_at,
    }))
}

// ── Instance-wide analytics (cached) ─────────────────────────────────────────

pub async fn get_instance_stats(
    auth: AuthAdmin,
    State(state): State<AppState>,
) -> Result<Json<InstanceStatsResponse>, AppError> {
    debug!(user = %auth.claims.sub, token_type = "admin", "admin: get_instance_stats");

    if let Some(cached) =
        cache_get_json::<InstanceStatsResponse>(state.cache.as_ref(), RedisKey::AdminStats).await?
    {
        return Ok(Json(cached));
    }

    let stats = AdminRepository::instance_stats(&state.db).await?;
    let _ = cache_set_json_ex(
        state.cache.as_ref(),
        RedisKey::AdminStats,
        &stats,
        INSTANCE_STATS_TTL,
    )
    .await;
    Ok(Json(stats))
}

// ── Per-user analytics (cached) ───────────────────────────────────────────────

pub async fn get_user_stats(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<UserStatsResponse>, AppError> {
    debug!(user = %auth.claims.sub, token_type = "admin", target_user_id = %user_id, "admin: get_user_stats");
    UserRepository::find_by_id(&state.db, user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let key = RedisKey::AdminUserStats(user_id);
    if let Some(cached) = cache_get_json::<UserStatsResponse>(state.cache.as_ref(), key).await? {
        return Ok(Json(cached));
    }

    let stats = AdminRepository::user_stats(&state.db, user_id).await?;
    let _ = cache_set_json_ex(state.cache.as_ref(), key, &stats, USER_STATS_TTL).await;
    Ok(Json(stats))
}

// ── User shares ───────────────────────────────────────────────────────────────

pub async fn get_user_shares(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(user = %auth.claims.sub, token_type = "admin", target_user_id = %user_id, "admin: get_user_shares");
    UserRepository::find_by_id(&state.db, user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let outgoing = OutgoingShareRepository::list_by_owner(&state.db, user_id).await?;
    let incoming = IncomingShareRepository::list_by_recipient(&state.db, user_id).await?;

    Ok(Json(serde_json::json!({
        "outgoing": outgoing,
        "incoming": incoming,
    })))
}

// ── Pipeline wake ─────────────────────────────────────────────────────────────

pub async fn wake_user_pipeline(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(user = %auth.claims.sub, token_type = "admin", target_user_id = %user_id, "admin: wake_user_pipeline");
    UserRepository::find_by_id(&state.db, user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    state.pipeline_waker.wake(user_id);
    Ok(Json(serde_json::json!({ "woken": true })))
}

// ── Job list ──────────────────────────────────────────────────────────────────

pub async fn list_jobs(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Query(query): Query<ListJobsQuery>,
) -> Result<Json<Vec<AdminJobResponse>>, AppError> {
    debug!(user = %auth.claims.sub, token_type = "admin", "admin: list_jobs");
    let limit = query.limit.clamp(1, 200);
    let jobs = AdminRepository::list_jobs(
        &state.db,
        query.status,
        query.job_type,
        query.user_id,
        limit,
        query.offset,
    )
    .await?;
    Ok(Json(jobs))
}

// ── Stale jobs ────────────────────────────────────────────────────────────────

pub async fn list_stale_jobs(
    auth: AuthAdmin,
    State(state): State<AppState>,
) -> Result<Json<Vec<AdminJobResponse>>, AppError> {
    debug!(user = %auth.claims.sub, token_type = "admin", "admin: list_stale_jobs");
    let jobs =
        AdminRepository::list_stale_jobs(&state.db, state.config.job_processing_timeout_secs)
            .await?;
    Ok(Json(jobs))
}

// ── Job reset ─────────────────────────────────────────────────────────────────

pub async fn reset_job(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Path(job_id): Path<Uuid>,
) -> Result<Json<AdminJobResponse>, AppError> {
    debug!(user = %auth.claims.sub, token_type = "admin", job_id = %job_id, "admin: reset_job");
    AdminRepository::reset_job(&state.db, job_id)
        .await?
        .ok_or(AppError::NotFound)
        .map(Json)
}

// ── Job cancel ────────────────────────────────────────────────────────────────

pub async fn cancel_job(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Path(job_id): Path<Uuid>,
) -> Result<Json<AdminJobResponse>, AppError> {
    debug!(user = %auth.claims.sub, token_type = "admin", job_id = %job_id, "admin: cancel_job");
    AdminRepository::cancel_job(&state.db, job_id)
        .await?
        .ok_or(AppError::NotFound)
        .map(Json)
}

// ── Errored shares (global) ───────────────────────────────────────────────────

pub async fn list_errored_shares(
    auth: AuthAdmin,
    State(state): State<AppState>,
) -> Result<Json<Vec<ErroredShareResponse>>, AppError> {
    debug!(user = %auth.claims.sub, token_type = "admin", "admin: list_errored_shares");
    let shares = AdminRepository::list_errored_shares(&state.db).await?;
    Ok(Json(shares))
}

// ── Force-reconcile a share ───────────────────────────────────────────────────

pub async fn force_reconcile_share(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Path(share_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    debug!(user = %auth.claims.sub, token_type = "admin", share_id = %share_id, "admin: force_reconcile_share");
    let owner_id = AdminRepository::clear_share_backoff(&state.db, share_id)
        .await?
        .ok_or(AppError::NotFound)?;

    state.pipeline_waker.wake(owner_id);
    Ok(Json(serde_json::json!({ "reconcile_triggered": true })))
}

// ── Active federation connections (Redis token cache) ─────────────────────────

pub async fn list_active_federation_connections(
    auth: AuthAdmin,
    State(state): State<AppState>,
) -> Result<Json<Vec<String>>, AppError> {
    debug!(user = %auth.claims.sub, token_type = "admin", "admin: list_active_federation_connections");
    let keys = state.cache.scan_keys("federation:token:*").await?;
    const PREFIX: &str = "federation:token:";
    let mut domains: Vec<String> = keys
        .into_iter()
        .filter_map(|k| k.strip_prefix(PREFIX).map(str::to_string))
        .collect();
    domains.sort();
    Ok(Json(domains))
}

// ── Federation instances ──────────────────────────────────────────────────────

pub async fn list_federation_instances(
    auth: AuthAdmin,
    State(state): State<AppState>,
) -> Result<Json<Vec<FederationInstanceResponse>>, AppError> {
    debug!(user = %auth.claims.sub, token_type = "admin", "admin: list_federation_instances");
    let instances = AdminRepository::list_federation_instances(&state.db).await?;
    Ok(Json(instances))
}

// ── Consistency check ─────────────────────────────────────────────────────────

pub async fn get_consistency(
    auth: AuthAdmin,
    State(state): State<AppState>,
) -> Result<Json<ConsistencyResponse>, AppError> {
    debug!(user = %auth.claims.sub, token_type = "admin", "admin: get_consistency");
    let stats = AdminRepository::consistency_stats(&state.db).await?;
    Ok(Json(stats))
}
