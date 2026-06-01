use thiserror::Error;

#[derive(Error, Debug)]
pub enum WorkerError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("JWT error: {0}")]
    Jwt(String),
    #[error("Image processing error: {0}")]
    Imaging(String),
    #[error("EXIF error: {0}")]
    Exif(String),
    #[error("Backend error: status={status}, body={body}")]
    BackendError { status: u16, body: String },
    #[error("No presigned URL for '{key}'")]
    MissingPresignedUrl { key: String },
}

pub type Result<T> = std::result::Result<T, WorkerError>;
