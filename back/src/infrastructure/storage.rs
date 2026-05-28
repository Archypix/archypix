use crate::infrastructure::config::Config;
use aws_config::Region;
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::Credentials;
use tracing::info;

pub async fn get_s3_client(config: &Config) -> anyhow::Result<Client> {
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
    Ok(client)
}
