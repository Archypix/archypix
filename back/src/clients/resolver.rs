use crate::domain::auth::{JwtClaims, TokenType};
use crate::infra::config::Config;
use crate::infra::crypto::JwtService;
use crate::infra::error::AppError;
use reqwest::Client as HttpClient;
use serde::Serialize;

/// Outbound client for the Resolver service — registers user→backend mappings.
#[derive(Clone)]
pub struct ResolverClient {
    http: HttpClient,
    config: Config,
    jwt: JwtService,
}

impl ResolverClient {
    pub fn new(http: HttpClient, config: Config, jwt: JwtService) -> Self {
        Self { http, config, jwt }
    }

    /// Verify an inbound resolver JWT (used by the resolver auth middleware).
    pub fn verify_token(&self, token: &str) -> Result<JwtClaims, AppError> {
        self.jwt.decode_any_issuer(token, &self.config.host)
    }

    /// Register or update the username→backend mapping in the resolver.
    /// No-op when `use_resolver` is false.
    pub async fn update_mapping(&self, username: &str, backend_url: &str) -> Result<(), AppError> {
        if !self.config.use_resolver {
            return Ok(());
        }

        let token = self.jwt.issue(
            "resolver-update",
            None,
            &self.config.host,
            TokenType::Resolver,
            false,
            &self.config.webfinger_host,
            300,
        )?;

        let url = format!(
            "{}/api/update",
            self.config.resolver_url.trim_end_matches('/')
        );

        self.http
            .post(&url)
            .bearer_auth(token)
            .json(&ResolverUpdateRequest {
                username: username.to_string(),
                backend_url: backend_url.to_string(),
            })
            .send()
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?
            .error_for_status()
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;

        Ok(())
    }
}

#[derive(Serialize)]
struct ResolverUpdateRequest {
    username: String,
    backend_url: String,
}
