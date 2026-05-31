use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use thiserror::Error;
use tracing::{error, warn};

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Not found")]
    NotFound,
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    #[error("Bad request: {0}")]
    BadRequest(String),
    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),
    /// Propagated HTTP error from a backend (status code + body).
    #[error("Backend error {0}: {1}")]
    BackendError(u16, String),
    #[error("Internal server error: {0}")]
    InternalServerError(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::NotFound => {
                warn!("client error: not found");
                (StatusCode::NOT_FOUND, "Not found".to_string())
            }
            AppError::Unauthorized(ref msg) => {
                warn!(error = msg.as_str(), "client error: unauthorized");
                (StatusCode::UNAUTHORIZED, self.to_string())
            }
            AppError::BadRequest(ref msg) => {
                warn!(error = msg.as_str(), "client error: bad request");
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            AppError::ServiceUnavailable(ref msg) => {
                warn!(error = msg.as_str(), "client error: service unavailable");
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
            AppError::BackendError(status_code, ref msg) => {
                let status =
                    StatusCode::from_u16(status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                warn!(
                    status = status_code,
                    error = msg.as_str(),
                    "backend returned error"
                );
                return (status, Json(serde_json::json!({ "error": msg }))).into_response();
            }
            AppError::InternalServerError(ref msg) => {
                error!(error = msg.as_str(), "internal server error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
        };

        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

/// Convert any `anyhow::Error` (returned by database helpers) into an internal error.
impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        AppError::InternalServerError(err.to_string())
    }
}
