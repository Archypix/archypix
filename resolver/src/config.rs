#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub managed_domain: String,
    pub admin_token: String,
    pub listen_addr: String,
    pub cache_ttl_secs: u64,
    pub cache_max_capacity: u64,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();

        Ok(Config {
            database_url: std::env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgres://archypix:archypix@localhost/archypix_resolver".to_string()
            }),
            managed_domain: std::env::var("MANAGED_DOMAIN")
                .unwrap_or_else(|_| "archypix.com".to_string()),
            admin_token: std::env::var("ADMIN_TOKEN")
                .unwrap_or_else(|_| "change-me-in-production".to_string()),
            listen_addr: std::env::var("LISTEN_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:8080".to_string()),
            cache_ttl_secs: std::env::var("CACHE_TTL_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3600),
            cache_max_capacity: std::env::var("CACHE_MAX_CAPACITY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(100_000),
        })
    }
}
