use crate::clients::federation::FederationClient;
use crate::clients::resolver::ResolverClient;
use crate::infra::config::Config;
use crate::infra::crypto::JwtService;
use crate::infra::redis::RedisClient;
use crate::infra::s3::StorageClient;
use sqlx::PgPool;

/// Application state injected into every Axum handler via `State<AppState>`.
#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub db: PgPool,
    pub redis: RedisClient,
    pub jwt: JwtService,
    pub storage: StorageClient,
    pub federation: FederationClient,
    pub resolver: ResolverClient,
}

impl AppState {
    pub fn new(
        config: Config,
        db: PgPool,
        redis: RedisClient,
        jwt: JwtService,
        storage: StorageClient,
        federation: FederationClient,
        resolver: ResolverClient,
    ) -> Self {
        Self {
            config,
            db,
            redis,
            jwt,
            storage,
            federation,
            resolver,
        }
    }
}
