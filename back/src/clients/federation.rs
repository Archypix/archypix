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
///
/// Domain terminology:
/// - **global domain** (WebFinger domain): the public identity domain, e.g. `example.com`.
///   Used in `@user:example.com` identities, stored in JWTs and the database.
/// - **backend domain**: the actual API server domain, e.g. `backend1.example.com`.
///   Resolved at request time via WebFinger; never stored persistently.
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
    ///
    /// Queries `global_domain/.well-known/webfinger?resource=acct:@username:global_domain`
    /// and returns the `backend_url` link. Result is cached by `(username, global_domain)`.
    pub async fn resolve_backend_domain(
        &self,
        username: &str,
        global_domain: &str,
    ) -> Result<String, AppError> {
        let cache_key = backend_cache_key(username, global_domain);
        if let Some(cached) = self.redis.get_string(&cache_key).await.ok().flatten() {
            return Ok(cached);
        }

        let url = format!(
            "{}://{}/.well-known/webfinger",
            self.config.federation_scheme, global_domain
        );
        let response = self
            .http
            .get(&url)
            .query(&[(
                "resource",
                format!("archypix:@{}:{}", username, global_domain),
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

    /// Request a federation token from the remote instance identified by `recipient_global_domain`.
    ///
    /// - `sender_username`: a local user included in the request so the remote can resolve our
    ///   backend domain via WebFinger when sending the grant callback.
    /// - `recipient_username`: used to resolve the remote backend domain via WebFinger.
    /// - `recipient_global_domain`: the remote instance's global (WebFinger) domain.
    ///
    /// The token is cached under `recipient_global_domain`.
    /// Returns `Some(token)` if already cached, `None` if a request was sent and the token will
    /// arrive asynchronously via the `/api/federation/auth/grant` callback.
    pub async fn ensure_federation_token(
        &self,
        sender_username: &str,
        recipient_username: &str,
        recipient_global_domain: &str,
    ) -> Result<Option<String>, AppError> {
        let cache_key = token_cache_key(recipient_global_domain);
        if let Some(token) = self.redis.get_string(&cache_key).await.ok().flatten() {
            return Ok(Some(token));
        }

        let backend_domain = self
            .resolve_backend_domain(recipient_username, recipient_global_domain)
            .await?;

        let request_url = format!(
            "{}://{}/api/federation/auth/request",
            self.config.federation_scheme, backend_domain
        );

        self.http
            .post(&request_url)
            .json(&FederationTokenRequest {
                requester_instance: self.config.webfinger_host.clone(),
                username: sender_username.to_string(),
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

    /// Get a valid federation token for `recipient_global_domain`, polling Redis until the
    /// grant callback arrives if the token is not already cached.
    pub async fn get_or_wait_federation_token(
        &self,
        sender_username: &str,
        recipient_username: &str,
        recipient_global_domain: &str,
    ) -> Result<String, AppError> {
        if let Some(token) = self
            .ensure_federation_token(sender_username, recipient_username, recipient_global_domain)
            .await?
        {
            return Ok(token);
        }

        let cache_key = token_cache_key(recipient_global_domain);
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

    /// Store a federation token received via the `/api/federation/auth/grant` callback.
    /// `issuer_global_domain` is the global (WebFinger) domain of the issuing instance.
    pub async fn store_federation_token(
        &self,
        issuer_global_domain: &str,
        token: &str,
        ttl_secs: i64,
    ) -> Result<(), AppError> {
        let ttl = ttl_secs
            .try_into()
            .map_err(|_| AppError::BadRequest("Invalid token TTL".to_string()))?;
        self.redis
            .set_string_ex(&token_cache_key(issuer_global_domain), token, ttl)
            .await
    }

    /// Issue a federation JWT for a requesting instance (used in the auth handshake).
    ///
    /// - `sub` = `requester_global_domain` (the requester's global identity)
    /// - `instance` = our own global (WebFinger) domain
    /// - `aud` = our backend domain (so this instance can verify tokens locally)
    pub fn issue_federation_token(
        &self,
        requester_global_domain: &str,
    ) -> Result<String, AppError> {
        self.jwt.issue(
            requester_global_domain,
            None,
            &self.config.webfinger_host,
            TokenType::Federation,
            false,
            &self.config.host,
            self.config.federation_jwt_ttl_secs,
        )
    }

    /// Send the federation token grant to the requester's backend.
    ///
    /// Resolves the requester's backend domain via WebFinger using `(username, requester_global_domain)`
    /// before sending, so no explicit callback URL is required in the auth request.
    pub async fn send_auth_grant(
        &self,
        username: &str,
        requester_global_domain: &str,
        grant: &FederationAuthGrant,
    ) -> Result<(), AppError> {
        let backend_domain = self
            .resolve_backend_domain(username, requester_global_domain)
            .await?;
        let callback_url = format!(
            "{}://{}/api/federation/auth/grant",
            self.config.federation_scheme, backend_domain
        );
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
    ///
    /// Resolves the recipient's backend domain via WebFinger (cached after the token was obtained).
    pub async fn announce_share(
        &self,
        recipient_username: &str,
        recipient_global_domain: &str,
        token: &str,
        announcement: &ShareAnnouncement,
    ) -> Result<(), AppError> {
        let backend_domain = self
            .resolve_backend_domain(recipient_username, recipient_global_domain)
            .await?;
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
    /// Global (WebFinger) domain of the issuing instance.
    pub issuer_instance: String,
    pub token: String,
    pub expires_at: i64,
    pub scope: String,
    pub nonce: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ShareAnnouncement {
    pub sender_username: String,
    /// Global (WebFinger) domain of the sender's instance.
    pub sender_instance: String,
    pub recipient_username: String,
    /// Global (WebFinger) domain of the recipient's instance.
    pub recipient_instance: String,
    pub outgoing_share_id: Uuid,
    pub tag_path: String,
    pub allow_share_back: bool,
    pub future: bool,
    pub shareback_of: Option<Uuid>,
}

// ── Internal types ──

/// Outbound auth request sent to a remote backend's `/api/federation/auth/request`.
#[derive(Serialize)]
struct FederationTokenRequest {
    /// Global (WebFinger) domain of the requesting instance.
    requester_instance: String,
    /// A user on the requesting instance; used by the remote to resolve our backend via WebFinger.
    username: String,
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

fn backend_cache_key(username: &str, global_domain: &str) -> String {
    format!("federation:backend:{}@{}", username, global_domain)
}

fn token_cache_key(global_domain: &str) -> String {
    format!("federation:token:{}", global_domain)
}

fn normalize_domain(url: &str) -> String {
    let stripped = url.trim().trim_end_matches('/');
    let stripped = stripped
        .strip_prefix("https://")
        .or_else(|| stripped.strip_prefix("http://"))
        .unwrap_or(stripped);
    stripped.split('/').next().unwrap_or(stripped).to_string()
}
