mod api;
mod clients;
mod domain;
mod infra;
mod repository;
mod services;
mod state;

use crate::clients::federation::FederationClient;
use crate::clients::resolver::ResolverClient;
use crate::infra::config::Config;
use crate::infra::crypto::JwtService;
use crate::infra::tasks;
use crate::state::AppState;
use reqwest::Client as HttpClient;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,archypix_back=debug".into()),
        )
        .init();

    info!("Starting Archypix Backend...");

    let config = Config::from_env()?;

    info!("Back domain:   {}", config.back_domain);
    info!("Global domain: {}", config.global_domain);
    info!("Database:      {}", config.database_url_masked());
    info!("Redis:         {}", config.redis_url_masked());

    let db = infra::db::connect(&config).await?;
    infra::db::run_migrations(&db).await?;

    let redis = infra::redis::connect(&config).await?;
    let storage = infra::s3::connect(&config).await?;
    let http = HttpClient::new();

    let jwt = JwtService::new(&config.jwt_secret, &config.back_domain);
    let resolver_jwt = JwtService::new(&config.resolver_jwt_secret, &config.back_domain);
    let worker_jwt = JwtService::new(&config.worker_jwt_secret, &config.back_domain);

    let federation =
        FederationClient::new(http.clone(), config.clone(), jwt.clone(), redis.clone());
    let resolver = ResolverClient::new(http, config.clone(), resolver_jwt);

    // Register with the resolver so it can route user registrations to this backend.
    resolver.self_register().await?;

    // Start the in-process background task queue.
    let (task_queue, task_runner) = tasks::create(db.clone(), config.task_queue_concurrency);
    tokio::spawn(task_runner);

    let state = AppState::new(
        config.clone(),
        db,
        redis,
        jwt,
        worker_jwt,
        storage,
        federation,
        resolver,
        task_queue,
    );

    let listener = tokio::net::TcpListener::bind(&config.listen_addr).await?;
    info!("Listening on {}", config.listen_addr);

    let app = api::routes(&config)
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state);

    axum::serve(listener, app).await?;
    Ok(())
}
