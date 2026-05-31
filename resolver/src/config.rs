#[derive(Clone)]
pub struct Config {
    pub listen_addr: String,

    // ── Database (split) ──────────────────────────────────────────────────────
    pub db_host: String,
    pub db_port: u16,
    pub db_user: String,
    pub db_password: Option<String>,
    pub db_name: String,

    // ── Identity ──────────────────────────────────────────────────────────────
    pub global_domain: String,

    // ── Auth ──────────────────────────────────────────────────────────────────
    pub resolver_jwt_secret: String,

    // ── CORS ──────────────────────────────────────────────────────────────────
    /// Comma-separated list of allowed origins. Use `*` to allow any origin (dev only).
    pub cors_origins: Vec<String>,

    // ── Cache ─────────────────────────────────────────────────────────────────
    pub cache_ttl_secs: u64,
    pub cache_max_capacity: u64,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();

        let config = Config {
            listen_addr: env("LISTEN_ADDR", "0.0.0.0:8080".to_string()),

            db_host: require_env("DB_HOST")?,
            db_port: env_u16("DB_PORT", 5432)?,
            db_user: env("DB_USER", "postgres".to_string()),
            db_password: optional_env("DB_PASSWORD"),
            db_name: env("DB_NAME", "archypix_resolver".to_string()),

            global_domain: require_env("GLOBAL_DOMAIN")?,
            resolver_jwt_secret: require_env("RESOLVER_JWT_SECRET")?,

            cors_origins: require_env("CORS_ORIGINS")?
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),

            cache_ttl_secs: env_u64("CACHE_TTL_SECS", 3600)?,
            cache_max_capacity: env_u64("CACHE_MAX_CAPACITY", 100_000)?,
        };

        Ok(config)
    }

    pub fn database_url(&self) -> String {
        build_postgres_url(
            &self.db_host,
            self.db_port,
            &self.db_user,
            self.db_password.as_deref(),
            &self.db_name,
        )
    }

    pub fn database_url_masked(&self) -> String {
        build_postgres_url(
            &self.db_host,
            self.db_port,
            &self.db_user,
            self.db_password.as_ref().map(|_| "***"),
            &self.db_name,
        )
    }
}

fn build_postgres_url(
    host: &str,
    port: u16,
    user: &str,
    password: Option<&str>,
    db: &str,
) -> String {
    match password {
        Some(pw) => format!("postgres://{}:{}@{}:{}/{}", user, pw, host, port, db),
        None => format!("postgres://{}@{}:{}/{}", user, host, port, db),
    }
}

fn env(name: &str, default: String) -> String {
    std::env::var(name).unwrap_or(default)
}

fn optional_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn require_env(name: &str) -> anyhow::Result<String> {
    let val = std::env::var(name)
        .map_err(|_| anyhow::anyhow!("{} environment variable must be specified.", name))?;
    if val.trim().is_empty() {
        return Err(anyhow::anyhow!(
            "{} environment variable cannot be empty.",
            name
        ));
    }
    Ok(val)
}

fn env_u16(name: &str, default: u16) -> anyhow::Result<u16> {
    let val = std::env::var(name).unwrap_or_else(|_| default.to_string());
    val.trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("{} must be a valid port number (0-65535).", name))
}

fn env_u64(name: &str, default: u64) -> anyhow::Result<u64> {
    let val = std::env::var(name).unwrap_or_else(|_| default.to_string());
    val.trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("{} must be a non-negative integer.", name))
}
