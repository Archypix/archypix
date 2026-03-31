mod config;
mod database;
mod error;
mod handler;

use crate::database::{init_database};
use crate::handler::{health_handler, update_handler, webfinger_handler};
use axum::{
    routing::{get, post},
    Router,
};
use config::Config;
use moka::future::Cache;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::time::Duration;
use tower_http::trace::TraceLayer;
use tracing::info;

#[derive(Clone)]
struct AppState {
    db: PgPool,
    cache: Cache<String, String>,
    managed_domain: String,
    admin_token: String,
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
