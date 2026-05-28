use crate::infrastructure::config::Config;
use crate::services::auth::JwtService;
use aws_sdk_s3::Client as S3Client;
use redis::aio::ConnectionManager;
use reqwest::Client as HttpClient;
use sqlx::PgPool;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) config: Config,
    pub(crate) db: PgPool,
    pub(crate) redis: ConnectionManager,
    pub(crate) s3: S3Client,
    pub(crate) http: HttpClient,
    pub(crate) jwt: JwtService,
    pub(crate) resolver_jwt: JwtService,
}

impl AppState {
    pub(crate) fn new(
        config: Config,
        db: PgPool,
        redis: ConnectionManager,
        s3: S3Client,
        http: HttpClient,
        jwt: JwtService,
        resolver_jwt: JwtService,
    ) -> Self {
        Self {
            config,
            db,
            redis,
            s3,
            http,
            jwt,
            resolver_jwt,
        }
    }
}
