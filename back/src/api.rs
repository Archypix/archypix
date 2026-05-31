mod admin;
mod federation;
mod middleware;
mod resolver;
mod user;
mod webfinger;

use crate::infra::config::Config;
use crate::state::AppState;
use axum::Router;
use axum::http::HeaderValue;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::routing::get;
use tower_http::cors::{Any, CorsLayer};

pub fn routes(config: &Config) -> Router<AppState> {
    let mut router = Router::new()
        .nest("/api", api_routes(config))
        .route("/health", get(|| async { "OK" }));

    if !config.use_resolver {
        router = router.route("/.well-known/webfinger", get(webfinger::handler));
    }

    router
}

fn api_routes(config: &Config) -> Router<AppState> {
    let allow_origin = build_cors_origin(&config.cors_origins);
    let cors = CorsLayer::new()
        .allow_methods(Any)
        .allow_origin(allow_origin)
        .allow_headers([AUTHORIZATION, CONTENT_TYPE]);

    Router::new()
        .nest("/resolver", resolver::routes())
        .nest("/admin", admin::routes())
        .nest("/auth", user::auth_routes())
        .nest("/public", user::public_routes())
        .nest("/authenticated", user::authenticated_routes())
        .nest("/federation", federation::routes())
        .layer(cors)
}

fn build_cors_origin(origins: &[String]) -> tower_http::cors::AllowOrigin {
    if origins.iter().any(|o| o == "*") {
        tower_http::cors::AllowOrigin::any()
    } else {
        let list: Vec<HeaderValue> = origins
            .iter()
            .filter_map(|o| o.parse::<HeaderValue>().ok())
            .collect();
        tower_http::cors::AllowOrigin::list(list)
    }
}
