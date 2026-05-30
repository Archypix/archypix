use crate::infra::config::Config;
use crate::infra::error::AppError;
use aws_config::Region;
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::Credentials;
use aws_sdk_s3::presigning::PresigningConfig;
use std::time::Duration;
use tracing::info;

/// Thin wrapper around the S3 client that adds presigned URL helpers.
#[derive(Clone)]
pub struct StorageClient {
    client: Client,
    presign_ttl: Duration,
}

impl StorageClient {
    pub fn new(client: Client, presign_ttl: Duration) -> Self {
        Self {
            client,
            presign_ttl,
        }
    }

    pub async fn presign_get(&self, bucket: &str, key: &str) -> Result<String, AppError> {
        let config = PresigningConfig::expires_in(self.presign_ttl)
            .map_err(|e| AppError::InternalServerError(format!("presign config: {e}")))?;
        self.client
            .get_object()
            .bucket(bucket)
            .key(key)
            .presigned(config)
            .await
            .map(|p| p.uri().to_string())
            .map_err(|e| AppError::InternalServerError(e.to_string()))
    }

    pub async fn presign_put(&self, bucket: &str, key: &str) -> Result<String, AppError> {
        let config = PresigningConfig::expires_in(self.presign_ttl)
            .map_err(|e| AppError::InternalServerError(format!("presign config: {e}")))?;
        self.client
            .put_object()
            .bucket(bucket)
            .key(key)
            .presigned(config)
            .await
            .map(|p| p.uri().to_string())
            .map_err(|e| AppError::InternalServerError(e.to_string()))
    }
}

pub async fn connect(config: &Config) -> anyhow::Result<StorageClient> {
    let region = Region::new(config.s3_region.clone());
    let region_provider = RegionProviderChain::first_try(region);
    let credentials = Credentials::new(
        config.s3_access_key.clone(),
        config.s3_secret_key.clone(),
        None,
        None,
        "static",
    );
    let shared_config = aws_config::from_env()
        .region(region_provider)
        .credentials_provider(credentials)
        .endpoint_url(config.s3_endpoint.clone())
        .load()
        .await;
    let s3_config = aws_sdk_s3::config::Builder::from(&shared_config)
        .force_path_style(true)
        .build();
    let client = Client::from_conf(s3_config);
    info!("Connected to MinIO/S3");
    Ok(StorageClient::new(
        client,
        Duration::from_secs(config.s3_presign_ttl_secs),
    ))
}
