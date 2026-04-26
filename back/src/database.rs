pub mod models;
pub mod user;

use crate::infrastructure::config::Config;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tracing::info;

pub async fn get_database_pool(config: &Config) -> anyhow::Result<PgPool> {
    info!("Database URL: {}", config.database_url);

    // Initialize database connection pool
    let db_pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await?;

    info!("Connected to database");
    Ok(db_pool)
}

pub async fn close_database_pool(pool: PgPool) {
    pool.close().await;
}

pub async fn run_migrations(pool: &PgPool) -> anyhow::Result<()> {
    info!("Migrating database...");

    sqlx::migrate!("./migrations").run(pool).await?;
    info!("Database migrated successfully");
    Ok(())
}
