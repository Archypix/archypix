use crate::domain::auth::TokenType;
use crate::infra::config::Config;
use crate::infra::crypto::JwtService;
use crate::infra::error::AppError;
use crate::infra::redis::RedisClient;
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::{sleep, timeout};
use uuid::Uuid;

/// Outbound federation HTTP client — resolves remote backends via WebFinger,
/// manages federation token lifecycle, and sends federation protocol messages.
#[derive(Clone)]
pub struct FederationClient {
    http: HttpClient,
    config: Config,
    jwt: JwtService,
    redis: RedisClient,
}

impl FederationClient {
    pub fn new(http: HttpClient, config: Config, jwt: JwtService, redis: RedisClient) -> Self {
        Self {
            http,
            config,
            jwt,
            redis,
        }
    }

    /// Resolve a user's owning backend domain via WebFinger, with Redis caching.
    pub async fn resolve_backend_domain(
        &self,
        username: &str,
        instance_domain: &str,
    ) -> Result<String, AppError> {
        let cache_key = backend_cache_key(username, instance_domain);
        if let Some(cached) = self.redis.get_string(&cache_key).await.ok().flatten() {
            return Ok(cached);
        }

        let url = format!(
            "{}://{}/.well-known/webfinger",
            self.config.federation_scheme, instance_domain
        );
        let response = self
            .http
            .get(&url)
            .query(&[(
                "resource",
                format!("acct:@{}:{}", username, instance_domain),
            )])
            .send()
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?
            .error_for_status()
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;

        let body: WebFingerResponse = response
            .json()
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;

        let backend_url = body
            .links
            .iter()
            .find(|l| l.rel == "backend_url")
            .map(|l| l.href.clone())
            .ok_or_else(|| AppError::BadRequest("Missing backend_url in WebFinger".to_string()))?;

        let backend_domain = normalize_domain(&backend_url);

        self.redis
            .set_string_ex(
                &cache_key,
                &backend_domain,
                self.config.federation_backend_cache_ttl_secs,
            )
            .await?;

        Ok(backend_domain)
    }

    /// Request a federation token from a remote backend (non-blocking — token arrives via callback).
    /// Returns `Some(token)` if already cached, `None` if a request was sent.
    pub async fn ensure_federation_token(
        &self,
        backend_domain: &str,
    ) -> Result<Option<String>, AppError> {
        let cache_key = token_cache_key(backend_domain);
        if let Some(token) = self.redis.get_string(&cache_key).await.ok().flatten() {
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

        self.http
            .post(&request_url)
            .json(&FederationTokenRequest {
                requester_instance: self.config.host.clone(),
                callback_url,
                scope: "federation".to_string(),
                nonce: Uuid::new_v4().to_string(),
            })
            .timeout(Duration::from_millis(
                self.config.federation_request_timeout_ms,
            ))
            .send()
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?
            .error_for_status()
            .map_err(|e| AppError::BadRequest(format!("Federation auth request failed: {e}")))?;

        Ok(None)
    }

    /// Get a valid federation token for `backend_domain`, waiting for the callback if needed.
    pub async fn get_or_wait_federation_token(
        &self,
        backend_domain: &str,
    ) -> Result<String, AppError> {
        if let Some(token) = self.ensure_federation_token(backend_domain).await? {
            return Ok(token);
        }

        let cache_key = token_cache_key(backend_domain);
        let deadline = Duration::from_millis(self.config.federation_request_timeout_ms);

        timeout(deadline, async {
            loop {
                if let Some(token) = self.redis.get_string(&cache_key).await.ok().flatten() {
                    return Ok(token);
                }
                sleep(Duration::from_millis(200)).await;
            }
        })
        .await
        .map_err(|_| AppError::BadRequest("Federation token request timed out".to_string()))?
    }

    /// Store a federation token received via the auth/grant callback.
    pub async fn store_federation_token(
        &self,
        issuer_instance: &str,
        token: &str,
        ttl_secs: i64,
    ) -> Result<(), AppError> {
        let ttl = ttl_secs
            .try_into()
            .map_err(|_| AppError::BadRequest("Invalid token TTL".to_string()))?;
        self.redis
            .set_string_ex(&token_cache_key(issuer_instance), token, ttl)
            .await
    }

    /// Issue a federation JWT for a requesting instance (used in the auth handshake).
    pub fn issue_federation_token(&self, requester_instance: &str) -> Result<String, AppError> {
        self.jwt.issue(
            requester_instance,
            None,
            &self.config.host,
            TokenType::Federation,
            false,
            &self.config.host,
            self.config.federation_jwt_ttl_secs,
        )
    }

    /// Send the federation token grant to the requester's callback URL.
    pub async fn send_auth_grant(
        &self,
        callback_url: &str,
        grant: &FederationAuthGrant,
    ) -> Result<(), AppError> {
        let resp = self
            .http
            .post(callback_url)
            .json(grant)
            .send()
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(AppError::InternalServerError(format!(
                "Callback rejected grant: {}",
                resp.status()
            )));
        }
        Ok(())
    }

    /// Announce a new outgoing share to the recipient's backend.
    pub async fn announce_share(
        &self,
        backend_domain: &str,
        token: &str,
        announcement: &ShareAnnouncement,
    ) -> Result<(), AppError> {
        let url = format!(
            "{}://{}/api/federation/shares/announce",
            self.config.federation_scheme, backend_domain
        );
        self.http
            .post(&url)
            .bearer_auth(token)
            .json(announcement)
            .send()
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?
            .error_for_status()
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        Ok(())
    }
}

// ── Federation protocol types (shared between inbound handlers and outbound client) ──

#[derive(Debug, Serialize, Deserialize)]
pub struct FederationAuthGrant {
    pub issuer_instance: String,
    pub token: String,
    pub expires_at: i64,
    pub scope: String,
    pub nonce: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ShareAnnouncement {
    pub sender_username: String,
    pub sender_instance: String,
    pub recipient_username: String,
    pub recipient_instance: String,
    pub outgoing_share_id: Uuid,
    pub tag_path: String,
    pub allow_share_back: bool,
    pub future: bool,
    pub shareback_of: Option<Uuid>,
}

// ── Internal types ──

#[derive(Serialize)]
struct FederationTokenRequest {
    requester_instance: String,
    callback_url: String,
    scope: String,
    nonce: String,
}

#[derive(Deserialize)]
struct WebFingerResponse {
    links: Vec<WebFingerLink>,
}

#[derive(Deserialize)]
struct WebFingerLink {
    rel: String,
    href: String,
}

fn backend_cache_key(username: &str, instance_domain: &str) -> String {
    format!("federation:backend:{}@{}", username, instance_domain)
}

fn token_cache_key(backend_domain: &str) -> String {
    format!("federation:token:{}", backend_domain)
}

fn normalize_domain(url: &str) -> String {
    let stripped = url.trim().trim_end_matches('/');
    let stripped = stripped
        .strip_prefix("https://")
        .or_else(|| stripped.strip_prefix("http://"))
        .unwrap_or(stripped);
    stripped.split('/').next().unwrap_or(stripped).to_string()
}
