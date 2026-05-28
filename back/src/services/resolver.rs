use crate::domain::auth::TokenType;
use crate::infrastructure::config::Config;
use crate::infrastructure::error::AppError;
use crate::services::auth::JwtService;
use reqwest::Client;
use serde::Deserialize;

#[derive(Clone)]
pub struct ResolverClient {
    http: Client,
    config: Config,
    resolver_jwt: JwtService,
}

impl ResolverClient {
    pub fn new(http: Client, config: Config, resolver_jwt: JwtService) -> Self {
        Self {
            http,
            config,
            resolver_jwt,
        }
    }

    pub async fn update_mapping(&self, username: &str, backend_url: &str) -> Result<(), AppError> {
        if !self.config.use_resolver {
            return Ok(());
        }

        let token = self
            .resolver_jwt
            .issue(
                "resolver-update",
                None,
                &self.config.host,
                TokenType::Resolver,
                false,
                &self.config.webfinger_host,
                300,
            )
            .map_err(|err| AppError::InternalServerError(err.to_string()))?;

        let url = format!(
            "{}/api/update",
            self.config.resolver_url.trim_end_matches('/')
        );

        self.http
            .post(url)
            .bearer_auth(token)
            .json(&ResolverUpdateRequest {
                username: username.to_string(),
                backend_url: backend_url.to_string(),
            })
            .send()
            .await
            .map_err(|err| AppError::InternalServerError(err.to_string()))?
            .error_for_status()
            .map_err(|err| AppError::InternalServerError(err.to_string()))?;

        Ok(())
    }
}

#[derive(Deserialize, serde::Serialize)]
struct ResolverUpdateRequest {
    username: String,
    backend_url: String,
}
