use crate::database::{get_backend_url, upsert_mapping};
use crate::error::AppError;
use crate::AppState;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

#[derive(Debug, Deserialize)]
pub struct WebFingerQuery {
    resource: String,
}
#[derive(Debug, Serialize)]
pub struct WebFingerResponse {
    subject: String,
    links: Vec<WebFingerLink>,
}
#[derive(Debug, Serialize)]
pub struct WebFingerLink {
    rel: String,
    href: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRequest {
    token: String,
    username: String,
    backend_url: String,
}
#[derive(Debug, Serialize)]
pub struct UpdateResponse {
    success: bool,
    message: String,
}

pub async fn webfinger_handler(
    Query(query): Query<WebFingerQuery>,
    State(state): State<AppState>,
) -> Result<Json<WebFingerResponse>, AppError> {
    // Parse acct:username@domain from resource
    let username = parse_acct_resource(&query.resource, &state)?;

    // Check cache first
    if let Some(backend_url) = state.cache.get(&username).await {
        info!("Cache hit for username: {}", username);
        return Ok(Json(build_webfinger_response(
            &username,
            &backend_url,
            &state.managed_domain,
        )));
    }

    // Cache miss - query database
    let backend_url = get_backend_url(&state.db, &username).await?;

    if let Some(backend_url) = backend_url {
        info!("Database hit for username: {}", username);
        // Update cache
        state
            .cache
            .insert(username.clone(), backend_url.clone())
            .await;

        Ok(Json(build_webfinger_response(
            &username,
            &backend_url,
            &state.managed_domain,
        )))
    } else {
        warn!("Unknown username: {}", username);
        Err(AppError::NotFound)
    }
}

pub async fn update_handler(
    State(state): State<AppState>,
    Json(payload): Json<UpdateRequest>,
) -> Result<Json<UpdateResponse>, AppError> {
    // Verify admin token
    if payload.token != state.admin_token {
        warn!(
            "Invalid admin token attempt for username: {}",
            payload.username
        );
        return Err(AppError::Unauthorized);
    }

    // Validate inputs
    if payload.username.is_empty() || payload.backend_url.is_empty() {
        return Err(AppError::BadRequest(
            "Username and backend_url are required".to_string(),
        ));
    }

    // Update database
    upsert_mapping(&state.db, &payload.username, &payload.backend_url).await?;

    // Invalidate/update cache
    state
        .cache
        .insert(payload.username.clone(), payload.backend_url.clone())
        .await;

    info!(
        "Updated mapping: {} -> {}",
        payload.username, payload.backend_url
    );

    Ok(Json(UpdateResponse {
        success: true,
        message: format!("Mapping updated for user {}", payload.username),
    }))
}

pub async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "archypix-resolver"
    }))
}

fn parse_acct_resource(resource: &str, state: &AppState) -> Result<String, AppError> {
    // Expected format: acct:username@domain or just username
    if let Some(username) = resource.strip_prefix("acct:@") {
        let mut iter = username.split(':');
        let user = iter.next().ok_or(AppError::BadRequest(
            "Invalid account format. Excepted acct:@user:domain".to_string(),
        ))?;
        let domain = iter.next().ok_or(AppError::BadRequest(
            "Invalid account format Excepted acct:@user:domain".to_string(),
        ))?;

        if domain != state.managed_domain {
            return Err(AppError::BadRequest(format!("Invalid domain: {}", domain)));
        }

        return Ok(user.to_string());
    }
    Err(AppError::BadRequest(
        "Invalid account forma Excepted acct:@user:domain".to_string(),
    ))
}

fn build_webfinger_response(
    username: &str,
    backend_url: &str,
    managed_domain: &str,
) -> WebFingerResponse {
    WebFingerResponse {
        subject: format!("acct:@{}:{}", username, managed_domain),
        links: vec![WebFingerLink {
            rel: "backend_url".to_string(),
            href: backend_url.to_string(),
        }],
    }
}
