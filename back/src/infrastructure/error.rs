use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use sqlx::Error;
use sqlx::error::DatabaseError;
use std::borrow::Cow;
use thiserror::Error;
use tracing::{info, warn};

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Not found")]
    NotFound,
    #[error("Unauthorized")]
    Unauthorized(String),
    #[error("Bad request")]
    BadRequest(String),
    #[error("Internal server error")]
    InternalServerError(String),
    #[error("Database error")]
    SqlxError(sqlx::Error),
    #[error("Database error")]
    DatabaseError(String, String),
    #[error("Conflict")]
    DbConflict(String),
}

// Implement IntoResponse for AppError so we can return it directly from handlers
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            AppError::BadRequest(msg) => StatusCode::BAD_REQUEST,
            AppError::InternalServerError(msg) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::SqlxError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::DatabaseError(_, _) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::DbConflict(_) => StatusCode::CONFLICT,
        };
        let body = serde_json::json!({
            "message": self.to_string(), // This error obfuscative formatting
        });
        warn!("responding with error: {:?}", self);
        (status, axum::Json(body)).into_response()
    }
}

pub fn map_sqlx_error(err: sqlx::Error) -> AppError {
    if let Error::Database(_) = &err {
        let db_error = err.into_database_error().unwrap();
        if let Some(Cow::Borrowed("23505")) = db_error.code() {
            return AppError::DbConflict(db_error.message().to_string());
        }
        warn!(
            "Database error: {:?} ({})",
            db_error.code(),
            db_error.message()
        );
        AppError::DatabaseError(
            db_error.code().unwrap().to_string(),
            db_error.message().to_string(),
        )
    } else {
        info!("SQLX error: {:?}", err);
        AppError::SqlxError(err)
    }
}
