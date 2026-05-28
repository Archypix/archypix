mod handlers;
mod models;

use crate::infrastructure::state::AppState;
use axum::Router;
use axum::routing::{get, patch};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/users",
            get(handlers::list_users).post(handlers::create_user),
        )
        .route(
            "/users/{id}",
            patch(handlers::update_user).delete(handlers::delete_user),
        )
}
