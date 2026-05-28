use crate::infrastructure::config::Config;
use redis::aio::ConnectionManager;
use tracing::info;

pub async fn get_redis_manager(config: &Config) -> anyhow::Result<ConnectionManager> {
    let client = redis::Client::open(config.redis_url.clone())?;
    let manager = client.get_connection_manager().await?;
    info!("Connected to Redis");
    Ok(manager)
}
