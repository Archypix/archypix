use crate::clients::federation::FederationClient;
use crate::clients::resolver::ResolverClient;
use crate::infra::config::Config;
use crate::infra::crypto::JwtService;
use crate::infra::redis::RedisClient;
use crate::infra::s3::StorageClient;
use crate::infra::tasks::TaskQueue;
use sqlx::PgPool;

/// Application state injected into every Axum handler via `State<AppState>`.
#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub db: PgPool,
    pub redis: RedisClient,
    pub jwt: JwtService,
    /// JWT service using the worker shared secret — verifies inbound worker tokens.
    pub worker_jwt: JwtService,
    pub storage: StorageClient,
    pub federation: FederationClient,
    pub resolver: ResolverClient,
    /// In-process background task queue (tag rename, tagging pipeline).
    pub task_queue: TaskQueue,
}

impl AppState {
    pub fn new(
        config: Config,
        db: PgPool,
        redis: RedisClient,
        jwt: JwtService,
        worker_jwt: JwtService,
        storage: StorageClient,
        federation: FederationClient,
        resolver: ResolverClient,
        task_queue: TaskQueue,
    ) -> Self {
        Self {
            config,
            db,
            redis,
            jwt,
            worker_jwt,
            storage,
            federation,
            resolver,
            task_queue,
        }
    }
}
