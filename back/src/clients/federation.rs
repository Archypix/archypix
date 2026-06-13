mod handshake;
pub mod models;
mod shares;
mod webfinger;

use crate::infra::config::Config;
use crate::infra::crypto::JwtService;
use crate::infra::redis::Cache;
use reqwest::Client as HttpClient;
use std::sync::Arc;

/// Outbound client for webfinger, federation auth, and protocol messages.
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
