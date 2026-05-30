use crate::AppState;
use crate::database::{
    count_users_per_backend, get_backend_url, list_backends, upsert_backend, upsert_mapping,
    username_exists,
};
use crate::error::AppError;
use axum::Json;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;
use axum::response::IntoResponse;
use chrono::Utc;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

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
    username: String,
    backend_url: String,
}
#[derive(Debug, Serialize)]
pub struct UpdateResponse {
    success: bool,
    message: String,
}

#[derive(Debug, Deserialize)]
pub struct RegisterBackendRequest {
    backend_url: String,
    name: String,
}
#[derive(Debug, Serialize)]
pub struct RegisterBackendResponse {
    success: bool,
    message: String,
}

#[derive(Debug, Serialize)]
pub struct ListBackendsResponse {
    backends: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    username: String,
    display_name: String,
    email: String,
    password: String,
}
#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    username: String,
    backend_url: String,
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
    headers: HeaderMap,
    Json(payload): Json<UpdateRequest>,
) -> Result<Json<UpdateResponse>, AppError> {
    let token = extract_bearer_token(&headers)?;
    verify_resolver_jwt(token, &state)?;

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

pub async fn register_backend_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<RegisterBackendRequest>,
) -> Result<Json<RegisterBackendResponse>, AppError> {
    let token = extract_bearer_token(&headers)?;
    verify_resolver_jwt(token, &state)?;

    if payload.backend_url.is_empty() || payload.name.is_empty() {
        return Err(AppError::BadRequest(
            "backend_url and name are required".to_string(),
        ));
    }

    upsert_backend(&state.db, &payload.backend_url, &payload.name).await?;

    info!(
        "Registered backend: {} ({})",
        payload.backend_url, payload.name
    );

    Ok(Json(RegisterBackendResponse {
        success: true,
        message: "Backend registered".to_string(),
    }))
}

pub async fn list_backends_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ListBackendsResponse>, AppError> {
    let token = extract_bearer_token(&headers)?;
    verify_resolver_jwt(token, &state)?;

    let backends = list_backends(&state.db).await?;

    Ok(Json(ListBackendsResponse { backends }))
}

pub async fn register_handler(
    State(state): State<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, AppError> {
    // Validate inputs
    if payload.username.is_empty() || payload.email.is_empty() {
        return Err(AppError::BadRequest(
            "username and email are required".to_string(),
        ));
    }

    // Check username not already taken
    if username_exists(&state.db, &payload.username).await? {
        return Err(AppError::BadRequest(format!(
            "Username '{}' is already taken",
            payload.username
        )));
    }

    // Get backend load counts
    let backend_counts = count_users_per_backend(&state.db).await?;
    if backend_counts.is_empty() {
        return Err(AppError::ServiceUnavailable(
            "No backend nodes registered".to_string(),
        ));
    }

    // Pick the backend with the lowest user count (first result from ordered query)
    let (chosen_backend_url, chosen_backend_host, _) = &backend_counts[0];

    // Generate a resolver JWT to authenticate with the backend
    let resolver_jwt = generate_resolver_jwt(&state, &chosen_backend_host)?;

    // Forward registration to the chosen backend
    let backend_register_url = format!("{}/api/resolver/users", chosen_backend_url);
    let body = serde_json::json!({
        "username": payload.username,
        "display_name": payload.display_name,
        "email": payload.email,
        "password": payload.password,
    });

    let response = state
        .reqwest_client
        .post(&backend_register_url)
        .header("Authorization", format!("Bearer {}", resolver_jwt))
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            warn!("Failed to contact backend {}: {}", chosen_backend_url, e);
            AppError::ServiceUnavailable(format!("Failed to contact backend: {e}"))
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let error_body = response
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        warn!(
            "Backend {} returned error {}: {}",
            chosen_backend_url, status, error_body
        );
        return Err(AppError::BackendError(status.as_u16(), error_body));
    }

    // Backend succeeded — record the mapping
    upsert_mapping(&state.db, &payload.username, &chosen_backend_url).await?;

    info!(
        "Registered user '{}' on backend '{}'",
        payload.username, chosen_backend_url
    );

    Ok(Json(RegisterResponse {
        username: payload.username,
        backend_url: chosen_backend_url.clone(),
        message: "User registered successfully".to_string(),
    }))
}

#[derive(Debug, Deserialize)]
struct ResolverJwtClaims {
    sub: String,
    is_admin: bool,
    instance: String,
    token_type: String,
    aud: String,
    iss: String,
    exp: i64,
    iat: i64,
    jti: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ResolverJwtClaimsEncode {
    sub: String,
    is_admin: bool,
    instance: String,
    token_type: String,
    aud: String,
    iss: String,
    exp: i64,
    iat: i64,
    jti: String,
}

pub async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "archypix-resolver"
    }))
}

fn extract_bearer_token(headers: &HeaderMap) -> Result<&str, AppError> {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(AppError::Unauthorized)
}

fn verify_resolver_jwt(token: &str, state: &AppState) -> Result<ResolverJwtClaims, AppError> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_audience(&[state.managed_domain.clone()]);
    let data = decode::<ResolverJwtClaims>(
        token,
        &DecodingKey::from_secret(state.resolver_admin_secret.as_bytes()),
        &validation,
    )
    .map_err(|_| AppError::Unauthorized)?;

    if data.claims.token_type != "resolver" {
        warn!("Invalid resolver token type");
        return Err(AppError::Unauthorized);
    }

    Ok(data.claims)
}

fn generate_resolver_jwt(state: &AppState, backend_host: &str) -> Result<String, AppError> {
    let now = Utc::now().timestamp();
    let claims = ResolverJwtClaimsEncode {
        sub: "resolver".to_string(),
        is_admin: false,
        instance: state.managed_domain.clone(),
        token_type: "resolver".to_string(),
        aud: backend_host.to_string(),
        iss: "resolver".to_string(),
        exp: now + 300,
        iat: now,
        jti: Uuid::new_v4().to_string(),
    };

    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(state.resolver_admin_secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(token)
}

fn parse_acct_resource(resource: &str, state: &AppState) -> Result<String, AppError> {
    // Expected format: archypix:@user:domain
    if let Some(rest) = resource.strip_prefix("archypix:@") {
        let mut iter = rest.split(':');
        let user = iter.next().ok_or(AppError::BadRequest(
            "Invalid resource format. Expected archypix:@user:domain".to_string(),
        ))?;
        let domain = iter.next().ok_or(AppError::BadRequest(
            "Invalid resource format. Expected archypix:@user:domain".to_string(),
        ))?;

        if domain != state.managed_domain {
            return Err(AppError::BadRequest(format!("Invalid domain: {}", domain)));
        }

        return Ok(user.to_string());
    }
    Err(AppError::BadRequest(
        "Invalid resource format. Expected archypix:@user:domain".to_string(),
    ))
}

fn build_webfinger_response(
    username: &str,
    backend_url: &str,
    managed_domain: &str,
) -> WebFingerResponse {
    WebFingerResponse {
        subject: format!("archypix:@{}:{}", username, managed_domain),
        links: vec![WebFingerLink {
            rel: "backend_url".to_string(),
            href: backend_url.to_string(),
        }],
    }
}
