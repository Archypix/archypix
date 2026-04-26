use crate::infrastructure::config::Config;
use sqlx::PgPool;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) config: Config,
    pub(crate) db: PgPool,
}

impl AppState {
    pub(crate) fn new(config: Config, db: PgPool) -> Self {
        Self { config, db }
    }
}
