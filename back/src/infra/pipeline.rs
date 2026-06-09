//! Tagging pipeline background loop.
//!
//! The pipeline evaluates enabled tagging services against dirty pictures and
//! applies the resulting tag assignments. A picture is dirty when:
//! - Its `last_pipeline_run_at` is NULL (never processed), or
//! - Its `last_pipeline_run_at` is older than any enabled service's `last_invalidated_at`.
//!
//! # Wake model
//! The loop uses a `tokio::sync::Notify` for event-driven wakes (e.g. after ingest,
//! manual tag change, or service config change) and falls back to a configurable
//! polling interval for crash recovery.
//!
//! # Evaluation order
//! Services run in fixed order: `SharedTagMapping → Rule → Segmentation`.
//! Each service sees the tags added by the previous ones (in-memory accumulation),
//! enabling downstream services to use upstream results via `requires`.

use crate::domain::pipeline::{self, PipelineInput};
use crate::domain::tag::TagPath;
use crate::domain::tagging::ServiceType;
use crate::repository::pipeline::{PipelineRepository, PipelineTagAssignment};
use crate::repository::tag::TagRepository;
use crate::repository::tagging::{
    RuleTaggingRuleRepository, SegmentationRuleRepository, SharedTagMappingRuleRepository,
    TaggingServiceRepository,
};
use chrono::Utc;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use uuid::Uuid;

/// Spawn the pipeline loop as a Tokio task.
///
/// `notify` — shared with `AppState`; call `notify.notify_one()` to wake the loop immediately.
/// `poll_interval` — fallback sweep period for crash recovery.
///
/// Returns a future that runs forever (until the process exits). Spawn it with `tokio::spawn`.
pub fn create(
    db: PgPool,
    notify: Arc<Notify>,
    poll_interval: Duration,
) -> impl std::future::Future<Output = ()> {
    async move { run(db, notify, poll_interval).await }
}

async fn run(db: PgPool, notify: Arc<Notify>, poll_interval: Duration) {
    tracing::info!(
        poll_interval_secs = poll_interval.as_secs(),
        "tagging pipeline loop started"
    );
    loop {
        tokio::select! {
            _ = notify.notified() => {
                tracing::debug!("pipeline: woken by event");
            }
            _ = tokio::time::sleep(poll_interval) => {
                tracing::debug!("pipeline: recovery sweep");
            }
        }

        if let Err(e) = sweep(&db).await {
            tracing::error!(error = ?e, "pipeline sweep error");
        }
    }
}

async fn sweep(db: &PgPool) -> Result<(), sqlx::Error> {
    let users = PipelineRepository::find_users_with_dirty_pictures(db)
        .await
        .map_err(|e| {
            tracing::error!(error = ?e, "pipeline: failed to find dirty users");
            sqlx::Error::PoolTimedOut // generic — error already logged
        })?;

    if users.is_empty() {
        return Ok(());
    }

    tracing::debug!(user_count = users.len(), "pipeline: processing dirty users");
    for user_id in users {
        if let Err(e) = run_for_user(db, user_id).await {
            tracing::error!(user_id = %user_id, error = ?e, "pipeline: failed for user");
        }
    }
    Ok(())
}

