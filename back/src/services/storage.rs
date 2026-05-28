use crate::infrastructure::error::AppError;
use aws_sdk_s3::Client;
use aws_sdk_s3::presigning::PresigningConfig;
use std::time::Duration;

#[derive(Clone)]
pub struct StorageService {
    client: Client,
    bucket: String,
    presign_ttl: Duration,
}

impl StorageService {
    pub fn new(client: Client, bucket: String, presign_ttl: Duration) -> Self {
        Self {
            client,
            bucket,
            presign_ttl,
        }
    }

    pub async fn presign_get(&self, key: &str) -> Result<String, AppError> {
        self.presign_get_in_bucket(&self.bucket, key).await
    }

    pub async fn presign_put(&self, key: &str) -> Result<String, AppError> {
        self.presign_put_in_bucket(&self.bucket, key).await
    }

    pub async fn presign_get_in_bucket(&self, bucket: &str, key: &str) -> Result<String, AppError> {
        let config = PresigningConfig::expires_in(self.presign_ttl).map_err(|err| {
            AppError::InternalServerError(format!("presign config error: {}", err))
        })?;
        let presigned = self
            .client
            .get_object()
            .bucket(bucket)
            .key(key)
            .presigned(config)
            .await
            .map_err(|err| AppError::InternalServerError(err.to_string()))?;
        Ok(presigned.uri().to_string())
    }

    pub async fn presign_put_in_bucket(&self, bucket: &str, key: &str) -> Result<String, AppError> {
        let config = PresigningConfig::expires_in(self.presign_ttl).map_err(|err| {
            AppError::InternalServerError(format!("presign config error: {}", err))
        })?;
        let presigned = self
            .client
            .put_object()
            .bucket(bucket)
            .key(key)
            .presigned(config)
            .await
            .map_err(|err| AppError::InternalServerError(err.to_string()))?;
        Ok(presigned.uri().to_string())
    }
}
