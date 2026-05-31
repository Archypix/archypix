use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct WebFingerQuery {
    pub resource: String,
}

#[derive(Serialize)]
struct WebFingerResponse {
    subject: String,
    links: Vec<WebFingerLink>,
}

#[derive(Serialize)]
struct WebFingerLink {
    rel: String,
    href: String,
}

/// Minimal WebFinger endpoint, active when `USE_RESOLVER=false`.
/// Returns this backend's own public base URL for any resource query.
pub async fn handler(
    State(state): State<AppState>,
    Query(query): Query<WebFingerQuery>,
) -> Response {
    let public_base_url = state.config.public_base_url();
    tracing::info!(
        resource = %query.resource,
        backend_url = %public_base_url,
        "WebFinger query"
    );
    let body = WebFingerResponse {
        subject: query.resource.clone(),
        links: vec![WebFingerLink {
            rel: "backend_url".to_string(),
            href: public_base_url,
        }],
    };
    let json = serde_json::to_string(&body).expect("WebFingerResponse is always serializable");
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/jrd+json"),
        )],
        json,
    )
        .into_response()
}
