use crate::state::AppState;
use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct WebFingerQuery {
    pub resource: String,
}

#[derive(Serialize)]
pub struct WebFingerResponse {
    subject: String,
    links: Vec<WebFingerLink>,
}

#[derive(Serialize)]
pub struct WebFingerLink {
    rel: String,
    href: String,
}

/// Minimal WebFinger endpoint, active when `USE_RESOLVER=false`.
/// Returns this backend's own public base URL for any resource query.
pub async fn handler(
    State(state): State<AppState>,
    Query(query): Query<WebFingerQuery>,
) -> Json<WebFingerResponse> {
    let public_base_url = state.config.public_base_url();
    tracing::info!(
        resource = %query.resource,
        backend_url = %public_base_url,
        "WebFinger query"
    );
    Json(WebFingerResponse {
        subject: query.resource.clone(),
        links: vec![WebFingerLink {
            rel: "backend_url".to_string(),
            href: public_base_url,
        }],
    })
}
