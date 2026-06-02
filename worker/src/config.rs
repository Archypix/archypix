use anyhow::Context;
use archypix_common::job::JobType;

#[derive(Debug, Clone)]
pub struct Config {
    // Backend connectivity
    pub back_url: String,
    pub back_domain: String,
    pub global_domain: String,
    pub worker_jwt_secret: String,

    // Worker identity
    pub worker_id: String,

    // Job polling
    pub poll_interval_ms: u64,
    pub max_concurrent_jobs: usize,
    /// Job types this worker handles. Empty = accept all types.
    pub job_types: Vec<JobType>,

    // HTTP server (health check)
    pub listen_addr: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();

        let back_url = require_env("BACK_URL")?;
        let back_domain = require_env("BACK_DOMAIN")?;
        let global_domain = require_env("GLOBAL_DOMAIN")?;
        let worker_jwt_secret = require_env("WORKER_JWT_SECRET")?;

        let worker_id = std::env::var("WORKER_ID").unwrap_or_else(|_| {
            format!(
                "worker-{}",
                uuid::Uuid::new_v4()
                    .to_string()
                    .split('-')
                    .next()
                    .unwrap_or("0")
            )
        });

        let poll_interval_ms = env_u64("POLL_INTERVAL_MS", 1000)?;
        let max_concurrent_jobs = env_usize("MAX_CONCURRENT_JOBS", 2)?;

        let job_types = std::env::var("JOB_TYPES")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .filter_map(|s| match s.parse::<JobType>() {
                Ok(t) => Some(t),
                Err(e) => {
                    tracing::warn!("ignoring unknown job type in JOB_TYPES: {e}");
                    None
                }
            })
            .collect();

        let listen_addr = std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:80".to_string());

        Ok(Config {
            back_url,
            back_domain,
            global_domain,
            worker_jwt_secret,
            worker_id,
            poll_interval_ms,
            max_concurrent_jobs,
            job_types,
            listen_addr,
        })
    }
}

fn require_env(name: &str) -> anyhow::Result<String> {
    let val = std::env::var(name).with_context(|| format!("{name} must be specified"))?;
    if val.trim().is_empty() {
        anyhow::bail!("{name} cannot be empty");
    }
    Ok(val)
}

fn env_u64(name: &str, default: u64) -> anyhow::Result<u64> {
    let val = std::env::var(name).unwrap_or_else(|_| default.to_string());
    val.trim()
        .parse()
        .with_context(|| format!("{name} must be a non-negative integer"))
}

fn env_usize(name: &str, default: usize) -> anyhow::Result<usize> {
    let val = std::env::var(name).unwrap_or_else(|_| default.to_string());
    val.trim()
        .parse()
        .with_context(|| format!("{name} must be a positive integer"))
}
