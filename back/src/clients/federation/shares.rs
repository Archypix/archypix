use super::{FederationClient, PicturesAnnouncement, ShareAnnouncement};
use crate::infra::error::AppError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, warn};
use uuid::Uuid;

impl FederationClient {
    /// Request presigned URLs for a batch of pictures stored on a remote instance.
    ///
    /// A single HTTP call is made per (owner_backend, share_token) pair, replacing the
    /// previous one-call-per-picture pattern. Returns a map of `picture_id (as string) → url`.
    pub async fn presign_remote_pictures(
        &self,
        owner_username: &str,
        owner_global_domain: &str,
        pictures: &[(Uuid, &str)],
        share_token: Uuid,
    ) -> Result<HashMap<String, String>, AppError> {
        let backend_base_url = self
            .resolve_backend_url(owner_username, owner_global_domain)
            .await?;
        let url = format!("{}/api/federation/pictures/presign", backend_base_url);

        let items: Vec<RemotePresignItem> = pictures
            .iter()
            .map(|(id, variant)| RemotePresignItem {
                picture_id: id.to_string(),
                variant: variant.to_string(),
            })
            .collect();

        let resp = self
            .http
            .post(&url)
            .json(&BatchPresignRequest {
                owner_username: owner_username.to_string(),
                owner_instance: owner_global_domain.to_string(),
                share_token,
                pictures: items,
            })
            .send()
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?
            .error_for_status()
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;

        let body: BatchPresignResponse = resp
            .json()
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;

        Ok(body
            .urls
            .into_iter()
            .map(|r| (r.picture_id, r.url))
            .collect())
    }

    /// Convenience wrapper: request a presigned URL for a single picture.
    pub async fn presign_remote_picture(
        &self,
        owner_username: &str,
        owner_global_domain: &str,
        picture_id: Uuid,
        variant: &str,
        share_token: Uuid,
    ) -> Result<String, AppError> {
        let mut results = self
            .presign_remote_pictures(
                owner_username,
                owner_global_domain,
                &[(picture_id, variant)],
                share_token,
            )
            .await?;
        results.remove(&picture_id.to_string()).ok_or_else(|| {
            AppError::InternalServerError("Empty presign response from remote backend".into())
        })
    }

    /// Notify the recipient's backend that an outgoing share has been revoked.
    ///
    /// Identified by `outgoing_share_id` so the recipient can look up their `IncomingShare`
    /// without Alice needing to know Bob's internal IDs.
    pub async fn send_revocation(
        &self,
        sender_username: &str,
        recipient_username: &str,
        recipient_global_domain: &str,
        outgoing_share_id: uuid::Uuid,
    ) -> Result<(), AppError> {
        let token = self
            .get_or_wait_federation_token(
                sender_username,
                recipient_username,
                recipient_global_domain,
            )
            .await?;
        let backend_base_url = self
            .resolve_backend_url(recipient_username, recipient_global_domain)
            .await?;
        debug!(
            recipient_global_domain,
            backend_base_url,
            %outgoing_share_id,
            "federation: sending share revocation"
        );
        let url = format!("{}/api/federation/shares/revoke", backend_base_url);
        self.http
            .post(&url)
            .bearer_auth(&token)
            .json(&ShareRevokeRequest { outgoing_share_id })
            .send()
            .await
            .map_err(|e| {
                warn!(recipient_global_domain, error = %e, "federation: revocation delivery failed");
                AppError::InternalServerError(e.to_string())
            })?
            .error_for_status()
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        Ok(())
    }

