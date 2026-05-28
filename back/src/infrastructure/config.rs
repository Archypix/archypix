#[derive(Debug, Clone)]
pub struct Config {
    pub listen_addr: String,
    pub database_url: String,
    pub front_url: String,
    pub use_resolver: bool,
    pub host: String,
    pub public_base_url: String,
    pub webfinger_host: String,
    pub resolver_url: String,
    pub resolver_admin_secret: String,
    pub jwt_secret: String,
    pub access_token_ttl_secs: i64,
    pub refresh_token_ttl_secs: i64,
    pub federation_jwt_ttl_secs: i64,
    pub federation_backend_cache_ttl_secs: u64,
    pub federation_request_timeout_ms: u64,
    pub federation_scheme: String,
    pub redis_url: String,
    pub s3_endpoint: String,
    pub s3_access_key: String,
    pub s3_secret_key: String,
    pub s3_region: String,
    pub s3_bucket: String,
    pub s3_presign_ttl_secs: u64,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();

        let config = Config {
            listen_addr: env("LISTEN_ADDR", "0.0.0.0:80".to_string()),
            database_url: require_env("DATABASE_URL")?,
            front_url: require_env("FRONT_URL")?,
            use_resolver: require_bool_env("USE_RESOLVER")?,
            host: require_env("HOST")?,
            public_base_url: env("PUBLIC_BASE_URL", String::default()),
            webfinger_host: require_env("WEBFINGER_HOST")?,
            resolver_url: env("RESOLVER_URL", String::default()),
            resolver_admin_secret: env("RESOLVER_ADMIN_SECRET", String::default()),
            jwt_secret: require_env("JWT_SECRET")?,
            access_token_ttl_secs: env_i64("ACCESS_TOKEN_TTL_SECS", 900)?,
            refresh_token_ttl_secs: env_i64("REFRESH_TOKEN_TTL_SECS", 2_592_000)?,
            federation_jwt_ttl_secs: env_i64("FEDERATION_JWT_TTL_SECS", 900)?,
            federation_backend_cache_ttl_secs: env_u64("FEDERATION_BACKEND_CACHE_TTL_SECS", 3600)?,
            federation_request_timeout_ms: env_u64("FEDERATION_REQUEST_TIMEOUT_MS", 5000)?,
            federation_scheme: env("FEDERATION_SCHEME", "https".to_string()),
            redis_url: require_env("REDIS_URL")?,
            s3_endpoint: require_env("S3_ENDPOINT")?,
            s3_access_key: require_env("S3_ACCESS_KEY")?,
            s3_secret_key: require_env("S3_SECRET_KEY")?,
            s3_region: env("S3_REGION", "us-east-1".to_string()),
            s3_bucket: require_env("S3_BUCKET")?,
            s3_presign_ttl_secs: env_u64("S3_PRESIGN_TTL_SECS", 300)?,
        };

        if config.public_base_url.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "PUBLIC_BASE_URL must be specified (e.g. https://backend.example.com)."
            ));
        }

        if config.use_resolver {
            if config.resolver_url.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "WEBFINGER_HOST must be specified when using a resolver."
                ));
            }

            if config.resolver_admin_secret.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "RESOLVER_ADMIN_SECRET must be specified when using a resolver."
                ));
            }
        }

        Ok(config)
    }
}

fn env(name: &str, default: String) -> String {
    std::env::var(name).unwrap_or_else(|_| default)
}

fn env_i64(name: &str, default: i64) -> anyhow::Result<i64> {
    let val = std::env::var(name).unwrap_or_else(|_| default.to_string());
    if val.trim().is_empty() {
        return Err(anyhow::anyhow!(
            "{} environment variable cannot be empty.",
            name
        ));
    }
    val.trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("{} must be an integer.", name))
}

fn env_u64(name: &str, default: u64) -> anyhow::Result<u64> {
    let val = std::env::var(name).unwrap_or_else(|_| default.to_string());
    if val.trim().is_empty() {
        return Err(anyhow::anyhow!(
            "{} environment variable cannot be empty.",
            name
        ));
    }
    val.trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("{} must be an integer.", name))
}

fn require_env(name: &str) -> anyhow::Result<String> {
    let val = std::env::var(name)
        .map_err(|_| anyhow::anyhow!("{} environment variable must be specified.", name));
    if let Ok(val) = &val
        && val.trim().is_empty()
    {
        return Err(anyhow::anyhow!(
            "{} environment variable cannot be empty.",
            name
        ));
    }
    val
}
fn require_bool_env(name: &str) -> anyhow::Result<bool> {
    require_env(name)
        .map(|s| {
            Ok(match s.trim().to_lowercase().as_str() {
                "true" => true,
                "false" => false,
                "1" => true,
                "0" => false,
                _ => anyhow::bail!("Invalid boolean value: {}", s),
            })
        })
        .flatten()
}
