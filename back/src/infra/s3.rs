use crate::infra::config::Config;
use crate::infra::error::AppError;
use aws_config::Region;
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::Credentials;
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::types::{
    BucketLifecycleConfiguration, ExpirationStatus, LifecycleExpiration, LifecycleRule,
    LifecycleRuleFilter,
};
use base64::Engine as _;
use std::time::Duration;
use tracing::{info, warn};

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

    pub async fn copy_object(
        &self,
        src_bucket: &str,
        src_key: &str,
        dst_bucket: &str,
        dst_key: &str,
    ) -> Result<(), AppError> {
        self.client
            .copy_object()
            .copy_source(format!("{}/{}", src_bucket, src_key))
            .bucket(dst_bucket)
            .key(dst_key)
            .send()
            .await
            .map(|_| ())
            .map_err(|e| AppError::InternalServerError(e.to_string()))
    }

    pub async fn delete_object(&self, bucket: &str, key: &str) -> Result<(), AppError> {
        self.client
            .delete_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .map(|_| ())
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
    info!("Connecting to MinIO/S3: {}", config.s3_endpoint);
    let client = Client::from_conf(s3_config);
    client
        .list_buckets()
        .send()
        .await
        .map_err(|e| match e.as_service_error() {
            Some(svc) => anyhow::anyhow!(
                "Failed to connect to MinIO/S3 at {}: {} (code: {}, message: {})",
                config.s3_endpoint,
                svc,
                svc.meta().code().unwrap_or("unknown"),
                svc.meta().message().unwrap_or("no message"),
            ),
            None => anyhow::anyhow!(
                "Failed to connect to MinIO/S3 at {}: {}",
                config.s3_endpoint,
                e
            ),
        })?;
    info!("Connected to MinIO/S3");

    let buckets = [
        config.s3_bucket_staging.as_str(),
        config.s3_bucket_originals.as_str(),
        config.s3_bucket_small.as_str(),
        config.s3_bucket_medium.as_str(),
        config.s3_bucket_large.as_str(),
    ];
    ensure_buckets(&client, &buckets).await?;
    if let Err(e) = ensure_staging_lifecycle(&client, &config.s3_bucket_staging).await {
        warn!("{}", e);
        warn!(
            "Staging bucket '{}' will not auto-expire — orphaned objects must be cleaned manually.",
            config.s3_bucket_staging
        );
    }

    Ok(StorageClient::new(
        client,
        Duration::from_secs(config.s3_presign_ttl_secs),
    ))
}

async fn ensure_staging_lifecycle(client: &Client, bucket: &str) -> anyhow::Result<()> {
    let expiration = LifecycleExpiration::builder().days(1).build();
    let rule = LifecycleRule::builder()
        .id("expire-staging")
        .status(ExpirationStatus::Enabled)
        .filter(LifecycleRuleFilter::builder().prefix("").build())
        .expiration(expiration)
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to build lifecycle rule: {}", e))?;
    let lifecycle_config = BucketLifecycleConfiguration::builder()
        .rules(rule)
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to build lifecycle config: {}", e))?;
    if let Err(e) = client
        .put_bucket_lifecycle_configuration()
        .bucket(bucket)
        .lifecycle_configuration(lifecycle_config)
        .customize()
        .mutate_request(|req| {
            // MinIO requires a Content-MD5 header; the AWS SDK does not add it automatically.
            if let Some(body) = req.body().bytes() {
                let digest = md5::compute(body);
                let encoded = base64::engine::general_purpose::STANDARD.encode(digest.as_ref());
                req.headers_mut().insert(
                    http::header::HeaderName::from_static("content-md5"),
                    http::header::HeaderValue::from_str(&encoded).unwrap(),
                );
            }
        })
        .send()
        .await
    {
        let svc = e.into_service_error();
        return Err(anyhow::anyhow!(
            "Failed to set lifecycle rule on '{}': {} (code: {}, message: {})",
            bucket,
            svc,
            svc.meta().code().unwrap_or("unknown"),
            svc.meta().message().unwrap_or("no message"),
        ));
    }
    info!(
        "Lifecycle rule set on staging bucket: {} (1-day expiry)",
        bucket
    );
    Ok(())
}

async fn ensure_buckets(client: &Client, buckets: &[&str]) -> anyhow::Result<()> {
    for &bucket in buckets {
        match client.create_bucket().bucket(bucket).send().await {
            Ok(_) => info!("Created S3 bucket: {}", bucket),
            Err(e) => {
                let svc = e.into_service_error();
                if svc.is_bucket_already_owned_by_you() || svc.is_bucket_already_exists() {
                    // Bucket already exists — nothing to do.
                } else {
                    return Err(anyhow::anyhow!(
                        "Failed to create bucket '{}': {} (code: {}, message: {})",
                        bucket,
                        svc,
                        svc.meta().code().unwrap_or("unknown"),
                        svc.meta().message().unwrap_or("no message"),
                    ));
                }
            }
        }
    }
    Ok(())
}
