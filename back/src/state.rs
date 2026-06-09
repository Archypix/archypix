use crate::clients::federation::FederationClient;
use crate::clients::resolver::ResolverClient;
use crate::infra::config::Config;
use crate::infra::crypto::JwtService;
use crate::infra::redis::Cache;
use crate::infra::s3::Storage;
use crate::infra::tasks::TaskQueue;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::Notify;

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
    /// In-process background task queue (tag rename).
    pub task_queue: TaskQueue,
    /// Wake signal for the tagging pipeline loop. Call `notify_one()` after any event
    /// that creates dirty pictures (ingest, tag edit, service config change, share accept).
    pub pipeline_notify: Arc<Notify>,
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
        pipeline_notify: Arc<Notify>,
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
            pipeline_notify,
        }
    }
}