async fn run_for_user(db: &PgPool, user_id: Uuid) -> Result<(), anyhow::Error> {
    let run_at = Utc::now().naive_utc();

    // ── Load services ─────────────────────────────────────────────────────────
    let services = TaggingServiceRepository::list_enabled_by_owner(db, user_id).await?;
    if services.is_empty() {
        return Ok(());
    }

    // ── Load all rule sub-items (3 parallel queries) ──────────────────────────
    let mut mapping_ids: Vec<Uuid> = vec![];
    let mut rule_ids: Vec<Uuid> = vec![];
    let mut segment_ids: Vec<Uuid> = vec![];
    for svc in &services {
        match svc.service_type {
            ServiceType::SharedTagMapping => mapping_ids.push(svc.id),
            ServiceType::Rule => rule_ids.push(svc.id),
            ServiceType::Segmentation => segment_ids.push(svc.id),
        }
    }

    let (all_mappings, all_rules, all_segments) = tokio::try_join!(
        SharedTagMappingRuleRepository::list_for_services(db, &mapping_ids),
        RuleTaggingRuleRepository::list_for_services(db, &rule_ids),
        SegmentationRuleRepository::list_for_services(db, &segment_ids),
    )?;

    // Group by service_id for O(1) lookup during evaluation.
    let mappings_by_svc: HashMap<Uuid, Vec<_>> = group_by_service(&all_mappings, |r| r.service_id);
    let rules_by_svc: HashMap<Uuid, Vec<_>> = group_by_service(&all_rules, |r| r.service_id);
    let segments_by_svc: HashMap<Uuid, Vec<_>> = group_by_service(&all_segments, |r| r.service_id);

    // ── Find dirty pictures ───────────────────────────────────────────────────
    let dirty = PipelineRepository::find_dirty_for_user(db, user_id).await?;
    if dirty.is_empty() {
        return Ok(());
    }

    tracing::debug!(
        user_id = %user_id,
        picture_count = dirty.len(),
        "pipeline: evaluating dirty pictures"
    );

    // ── Process in batches of 100 ─────────────────────────────────────────────
    for chunk in dirty.chunks(100) {
        let picture_ids: Vec<Uuid> = chunk.iter().map(|p| p.id).collect();

        // Load tags and incoming share IDs for the whole batch at once.
        let all_tags = TagRepository::list_for_pictures(db, &picture_ids).await?;
        let share_ids_by_pic =
            PipelineRepository::find_incoming_share_ids(db, &picture_ids).await?;

        // Group tags by picture_id.
        let mut tags_by_pic: HashMap<Uuid, Vec<TagPath>> = HashMap::new();
        for tag in all_tags {
            tags_by_pic
                .entry(tag.picture_id)
                .or_default()
                .push(TagPath::from_ltree(&tag.tag_path));
        }

        let mut success_ids: Vec<Uuid> = Vec::with_capacity(chunk.len());

        for picture in chunk {
            let mut current_tags: Vec<TagPath> =
                tags_by_pic.remove(&picture.id).unwrap_or_default();
            let incoming_share_ids: &[Uuid] = share_ids_by_pic
                .get(&picture.id)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);

            let mut assignments: Vec<PipelineTagAssignment> = Vec::new();

            for service in &services {
                // Rebuild input with the current in-memory tag set so each service
                // sees tags added by earlier ones in this run.
                let input = PipelineInput {
                    picture_id: picture.id,
                    captured_at: picture.captured_at,
                    gps_lat: picture.gps_lat,
                    gps_lng: picture.gps_lng,
                    filename: picture.filename.clone(),
                    current_tags: current_tags.clone(),
                };

                if !pipeline::should_run(service, &input) {
                    continue;
                }

                let result = match service.service_type {
                    ServiceType::SharedTagMapping => pipeline::evaluate_shared_tag_mapping(
                        mappings_by_svc
                            .get(&service.id)
                            .map(|v| v.as_slice())
                            .unwrap_or(&[]),
                        incoming_share_ids,
                    ),
                    ServiceType::Rule => pipeline::evaluate_rule(
                        rules_by_svc
                            .get(&service.id)
                            .map(|v| v.as_slice())
                            .unwrap_or(&[]),
                        &input,
                    ),
                    ServiceType::Segmentation => pipeline::evaluate_segmentation(
                        segments_by_svc
                            .get(&service.id)
                            .map(|v| v.as_slice())
                            .unwrap_or(&[]),
                        &input,
                    ),
                };

                let source_str = match service.service_type {
                    ServiceType::SharedTagMapping => "share_mapping",
                    ServiceType::Rule => "rule",
                    ServiceType::Segmentation => "segment",
                };

                for tag in result.tags_to_add {
                    // Skip if an equal or more-specific tag is already present.
                    // e.g. don't add Photos.Travel when Photos.Travel.Alps is already there.
                    if current_tags
                        .iter()
                        .any(|t| t == &tag || tag.is_ancestor_of(t))
                    {
                        continue;
                    }
                    // Drop ancestors that are now redundant (replaced by this deeper tag).
                    // e.g. remove Photos.Travel when adding Photos.Travel.Alps.
                    current_tags.retain(|t| !t.is_ancestor_of(&tag));
                    current_tags.push(tag.clone());
                    assignments.push(PipelineTagAssignment {
                        tag_path: tag.as_ltree().to_string(),
                        source: source_str.to_string(),
                        source_id: service.id,
                    });
                }
            }

            // Write new tags for this picture.
            if let Err(e) = PipelineRepository::assign_tags(db, picture.id, &assignments).await {
                tracing::error!(
                    picture_id = %picture.id,
                    error = ?e,
                    "pipeline: failed to assign tags — picture will be retried"
                );
                continue; // do not mark this picture as run
            }

            // TODO: to support tag removal, we should tag that a test of a non-manual source
            //  is in none of the result.tags_to_add, and that its source_id has removal_enabled=true.
            //  This requires keeping track of result.tags_to_add, and matching the tag source with
            //  the service to get removal_enabled. If one of the services created an ancestor tag,
            //  the ancestor should be added after the removed one is removed.
            //  Other option : add source_id to the PK of tags, allowing to keep track of all tags
            //  individually, and to clean up the non-relevant tags automatically in sql by adding
            //  a condition in `WITH cleanup AS` on source_id.

            success_ids.push(picture.id);
        }

        // Mark successfully processed pictures so they aren't re-evaluated unnecessarily.
        if let Err(e) = PipelineRepository::mark_run(db, &success_ids, run_at).await {
            tracing::error!(error = ?e, "pipeline: failed to mark pictures as run");
        }
    }

    tracing::debug!(user_id = %user_id, "pipeline: sweep complete for user");
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn group_by_service<T: Clone, F: Fn(&T) -> Uuid>(items: &[T], key: F) -> HashMap<Uuid, Vec<T>> {
    let mut map: HashMap<Uuid, Vec<T>> = HashMap::new();
    for item in items {
        map.entry(key(item)).or_default().push(item.clone());
    }
    map
}
