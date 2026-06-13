//! Federation integration suite.
//!
//! Sub-modules:
//!   `contract`  — end-to-end protocol flows with two real Axum servers.
//!   `rejection` — security boundaries and error paths in federation API handlers.
//!   `presign`   — the share-presign endpoint (authorised by share_token, not a JWT).

#[path = "../common/mod.rs"]
mod common;
mod contract;
mod presign;
mod rejection;

// ── Shared infrastructure ─────────────────────────────────────────────────────

use axum::body::Body;
use axum::http::{Request, header};
use serde_json::Value;

/// Single-server config for "backend A" — oneshot tests only.
/// `back_domain` is a static fake hostname; no real port needed.
pub(crate) fn cfg_a() -> archypix_back::infra::config::Config {
    archypix_back::infra::config::Config {
        global_domain: "a.test".to_string(),
        back_domain: "backend-a.test".to_string(),
        ..archypix_back::infra::config::Config::test_defaults()
    }
}

/// Single-server config for "backend B" — oneshot tests only.
pub(crate) fn cfg_b() -> archypix_back::infra::config::Config {
    archypix_back::infra::config::Config {
        global_domain: "b.test".to_string(),
        back_domain: "backend-b.test".to_string(),
        ..archypix_back::infra::config::Config::test_defaults()
    }
}

/// Build a POST request with a federation bearer token.
pub(crate) fn post_fed(path: &str, bearer: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

/// Build a POST request with no authentication header.
pub(crate) fn post_no_auth(path: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

/// Consume a response body and parse it as JSON.
pub(crate) async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}
