mod api;
mod database;
mod infrastructure;

use crate::infrastructure::config::Config;
use crate::infrastructure::state::AppState;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,archypix_backend=debug".into()),
        )
        .init();

    info!("Starting Archypix Backend...");

    // Load configuration, setup DB and state
    let config = Config::from_env()?;
    let db_pool = database::get_database_pool(&config).await?;
    database::run_migrations(&db_pool).await?;

    let state = AppState::new(config.clone(), db_pool);

    // Listen to listen_addr and build router with API routes
    let listener = tokio::net::TcpListener::bind(&config.listen_addr).await?;
    info!("Server listening on {}", config.listen_addr);

    let app = api::routes(&config)
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state);

    axum::serve(listener, app).await?;

    Ok(())
}
