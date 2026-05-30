use crate::infra::config::Config;
use crate::infra::error::AppError;
use bb8_redis::{
    RedisConnectionManager, bb8,
    redis::{AsyncCommands, cmd},
};
use serde::{Serialize, de::DeserializeOwned};
use tracing::info;

#[derive(Clone)]
pub struct RedisClient {
    pool: bb8::Pool<RedisConnectionManager>,
}

impl RedisClient {
    pub async fn get_json<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, AppError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        let raw: Option<String> = conn
            .get(key)
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        raw.map(|s| {
            serde_json::from_str(&s).map_err(|e| AppError::InternalServerError(e.to_string()))
        })
        .transpose()
    }

    pub async fn set_json_ex<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl_secs: u64,
    ) -> Result<(), AppError> {
        let json = serde_json::to_string(value)
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        let _: () = conn
            .set_ex(key, json, ttl_secs)
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        Ok(())
    }

    pub async fn get_string(&self, key: &str) -> Result<Option<String>, AppError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        conn.get(key)
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))
    }

    pub async fn set_string_ex(
        &self,
        key: &str,
        value: &str,
        ttl_secs: u64,
    ) -> Result<(), AppError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        let _: () = conn
            .set_ex(key, value, ttl_secs)
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        Ok(())
    }

    pub async fn del(&self, key: &str) -> Result<(), AppError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        let _: () = conn
            .del(key)
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        Ok(())
    }
}

pub async fn connect(config: &Config) -> anyhow::Result<RedisClient> {
    info!("Connecting to Redis: {}", config.redis_url);
    let manager = RedisConnectionManager::new(config.redis_url.clone())?;
    let pool = bb8::Pool::builder()
        .connection_timeout(std::time::Duration::from_secs(5))
        .build(manager)
        .await?;
    {
        let mut conn = pool.get().await?;
        let reply: String = cmd("PING").query_async(&mut *conn).await?;
        assert_eq!("PONG", reply);
    }
    info!("Connected to Redis");
    Ok(RedisClient { pool })
}
