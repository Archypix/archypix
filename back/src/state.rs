use crate::clients::federation::FederationClient;
use crate::clients::resolver::ResolverClient;
use crate::infra::config::Config;
use crate::infra::crypto::JwtService;
use crate::infra::redis::Cache;
use crate::infra::s3::Storage;
use crate::infra::tasks::TaskQueue;
use sqlx::PgPool;
use std::sync::Arc;

/// Application state injected into every Axum handler via `State<AppState>`.
#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub db: PgPool,
    /// Cache abstraction — `RedisClient` in production, `InMemoryCache` in tests.
    pub cache: Arc<dyn Cache>,
    pub jwt: JwtService,
    /// JWT service using the worker shared secret — verifies inbound worker tokens.
    pub worker_jwt: JwtService,
    /// Object storage abstraction — `StorageClient` in production, `MockStorage` in tests.
    pub storage: Arc<dyn Storage>,
    pub federation: FederationClient,
    pub resolver: ResolverClient,
    /// In-process background task queue (tag rename, tagging pipeline).
    pub task_queue: TaskQueue,
}

impl AppState {
    pub fn new(
        config: Config,
        db: PgPool,
        cache: Arc<dyn Cache>,
        jwt: JwtService,
        worker_jwt: JwtService,
        storage: Arc<dyn Storage>,
        federation: FederationClient,
        resolver: ResolverClient,
        task_queue: TaskQueue,
    ) -> Self {
        Self {
            config,
            db,
            cache,
            jwt,
            worker_jwt,
            storage,
            federation,
            resolver,
            task_queue,
        }
    }
}
