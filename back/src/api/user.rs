mod auth;
mod pictures;
mod shares;
mod tags;
mod users;

use crate::state::AppState;
use axum::Router;
use axum::routing::{get, patch, post};

pub fn auth_routes() -> Router<AppState> {
    Router::new()
        .route("/login", post(auth::login))
        .route("/refresh", post(auth::refresh))
        .route("/logout", post(auth::logout))
        .route("/me", get(auth::me))
}

pub fn public_routes() -> Router<AppState> {
    Router::new()
        .route("/users", post(users::register))
        .route("/users/{username}", get(users::get_public))
}

pub fn authenticated_routes() -> Router<AppState> {
    Router::new()
        .route("/users/me", patch(users::update_me))
        .route("/pictures/uploads", post(pictures::create_upload))
        .route(
            "/pictures/uploads/{id}/complete",
            post(pictures::complete_upload),
        )
        .route("/pictures", get(pictures::list))
        .route("/pictures/{id}", get(pictures::get))
        .route("/pictures/{id}/download", get(pictures::download))
        .route(
            "/tags",
            get(tags::list).post(tags::assign).delete(tags::remove),
        )
        .route(
            "/pictures/{id}/tags",
            post(tags::assign_to_picture).delete(tags::remove_from_picture),
        )
        .route(
            "/shares/outgoing",
            post(shares::create_outgoing).get(shares::list_outgoing),
        )
        .route("/shares/incoming", get(shares::list_incoming))
        .route(
            "/shares/incoming/{id}/accept",
            post(shares::accept_incoming),
        )
        .route(
            "/shares/incoming/{id}/reject",
            post(shares::reject_incoming),
        )
}
