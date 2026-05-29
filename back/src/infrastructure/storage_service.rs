use crate::infrastructure::error::AppError;
use aws_sdk_s3::Client;
use aws_sdk_s3::presigning::PresigningConfig;
use std::time::Duration;

#[derive(Clone)]
pub struct StorageService {
    client: Client,
    presign_ttl: Duration,
}

impl StorageService {
    pub fn new(client: Client, presign_ttl: Duration) -> Self {
        Self {
            client,
            presign_ttl,
        }
    }

    pub async fn presign_get(&self, bucket: &str, key: &str) -> Result<String, AppError> {
        let config = PresigningConfig::expires_in(self.presign_ttl)
            .map_err(|e| AppError::InternalServerError(format!("presign config error: {}", e)))?;
        let presigned = self
            .client
            .get_object()
            .bucket(bucket)
            .key(key)
            .presigned(config)
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        Ok(presigned.uri().to_string())
    }

    pub async fn presign_put(&self, bucket: &str, key: &str) -> Result<String, AppError> {
        let config = PresigningConfig::expires_in(self.presign_ttl)
            .map_err(|e| AppError::InternalServerError(format!("presign config error: {}", e)))?;
        let presigned = self
            .client
            .put_object()
            .bucket(bucket)
            .key(key)
            .presigned(config)
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        Ok(presigned.uri().to_string())
    }
}
