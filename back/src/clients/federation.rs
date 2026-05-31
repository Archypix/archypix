use crate::domain::auth::TokenType;
use crate::infra::config::Config;
use crate::infra::crypto::JwtService;
use crate::infra::error::AppError;
use crate::infra::redis::{RedisClient, RedisKey};
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tracing::{debug, trace, warn};
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
        if let Some(cached) = self
            .redis
            .get_string(RedisKey::FederationBackend(username, global_domain))
            .await
            .ok()
            .flatten()
        {
            trace!(
                username,
                global_domain, "federation: backend domain resolved from cache"
            );
            return Ok(cached);
        }

        debug!(
            username,
            global_domain, "federation: resolving backend domain via WebFinger"
        );
        let url = format!(
            "{}://{}/.well-known/webfinger",
            self.config.federation_scheme(),
            global_domain
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
            .map_err(|e| {
                warn!(username, global_domain, error = %e, "federation: WebFinger request failed");
                AppError::InternalServerError(e.to_string())
            })?
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
        debug!(
            username,
            global_domain, backend_domain, "federation: backend domain resolved via WebFinger"
        );

        self.redis
            .set_string_ex(
                RedisKey::FederationBackend(username, global_domain),
                &backend_domain,
                self.config.federation_backend_cache_ttl_secs,
            )
            .await?;

        Ok(backend_domain)
    }

    /// Request a federation token from the remote instance identified by `recipient_global_domain`.
    pub async fn ensure_federation_token(
        &self,
        sender_username: &str,
        recipient_username: &str,
        recipient_global_domain: &str,
    ) -> Result<Option<String>, AppError> {
        if let Some(token) = self
            .redis
            .get_string(RedisKey::FederationToken(recipient_global_domain))
            .await
            .ok()
            .flatten()
        {
            trace!(
                recipient_global_domain,
                "federation: token resolved from cache"
            );
            return Ok(Some(token));
        }

        let backend_domain = self
            .resolve_backend_domain(recipient_username, recipient_global_domain)
            .await?;

        debug!(
            sender = sender_username,
            recipient_global_domain, backend_domain, "federation: requesting auth token"
        );
        let request_url = format!(
            "{}://{}/api/federation/auth/request",
            self.config.federation_scheme(),
            backend_domain
        );

        self.http
            .post(&request_url)
            .json(&FederationTokenRequest {
                requester_instance: self.config.global_domain.clone(),
                username: sender_username.to_string(),
                scope: "federation".to_string(),
                nonce: Uuid::new_v4().to_string(),
            })
            .timeout(Duration::from_millis(
                self.config.federation_request_timeout_ms,
            ))
            .send()
            .await
            .map_err(|e| {
                warn!(recipient_global_domain, error = %e, "federation: auth request failed");
                AppError::InternalServerError(e.to_string())
            })?
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

        debug!(
            recipient_global_domain,
            "federation: waiting for auth token grant"
        );
        let deadline = Duration::from_millis(self.config.federation_request_timeout_ms);
        let domain = recipient_global_domain;

        timeout(deadline, async move {
            loop {
                if let Some(token) = self
                    .redis
                    .get_string(RedisKey::FederationToken(domain))
                    .await
                    .ok()
                    .flatten()
                {
                    return Ok(token);
                }
                sleep(Duration::from_millis(200)).await;
            }
        })
        .await
        .map_err(|_| {
            warn!(
                recipient_global_domain,
                "federation: auth token grant timed out"
            );
            AppError::BadRequest("Federation token request timed out".to_string())
        })?
    }

    /// Store a federation token received via the `/api/federation/auth/grant` callback.
    pub async fn store_federation_token(
        &self,
        issuer_global_domain: &str,
        token: &str,
        ttl_secs: i64,
    ) -> Result<(), AppError> {
        let ttl = ttl_secs
            .try_into()
            .map_err(|_| AppError::BadRequest("Invalid token TTL".to_string()))?;
        trace!(
            issuer_global_domain,
            ttl_secs, "federation: storing auth token"
        );
        self.redis
            .set_string_ex(RedisKey::FederationToken(issuer_global_domain), token, ttl)
            .await
    }

    /// Issue a federation JWT for a requesting instance (used in the auth handshake).
    pub fn issue_federation_token(
        &self,
        requester_global_domain: &str,
    ) -> Result<String, AppError> {
        self.jwt.issue(
            requester_global_domain,
            None,
            &self.config.global_domain,
            TokenType::Federation,
            false,
            &self.config.back_domain,
            self.config.federation_jwt_ttl_secs,
        )
    }

    /// Send the federation token grant to the requester's backend.
    pub async fn send_auth_grant(
        &self,
        username: &str,
        requester_global_domain: &str,
        grant: &FederationAuthGrant,
    ) -> Result<(), AppError> {
        let backend_domain = self
            .resolve_backend_domain(username, requester_global_domain)
            .await?;
        debug!(
            requester_global_domain,
            backend_domain, "federation: sending auth grant"
        );
        let callback_url = format!(
            "{}://{}/api/federation/auth/grant",
            self.config.federation_scheme(),
            backend_domain
        );
        let resp = self
            .http
            .post(callback_url)
            .json(grant)
            .send()
            .await
            .map_err(|e| {
                warn!(requester_global_domain, error = %e, "federation: auth grant delivery failed");
                AppError::InternalServerError(e.to_string())
            })?;

        if !resp.status().is_success() {
            warn!(requester_global_domain, status = %resp.status(), "federation: auth grant rejected by remote");
            return Err(AppError::InternalServerError(format!(
                "Callback rejected grant: {}",
                resp.status()
            )));
        }
        Ok(())
    }

    /// Request a presigned URL for a picture stored on a remote instance.
    ///
    /// Used when serving thumbnails/originals for received (non-owned) pictures.
    /// Pass the `share_token` received from the picture's origin `IncomingShare` so
    /// the remote instance can authorize the request even for transitive shares.
    /// Request a presigned URL for a picture stored on a remote instance.
    /// No federation JWT needed — the share_token is the sole authorization proof.
    /// The backend domain is resolved via WebFinger (cached in Redis).
    pub async fn presign_remote_picture(
        &self,
        owner_username: &str,
        owner_global_domain: &str,
        picture_id: Uuid,
        variant: &str,
        share_token: Uuid,
    ) -> Result<String, AppError> {
        let backend_domain = self
            .resolve_backend_domain(owner_username, owner_global_domain)
            .await?;
        let url = format!(
            "{}://{}/api/federation/pictures/presign",
            self.config.federation_scheme(),
            backend_domain
        );
        let resp = self
            .http
            .post(&url)
            .json(&RemotePresignRequest {
                owner_username: owner_username.to_string(),
                owner_instance: owner_global_domain.to_string(),
                picture_id: picture_id.to_string(),
                variant: variant.to_string(),
                share_token,
            })
            .send()
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?
            .error_for_status()
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        body["url"].as_str().map(str::to_string).ok_or_else(|| {
            AppError::InternalServerError("Missing url in presign response".to_string())
        })
    }

    /// Announce a new outgoing share to the recipient's backend.
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
        debug!(
            recipient = recipient_username,
            recipient_global_domain,
            backend_domain,
            tag_path = %announcement.tag_path,
            "federation: announcing share"
        );
        let url = format!(
            "{}://{}/api/federation/shares/announce",
            self.config.federation_scheme(),
            backend_domain
        );
        self.http
            .post(&url)
            .bearer_auth(token)
            .json(announcement)
            .send()
            .await
            .map_err(|e| {
                warn!(recipient_global_domain, error = %e, "federation: share announcement delivery failed");
                AppError::InternalServerError(e.to_string())
            })?
            .error_for_status()
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        Ok(())
    }
}

// ── Federation protocol types ─────────────────────────────────────────────────

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
    /// The sender's OutgoingShare token — forwarded to the recipient so they can
    /// authorize presign requests for transitively shared pictures.
    pub share_token: Uuid,
}

// ── Internal types ────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct FederationTokenRequest {
    requester_instance: String,
    username: String,
    scope: String,
    nonce: String,
}

#[derive(Serialize)]
struct RemotePresignRequest {
    owner_username: String,
    owner_instance: String,
    picture_id: String,
    variant: String,
    share_token: uuid::Uuid,
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

fn normalize_domain(url: &str) -> String {
    let stripped = url.trim().trim_end_matches('/');
    let stripped = stripped
        .strip_prefix("https://")
        .or_else(|| stripped.strip_prefix("http://"))
        .unwrap_or(stripped);
    stripped.split('/').next().unwrap_or(stripped).to_string()
}
