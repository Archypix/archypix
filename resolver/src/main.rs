mod config;
mod database;
mod error;
mod handler;

use crate::database::init_database;
use crate::handler::{
    health_handler, list_backends_handler, register_backend_handler, register_handler,
    update_handler, webfinger_handler,
};
use axum::{
    Router,
    http::HeaderValue,
    routing::{get, post},
};
use config::Config;
use moka::future::Cache;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

#[derive(Clone)]
struct AppState {
    db: PgPool,
    cache: Cache<String, String>,
    managed_domain: String,
    resolver_admin_secret: String,
    reqwest_client: reqwest::Client,
}

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

    // Initialize shared HTTP client
    let reqwest_client = reqwest::Client::new();

    // Create application state
    let state = AppState {
        db: db_pool,
        managed_domain: config.managed_domain,
        cache,
        resolver_admin_secret: config.resolver_admin_secret,
        reqwest_client,
    };

    let allow_origin = if config.front_url == "*" {
        tower_http::cors::AllowOrigin::any()
    } else {
        tower_http::cors::AllowOrigin::exact(
            config
                .front_url
                .parse::<HeaderValue>()
                .expect("FRONT_URL is not a valid origin"),
        )
    };
    let cors = CorsLayer::new()
        .allow_methods(Any)
        .allow_origin(allow_origin)
        .allow_headers(Any);

    // Build router
    let app = Router::new()
        .route("/.well-known/webfinger", get(webfinger_handler))
        .route("/api/update", post(update_handler))
        .route("/api/register", post(register_handler))
        .route(
            "/api/backends",
            post(register_backend_handler).get(list_backends_handler),
        )
        .route("/health", get(health_handler))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // Start server
    let listener = tokio::net::TcpListener::bind(&config.listen_addr).await?;
    info!("Server listening on {}", config.listen_addr);

    axum::serve(listener, app).await?;

    Ok(())
}
