mod handlers;
mod models;

use crate::state::AppState;
use axum::Router;
use axum::routing::{get, post};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/jobs/next", get(handlers::claim_next_job))
        .route("/jobs/{id}/complete", post(handlers::complete_job))
        .route("/jobs/{id}/fail", post(handlers::fail_job))
}
