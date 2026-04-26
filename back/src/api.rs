mod user_handlers;

use crate::AppState;
use crate::api::user_handlers::{create_user, get_user};
use crate::infrastructure::config::Config;
use axum::Router;
use axum::http::HeaderValue;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::routing::{get, post};
use tower_http::cors::{Any, CorsLayer};

pub fn routes(config: &Config) -> Router<AppState> {
    Router::new()
        .nest("/api", api_routes(config))
        .route("/health", get(|| async { "Archypix Backend is healthy" }))
}

fn api_routes(config: &Config) -> Router<AppState> {
    let cors_layer = CorsLayer::new()
        .allow_methods(Any)
        .allow_origin(config.front_url.parse::<HeaderValue>().unwrap())
        .allow_headers([AUTHORIZATION, CONTENT_TYPE]);

    Router::new()
        .route("/users/{username}", get(get_user))
        .route("/users", post(create_user))
        .layer(cors_layer)
}
