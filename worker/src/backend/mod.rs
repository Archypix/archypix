pub mod models;

use crate::auth::generate_token;
use crate::config::Config;
use crate::error::{Result, WorkerError};
use models::{ClaimedJob, CompleteJobRequest, FailJobRequest};
use reqwest::Client;
use tracing::debug;
use uuid::Uuid;

/// HTTP client for communicating with the Archypix backend.
#[derive(Clone)]
pub struct BackendClient {
    http: Client,
    config: Config,
}

impl BackendClient {
    pub fn new(config: Config) -> Self {
        Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("failed to build HTTP client"),
            config,
        }
    }

    /// Attempt to claim the next pending job. Returns `None` if no job is available.
    pub async fn claim_next_job(&self) -> Result<Option<ClaimedJob>> {
        let token = generate_token(&self.config)?;
        let url = format!(
            "{}/api/worker/jobs/next",
            self.config.back_url.trim_end_matches('/')
        );
        let mut req = self.http.get(&url).bearer_auth(&token);
        if !self.config.job_types.is_empty() {
            req = req.query(&[("types", self.config.job_types_query())]);
        }
        let resp = req.send().await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(WorkerError::BackendError { status, body });
        }

        let body = resp.bytes().await?;
        // The backend returns `null` JSON when no job is available.
        if body.as_ref() == b"null" || body.is_empty() {
            return Ok(None);
        }
        let job = serde_json::from_slice::<ClaimedJob>(&body)?;
        debug!(job_id = %job.job_id, job_type = %job.job_type, "claimed job");
        Ok(Some(job))
    }

    /// Report a job as completed.
    pub async fn complete_job(&self, job_id: Uuid, body: CompleteJobRequest) -> Result<()> {
        let token = generate_token(&self.config)?;
        let url = format!(
            "{}/api/worker/jobs/{}/complete",
            self.config.back_url.trim_end_matches('/'),
            job_id
        );
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(WorkerError::BackendError { status, body });
        }
        Ok(())
    }

    /// Report a job as failed.
    pub async fn fail_job(&self, job_id: Uuid, error: &str) -> Result<()> {
        let token = generate_token(&self.config)?;
        let url = format!(
            "{}/api/worker/jobs/{}/fail",
            self.config.back_url.trim_end_matches('/'),
            job_id
        );
        let body = FailJobRequest {
            error: error.to_string(),
        };
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(WorkerError::BackendError { status, body });
        }
        Ok(())
    }

    /// Download a file from a presigned URL to a local path.
    pub async fn download_presigned(&self, url: &str, dest: &std::path::Path) -> Result<()> {
        let resp = self.http.get(url).send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            return Err(WorkerError::BackendError {
                status,
                body: "failed to download from presigned URL".to_string(),
            });
        }
        let bytes = resp.bytes().await?;
        tokio::fs::write(dest, &bytes).await?;
        Ok(())
    }

    /// Upload a file to a presigned PUT URL.
    pub async fn upload_presigned(&self, url: &str, src: &std::path::Path) -> Result<()> {
        let data = tokio::fs::read(src).await?;
        let resp = self.http.put(url).body(data).send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(WorkerError::BackendError { status, body });
        }
        Ok(())
    }
}
