use crate::domain::tag::TagPath;
use crate::domain::tagging::{SegmentationRule, ServiceType, SharedTagMappingRule, TaggingService};
use chrono::NaiveDateTime;
use uuid::Uuid;

/// Labels carried by a pipeline event — determines which services re-run.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EventLabel {
    IncomingShare,
    Ingest,
    Metadata,
    ManualTag,
    RuleEdit,
    SegmentationEdit,
}

/// Input fed to the pipeline evaluator for a single picture.
#[derive(Debug, Clone)]
pub struct PipelineInput {
    pub picture_id: Uuid,
    pub captured_at: Option<NaiveDateTime>,
    pub current_tags: Vec<TagPath>,
}

/// Result of evaluating one service against one picture.
#[derive(Debug, Clone)]
pub struct ServiceResult {
    pub tags_to_add: Vec<TagPath>,
    pub tags_to_remove: Vec<TagPath>,
}

/// Determine whether a service should run for this input and event.
pub fn should_run(
    service: &TaggingService,
    input: &PipelineInput,
    event_labels: &[EventLabel],
) -> bool {
    if !service.enabled {
        return false;
    }
    let triggered = match service.service_type {
        ServiceType::SharedTagMapping => event_labels.contains(&EventLabel::IncomingShare),
        ServiceType::Rule => {
            event_labels.contains(&EventLabel::IncomingShare)
                || event_labels.contains(&EventLabel::Ingest)
                || event_labels.contains(&EventLabel::Metadata)
                || event_labels.contains(&EventLabel::ManualTag)
                || event_labels.contains(&EventLabel::RuleEdit)
        }
        ServiceType::Segmentation => {
            event_labels.contains(&EventLabel::IncomingShare)
                || event_labels.contains(&EventLabel::Ingest)
                || event_labels.contains(&EventLabel::Metadata)
                || event_labels.contains(&EventLabel::ManualTag)
                || event_labels.contains(&EventLabel::RuleEdit)
                || event_labels.contains(&EventLabel::SegmentationEdit)
        }
    };
    if !triggered {
        return false;
    }
    // Check requires: picture must have ALL tags in the requires list.
    let satisfied = service.requires.iter().all(|req| {
        let req_path = TagPath::from_ltree(req);
        input
            .current_tags
            .iter()
            .any(|t| t == &req_path || t.ancestors().contains(&req_path))
    });
    // Check excludes: picture must have NONE of the tags in the excludes list.
    let excluded = service.excludes.iter().any(|exc| {
        let exc_path = TagPath::from_ltree(exc);
        input
            .current_tags
            .iter()
            .any(|t| t == &exc_path || t.ancestors().contains(&exc_path))
    });
    satisfied && !excluded
}

/// Evaluate a segmentation service against an input — pure, no I/O.
pub fn evaluate_segmentation(rules: &[SegmentationRule], input: &PipelineInput) -> ServiceResult {
    let mut tags_to_add = Vec::new();
    if let Some(captured_at) = input.captured_at {
        for rule in rules {
            if captured_at >= rule.date_start && captured_at <= rule.date_end {
                tags_to_add.push(TagPath::from_ltree(&rule.assign_tag));
            }
        }
    }
    ServiceResult {
        tags_to_add,
        tags_to_remove: vec![],
    }
}

/// Evaluate a shared-tag-mapping service — pure, no I/O.
pub fn evaluate_shared_tag_mapping(
    rules: &[SharedTagMappingRule],
    incoming_share_ids: &[Uuid],
) -> ServiceResult {
    let tags_to_add = rules
        .iter()
        .filter(|r| !r.is_broken && incoming_share_ids.contains(&r.incoming_share_id))
        .map(|r| TagPath::from_ltree(&r.assign_tag))
        .collect();
    ServiceResult {
        tags_to_add,
        tags_to_remove: vec![],
    }
}
