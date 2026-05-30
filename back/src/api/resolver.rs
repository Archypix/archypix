mod handlers;
mod models;

use crate::state::AppState;
use axum::Router;
use axum::routing::{get, post};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/users", post(handlers::create_user))
        .route("/users/{username}", get(handlers::get_user))
}