    /// Announce a new outgoing share to the recipient's backend.
    pub async fn announce_share(
        &self,
        recipient_username: &str,
        recipient_global_domain: &str,
        token: &str,
        announcement: &ShareAnnouncement,
    ) -> Result<(), AppError> {
        let backend_base_url = self
            .resolve_backend_url(recipient_username, recipient_global_domain)
            .await?;
        debug!(
            recipient = recipient_username,
            recipient_global_domain,
            backend_base_url,
            tag_path = %announcement.tag_path,
            "federation: announcing share"
        );
        let url = format!("{}/api/federation/shares/announce", backend_base_url);
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

    /// Send a share-acceptance notification to the sender's backend.
    ///
    /// Called by the recipient (Bob) after accepting an incoming share. The sender (Alice) will
    /// respond by announcing all current pictures under the shared tag.
    pub async fn send_share_accept(
        &self,
        acceptor_username: &str,
        sender_username: &str,
        sender_global_domain: &str,
        outgoing_share_id: Uuid,
    ) -> Result<(), AppError> {
        let token = self
            .get_or_wait_federation_token(acceptor_username, sender_username, sender_global_domain)
            .await?;
        let backend_base_url = self
            .resolve_backend_url(sender_username, sender_global_domain)
            .await?;
        debug!(
            sender_global_domain,
            backend_base_url,
            %outgoing_share_id,
            "federation: sending share accept"
        );
        let url = format!("{}/api/federation/shares/accept", backend_base_url);
        self.http
            .post(&url)
            .bearer_auth(&token)
            .json(&ShareAcceptRequest { outgoing_share_id })
            .send()
            .await
            .map_err(|e| {
                warn!(sender_global_domain, error = %e, "federation: share accept delivery failed");
                AppError::InternalServerError(e.to_string())
            })?
            .error_for_status()
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        Ok(())
    }

    /// Send a share-rejection notification to the sender's backend.
    ///
    /// Called by the recipient (Bob) after rejecting an incoming share. The sender (Alice) will
    /// tombstone her OutgoingShare so it no longer appears as pending/active on her side.
    pub async fn send_share_reject(
        &self,
        rejector_username: &str,
        sender_username: &str,
        sender_global_domain: &str,
        outgoing_share_id: Uuid,
    ) -> Result<(), AppError> {
        let token = self
            .get_or_wait_federation_token(rejector_username, sender_username, sender_global_domain)
            .await?;
        let backend_base_url = self
            .resolve_backend_url(sender_username, sender_global_domain)
            .await?;
        debug!(
            sender_global_domain,
            backend_base_url,
            %outgoing_share_id,
            "federation: sending share reject"
        );
        let url = format!("{}/api/federation/shares/reject", backend_base_url);
        self.http
            .post(&url)
            .bearer_auth(&token)
            .json(&ShareRejectRequest { outgoing_share_id })
            .send()
            .await
            .map_err(|e| {
                warn!(sender_global_domain, error = %e, "federation: share reject delivery failed");
                AppError::InternalServerError(e.to_string())
            })?
            .error_for_status()
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        Ok(())
    }

    /// Announce a batch of pictures to the recipient's backend after share acceptance.
    ///
    /// Called by the sender (Alice) to push all pictures currently under the shared tag to Bob.
    pub async fn announce_pictures_to_backend(
        &self,
        sender_username: &str,
        recipient_username: &str,
        recipient_global_domain: &str,
        payload: &PicturesAnnouncement,
    ) -> Result<(), AppError> {
        let token = self
            .get_or_wait_federation_token(
                sender_username,
                recipient_username,
                recipient_global_domain,
            )
            .await?;
        let backend_base_url = self
            .resolve_backend_url(recipient_username, recipient_global_domain)
            .await?;
        debug!(
            recipient_global_domain,
            backend_base_url,
            picture_count = payload.pictures.len(),
            "federation: announcing pictures"
        );
        let url = format!("{}/api/federation/pictures/announce", backend_base_url);
        self.http
            .post(&url)
            .bearer_auth(&token)
            .json(payload)
            .send()
            .await
            .map_err(|e| {
                warn!(recipient_global_domain, error = %e, "federation: pictures announcement delivery failed");
                AppError::InternalServerError(e.to_string())
            })?
            .error_for_status()
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
        Ok(())
    }
}

// ── Internal request/response types ──────────────────────────────────────────

#[derive(Serialize)]
struct RemotePresignItem {
    picture_id: String,
    variant: String,
}

#[derive(Serialize)]
struct BatchPresignRequest {
    owner_username: String,
    owner_instance: String,
    share_token: Uuid,
    pictures: Vec<RemotePresignItem>,
}

#[derive(Deserialize)]
struct PresignResultItem {
    picture_id: String,
    url: String,
}

#[derive(Deserialize)]
struct BatchPresignResponse {
    urls: Vec<PresignResultItem>,
}

#[derive(Serialize)]
struct ShareAcceptRequest {
    outgoing_share_id: Uuid,
}

#[derive(Serialize)]
struct ShareRejectRequest {
    outgoing_share_id: Uuid,
}

#[derive(Serialize)]
struct ShareRevokeRequest {
    outgoing_share_id: Uuid,
}
