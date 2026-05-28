mod admin;
mod federation;
mod middleware;
mod resolver;
mod user;

use crate::infrastructure::config::Config;
use crate::infrastructure::state::AppState;
use axum::Router;
use axum::http::HeaderValue;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::routing::get;
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
        .nest("/resolver", resolver::routes())
        .nest("/admin", admin::routes())
        .nest("/auth", user::auth_routes())
        .nest("/public", user::public_routes())
        .nest("/authenticated", user::authenticated_routes())
        .nest("/federation", federation::routes())
        .layer(cors_layer)
}
