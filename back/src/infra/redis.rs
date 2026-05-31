use crate::infra::config::Config;
use crate::infra::error::AppError;
use bb8_redis::{
    RedisConnectionManager, bb8,
    redis::{AsyncCommands, cmd},
};
use serde::{Serialize, de::DeserializeOwned};
use std::fmt;
use tracing::info;
use uuid::Uuid;

/// Canonical Redis key definitions. Every key used anywhere in the codebase is listed here.
/// Pass a value of this enum directly to `RedisClient` methods — no manual key building needed.
pub enum RedisKey<'a> {
    /// Transient upload session during the presigned-PUT window.
    UploadSession(Uuid),
    /// Cached presigned GET URL for a picture — covers owned, same-backend, and cross-instance.
    /// Keyed by `(picture_id, variant)`. Picture UUIDs are unique per backend.
    PictureUrl(Uuid, &'a str),
    /// Cached federation JWT for communicating with `global_domain`.
    FederationToken(&'a str),
    /// Cached backend domain for `username@global_domain`.
    FederationBackend(&'a str, &'a str),
}

impl<'a> RedisKey<'a> {
    fn build(&self) -> String {
        match self {
            Self::UploadSession(id) => format!("upload:{id}"),
            Self::PictureUrl(id, variant) => format!("presign:{id}:{variant}"),
            Self::FederationToken(domain) => format!("federation:token:{domain}"),
            Self::FederationBackend(u, d) => format!("federation:backend:{u}@{d}"),
        }
    }
}

impl<'a> fmt::Display for RedisKey<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.build())
    }
}

#[derive(Clone)]
pub struct RedisClient {
    pool: bb8::Pool<RedisConnectionManager>,
}

impl RedisClient {
    pub async fn get_json<T: DeserializeOwned>(
        &self,
        key: RedisKey<'_>,
    ) -> Result<Option<T>, AppError> {
        let k = key.build();
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        let raw: Option<String> = conn
            .get(&k)
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        raw.map(|s| {
            serde_json::from_str(&s).map_err(|e| AppError::InternalServerError(e.to_string()))
        })
        .transpose()
    }

    pub async fn set_json_ex<T: Serialize>(
        &self,
        key: RedisKey<'_>,
        value: &T,
        ttl_secs: u64,
    ) -> Result<(), AppError> {
        let k = key.build();
        let json = serde_json::to_string(value)
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        let _: () = conn
            .set_ex(&k, json, ttl_secs)
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        Ok(())
    }

    pub async fn get_string(&self, key: RedisKey<'_>) -> Result<Option<String>, AppError> {
        let k = key.build();
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        conn.get(&k)
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))
    }

    pub async fn set_string_ex(
        &self,
        key: RedisKey<'_>,
        value: &str,
        ttl_secs: u64,
    ) -> Result<(), AppError> {
        let k = key.build();
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        let _: () = conn
            .set_ex(&k, value, ttl_secs)
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        Ok(())
    }

    pub async fn del(&self, key: RedisKey<'_>) -> Result<(), AppError> {
        let k = key.build();
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        let _: () = conn
            .del(&k)
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        Ok(())
    }
}

pub async fn connect(config: &Config) -> anyhow::Result<RedisClient> {
    info!("Connecting to Redis: {}", config.redis_url_masked());
    let manager = RedisConnectionManager::new(config.redis_url())?;
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
