mod auth;
mod backend;
mod config;
mod error;
mod imaging;
mod jobs;

use backend::BackendClient;
use config::Config;
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,archypix_worker=debug".into()),
        )
        .init();

    info!("Starting Archypix Worker...");

    let config = Config::from_env()?;

    info!("Worker ID:         {}", config.worker_id);
    info!("Backend URL:       {}", config.back_url);
    info!("Poll interval:     {}ms", config.poll_interval_ms);
    info!("Max concurrent:    {}", config.max_concurrent_jobs);
    info!("Job types:         {:?}", config.job_types);

    let config = Arc::new(config);
    let client = Arc::new(BackendClient::new((*config).clone()));

    // Health check HTTP server (minimal, just for orchestration probes).
    let health_addr = config.listen_addr.clone();
    tokio::spawn(async move {
        run_health_server(&health_addr).await;
    });

    // Main job loop (runs indefinitely).
    jobs::run_job_loop(config, client).await;

    Ok(())
}

async fn run_health_server(addr: &str) {
    use axum::{Json, Router, routing::get};

    let app = Router::new().route(
        "/health",
        get(|| async {
            Json(serde_json::json!({
                "status": "healthy",
                "service": "archypix-worker"
            }))
        }),
    );

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!(addr, error = ?e, "health server failed to bind");
            return;
        }
    };
    info!("Health server listening on {}", addr);
    if let Err(e) = axum::serve(listener, app).await {
        tracing::warn!(error = ?e, "health server error");
    }
}
