mod auth;
mod jobs;
mod pictures;
mod settings;
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
        .route("/pictures/{id}", get(pictures::details))
        .route("/pictures/{id}/url", get(pictures::picture_url))
        .route("/settings", get(settings::get_settings))
        .route("/settings", patch(settings::update_settings))
        .route("/tags", get(tags::list).patch(tags::edit))
        .route(
            "/shares/outgoing",
            post(shares::create_outgoing).get(shares::list_outgoing),
        )
        .route(
            "/shares/outgoing/{id}/revoke",
            post(shares::revoke_outgoing),
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
        .route("/jobs/{id}", get(jobs::get_job))
        .route("/pictures/{id}/jobs", get(jobs::list_picture_jobs))
        .route("/pictures/{id}/edit", post(jobs::enqueue_edit))
}
