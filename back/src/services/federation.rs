use crate::domain::auth::TokenType;
use crate::infrastructure::config::Config;
use crate::infrastructure::error::AppError;
use crate::services::auth::JwtService;
use redis::AsyncCommands;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tokio::time::{sleep, timeout};

#[derive(Clone)]
pub struct FederationService {
    http: Client,
    config: Config,
    jwt: JwtService,
    redis: redis::aio::ConnectionManager,
}

impl FederationService {
    pub fn new(
        http: Client,
        config: Config,
        jwt: JwtService,
        redis: redis::aio::ConnectionManager,
    ) -> Self {
        Self {
            http,
            config,
            jwt,
            redis,
        }
    }

    pub async fn resolve_backend_domain(
        &self,
        username: &str,
        instance_domain: &str,
    ) -> Result<String, AppError> {
        let cache_key = backend_cache_key(username, instance_domain);
        let mut redis = self.redis.clone();
        if let Ok(Some(value)) = redis.get::<_, Option<String>>(&cache_key).await {
            return Ok(value);
        }

        let url = format!(
            "{}://{}/.well-known/webfinger",
            self.config.federation_scheme, instance_domain
        );
        let response = self
            .http
            .get(url)
            .query(&[(
                "resource",
                format!("acct:@{}:{}", username, instance_domain),
            )])
            .send()
            .await
            .map_err(|err| AppError::InternalServerError(err.to_string()))?
            .error_for_status()
            .map_err(|err| AppError::InternalServerError(err.to_string()))?;

        let body: WebFingerResponse = response
            .json()
            .await
            .map_err(|err| AppError::InternalServerError(err.to_string()))?;

        let backend_url = body
            .links
            .iter()
            .find(|link| link.rel == "backend_url")
            .map(|link| link.href.clone())
            .ok_or_else(|| AppError::BadRequest("Missing backend_url in WebFinger".to_string()))?;

        let backend_domain = normalize_backend_domain(&backend_url);

        let _: () = redis
            .set_ex(
                &cache_key,
                &backend_domain,
                self.config.federation_backend_cache_ttl_secs,
            )
            .await
            .map_err(|err| AppError::InternalServerError(err.to_string()))?;

        Ok(backend_domain)
    }

    pub async fn ensure_federation_token(
        &self,
        backend_domain: &str,
    ) -> Result<Option<String>, AppError> {
        let cache_key = token_cache_key(backend_domain);
        let mut redis = self.redis.clone();
        if let Ok(Some(token)) = redis.get::<_, Option<String>>(&cache_key).await {
            return Ok(Some(token));
        }

        let callback_url = format!(
            "{}/api/federation/auth/grant",
            self.config.public_base_url.trim_end_matches('/')
        );
        let request_url = format!(
            "{}://{}/api/federation/auth/request",
            self.config.federation_scheme, backend_domain
        );

        let nonce = uuid::Uuid::new_v4().to_string();
        let response = self
            .http
            .post(request_url)
            .json(&FederationAuthRequest {
                requester_instance: self.config.host.clone(),
                callback_url,
                scope: "federation".to_string(),
                nonce: nonce.clone(),
            })
            .timeout(Duration::from_millis(
                self.config.federation_request_timeout_ms,
            ))
            .send()
            .await
            .map_err(|err| AppError::InternalServerError(err.to_string()))?;

        if !response.status().is_success() {
            return Err(AppError::BadRequest(format!(
                "Federation auth request failed: {}",
                response.status()
            )));
        }

        Ok(None)
    }

    pub async fn get_or_wait_federation_token(
        &self,
        backend_domain: &str,
    ) -> Result<String, AppError> {
        match self.ensure_federation_token(backend_domain).await? {
            Some(token) => return Ok(token),
            None => {}
        }

        let cache_key = token_cache_key(backend_domain);
        let mut redis = self.redis.clone();
        let timeout_duration = Duration::from_millis(self.config.federation_request_timeout_ms);
        let poll_interval = Duration::from_millis(200);

        let result = timeout(timeout_duration, async {
            loop {
                if let Ok(Some(token)) = redis.get::<_, Option<String>>(&cache_key).await {
                    return Ok(token);
                }
                sleep(poll_interval).await;
            }
        })
        .await;

        match result {
            Ok(Ok(token)) => Ok(token),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(AppError::BadRequest(
                "Federation token request timed out".to_string(),
            )),
        }
    }

    pub async fn store_federation_token(
        &self,
        issuer_instance: &str,
        token: &str,
        ttl_secs: i64,
    ) -> Result<(), AppError> {
        let cache_key = token_cache_key(issuer_instance);
        let mut redis = self.redis.clone();
        let ttl = ttl_secs
            .try_into()
            .map_err(|_| AppError::BadRequest("Invalid token TTL".to_string()))?;
        let _: () = redis
            .set_ex(&cache_key, token, ttl)
            .await
            .map_err(|err| AppError::InternalServerError(err.to_string()))?;
        Ok(())
    }

    pub fn issue_federation_token(&self, requester_instance: &str) -> Result<String, AppError> {
        self.jwt
            .issue(
                requester_instance,
                None,
                &self.config.host,
                TokenType::Federation,
                false,
                &self.config.host,
                self.config.federation_jwt_ttl_secs,
            )
            .map_err(|err| AppError::InternalServerError(err.to_string()))
    }
}

#[derive(Debug, Deserialize)]
struct WebFingerResponse {
    links: Vec<WebFingerLink>,
}

#[derive(Debug, Deserialize)]
struct WebFingerLink {
    rel: String,
    href: String,
}

#[derive(serde::Serialize)]
struct FederationAuthRequest {
    requester_instance: String,
    callback_url: String,
    scope: String,
    nonce: String,
}

fn backend_cache_key(username: &str, instance_domain: &str) -> String {
    format!("federation:backend:{}@{}", username, instance_domain)
}

fn token_cache_key(backend_domain: &str) -> String {
    format!("federation:token:{}", backend_domain)
}

fn normalize_backend_domain(backend_url: &str) -> String {
    let trimmed = backend_url.trim().trim_end_matches('/');
    let stripped = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    stripped.split('/').next().unwrap_or(stripped).to_string()
}
