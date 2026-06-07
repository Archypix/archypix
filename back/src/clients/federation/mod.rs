mod handshake;
mod shares;
mod webfinger;

use crate::infra::config::Config;
use crate::infra::crypto::JwtService;
use crate::infra::redis::Cache;
use reqwest::Client as HttpClient;
use std::sync::Arc;

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
    pub(super) http: HttpClient,
    pub(super) config: Config,
    pub(super) jwt: JwtService,
    pub(super) cache: Arc<dyn Cache>,
}

impl FederationClient {
    pub fn new(http: HttpClient, config: Config, jwt: JwtService, cache: Arc<dyn Cache>) -> Self {
        Self {
            http,
            config,
            jwt,
            cache,
        }
    }
}

// ── Federation protocol types (shared across submodules) ─────────────────────

use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

#[derive(Debug, Serialize, Deserialize)]
pub struct AnnouncedPicture {
    pub picture_id: String,
    pub owner_username: String,
    pub owner_instance_domain: String,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub file_size: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub captured_at: Option<chrono::NaiveDateTime>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PicturesAnnouncement {
    pub outgoing_share_id: Uuid,
    /// Alice's shared tag path (LTREE format). The recipient uses this to build the
    /// `/SharedToMe/<sender>/<tag_path>` tag for each announced picture.
    pub tag_path: String,
    pub sender_username: String,
    pub sender_instance: String,
    pub pictures: Vec<AnnouncedPicture>,
}
