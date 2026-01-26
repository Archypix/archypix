use axum::{
    extract::{Query, State}, http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json,
    Router,
};
use moka::future::Cache;
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::time::Duration;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

// ============================================================================
// Configuration
// ============================================================================

#[derive(Clone)]
struct Config {
    database_url: String,
    managed_domain: String,
    admin_token: String,
    listen_addr: String,
    cache_ttl_secs: u64,
    cache_max_capacity: u64,
}

impl Config {
    fn from_env() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();

        Ok(Config {
            database_url: std::env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgres://archypix:archypix@localhost/archypix_resolver".to_string()
            }),
            managed_domain: std::env::var("MANAGED_DOMAIN")
                .unwrap_or_else(|_| "archypix.com".to_string()),
            admin_token: std::env::var("ADMIN_TOKEN")
                .unwrap_or_else(|_| "change-me-in-production".to_string()),
            listen_addr: std::env::var("LISTEN_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:8080".to_string()),
            cache_ttl_secs: std::env::var("CACHE_TTL_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3600),
            cache_max_capacity: std::env::var("CACHE_MAX_CAPACITY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(100_000),
        })
    }
}

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Deserialize)]
struct WebFingerQuery {
    resource: String,
}

#[derive(Debug, Serialize)]
struct WebFingerResponse {
    subject: String,
    links: Vec<WebFingerLink>,
}

#[derive(Debug, Serialize)]
struct WebFingerLink {
    rel: String,
    href: String,
}

#[derive(Debug, Deserialize)]
struct UpdateRequest {
    token: String,
    username: String,
    backend_url: String,
}

#[derive(Debug, Serialize)]
struct UpdateResponse {
    success: bool,
    message: String,
}

// ============================================================================
// Application State
// ============================================================================

#[derive(Clone)]
struct AppState {
    db: PgPool,
    cache: Cache<String, String>,
    managed_domain: String,
    admin_token: String,
}

// ============================================================================
// Database Operations
// ============================================================================

async fn init_database(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS user_mappings (
            username VARCHAR(255) PRIMARY KEY,
            backend_url VARCHAR(255) NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_backend_url
        ON user_mappings(backend_url)
        "#,
    )
    .execute(pool)
    .await?;

    info!("Database schema initialized");
    Ok(())
}

async fn get_backend_url(pool: &PgPool, username: &str) -> anyhow::Result<Option<String>> {
    let result = sqlx::query_scalar::<_, String>(
        "SELECT backend_url FROM user_mappings WHERE username = $1",
    )
    .bind(username)
    .fetch_optional(pool)
    .await?;

    Ok(result)
}

async fn upsert_mapping(pool: &PgPool, username: &str, backend_url: &str) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO user_mappings (username, backend_url, updated_at)
        VALUES ($1, $2, NOW())
        ON CONFLICT (username)
        DO UPDATE SET backend_url = $2, updated_at = NOW()
        "#,
    )
    .bind(username)
    .bind(backend_url)
    .execute(pool)
    .await?;

    Ok(())
}

// ============================================================================
// HTTP Handlers
// ============================================================================

async fn webfinger_handler(
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

async fn update_handler(
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

async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "archypix-resolver"
    }))
}

// ============================================================================
// Helper Functions
// ============================================================================

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

// ============================================================================
// Error Handling
// ============================================================================

enum AppError {
    NotFound,
    Unauthorized,
    BadRequest(String),
    Internal(anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "User not found".to_string()),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            AppError::Internal(err) => {
                warn!("Internal error: {:?}", err);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
        };

        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        AppError::Internal(err.into())
    }
}

// ============================================================================
// Main Application
// ============================================================================

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,archypix_resolver=debug".into()),
        )
        .init();

    // Load configuration
    let config = Config::from_env()?;
    info!("Starting Archypix Resolver");
    info!("Listen address: {}", config.listen_addr);
    info!("Cache TTL: {}s", config.cache_ttl_secs);
    info!("Cache max capacity: {}", config.cache_max_capacity);

    // Initialize database connection pool
    let db_pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await?;

    info!("Connected to database");

    // Initialize database schema
    init_database(&db_pool).await?;

    // Initialize moka cache
    let cache = Cache::builder()
        .time_to_live(Duration::from_secs(config.cache_ttl_secs))
        .max_capacity(config.cache_max_capacity)
        .build();

    info!("Initialized in-memory cache");

    // Create application state
    let state = AppState {
        db: db_pool,
        managed_domain: config.managed_domain,
        cache,
        admin_token: config.admin_token,
    };

    // Build router
    let app = Router::new()
        .route("/.well-known/webfinger", get(webfinger_handler))
        .route("/api/update", post(update_handler))
        .route("/health", get(health_handler))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // Start server
    let listener = tokio::net::TcpListener::bind(&config.listen_addr).await?;
    info!("Server listening on {}", config.listen_addr);

    axum::serve(listener, app).await?;

    Ok(())
}
