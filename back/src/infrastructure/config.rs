#[derive(Debug, Clone)]
pub struct Config {
    pub listen_addr: String,
    pub database_url: String,
    pub front_url: String,
    pub use_resolver: bool,
    pub host: String,
    pub webfinger_host: String,
    pub resolver_url: String,
    pub resolver_admin_token: String,
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
            webfinger_host: require_env("WEBFINGER_HOST")?,
            resolver_url: env("RESOLVER_URL", String::default()),
            resolver_admin_token: env("RESOLVER_ADMIN_TOKEN", String::default()),
        };

        if config.use_resolver {
            if config.resolver_url.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "WEBFINGER_HOST must be specified when using a resolver."
                ));
            }
            if config.resolver_admin_token.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "RESOLVER_ADMIN_TOKEN must be specified when using a resolver."
                ));
            }
        }

        Ok(config)
    }
}

fn env(name: &str, default: String) -> String {
    std::env::var(name).unwrap_or_else(|_| default)
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
    require_env(name).map(|s| s.trim().parse::<bool>().unwrap())
}
