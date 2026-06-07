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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tagging::{
        SegmentationRule, ServiceType, SharedTagMappingRule, TaggingService,
    };
    use chrono::NaiveDateTime;
    use uuid::Uuid;

    fn dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn service(stype: ServiceType, requires: &[&str], excludes: &[&str]) -> TaggingService {
        TaggingService {
            id: Uuid::new_v4(),
            owner_id: Uuid::new_v4(),
            service_type: stype,
            requires: requires.iter().map(|s| s.to_string()).collect(),
            excludes: excludes.iter().map(|s| s.to_string()).collect(),
            enabled: true,
            created_at: dt("2024-01-01 00:00:00"),
            updated_at: dt("2024-01-01 00:00:00"),
        }
    }

    fn input(tags: &[&str], captured_at: Option<&str>) -> PipelineInput {
        PipelineInput {
            picture_id: Uuid::new_v4(),
            captured_at: captured_at.map(|s| dt(s)),
            current_tags: tags.iter().map(|s| TagPath::from_ltree(*s)).collect(),
        }
    }

    // ── should_run: disabled ──────────────────────────────────────────────────

    #[test]
    fn disabled_service_never_runs() {
        let mut svc = service(ServiceType::Segmentation, &[], &[]);
        svc.enabled = false;
        let inp = input(&["Photos"], None);
        assert!(!should_run(&svc, &inp, &[EventLabel::Ingest]));
    }

    // ── should_run: label matching ────────────────────────────────────────────

    #[test]
    fn shared_mapping_only_fires_on_incoming_share_label() {
        let svc = service(ServiceType::SharedTagMapping, &[], &[]);
        let inp = input(&[], None);
        assert!(!should_run(&svc, &inp, &[EventLabel::Ingest]));
        assert!(should_run(&svc, &inp, &[EventLabel::IncomingShare]));
    }

    #[test]
    fn rule_service_fires_on_ingest_not_segmentation_edit() {
        let svc = service(ServiceType::Rule, &[], &[]);
        let inp = input(&[], None);
        assert!(should_run(&svc, &inp, &[EventLabel::Ingest]));
        assert!(!should_run(&svc, &inp, &[EventLabel::SegmentationEdit]));
    }

    #[test]
    fn segmentation_fires_on_segmentation_edit() {
        let svc = service(ServiceType::Segmentation, &[], &[]);
        let inp = input(&[], None);
        assert!(should_run(&svc, &inp, &[EventLabel::SegmentationEdit]));
    }

    // ── should_run: requires / excludes ──────────────────────────────────────

    #[test]
    fn requires_exact_tag_match() {
        let svc = service(ServiceType::Rule, &["Photos"], &[]);
        let inp_without = input(&["Images"], None);
        let inp_with = input(&["Photos"], None);
        assert!(!should_run(&svc, &inp_without, &[EventLabel::Ingest]));
        assert!(should_run(&svc, &inp_with, &[EventLabel::Ingest]));
    }

    #[test]
    fn requires_satisfied_by_ancestor() {
        // A picture with Photos.Travel.Alps satisfies requires: [Photos]
        let svc = service(ServiceType::Rule, &["Photos"], &[]);
        let inp = input(&["Photos.Travel.Alps"], None);
        assert!(should_run(&svc, &inp, &[EventLabel::Ingest]));
    }

    #[test]
    fn excludes_suppresses_service() {
        let svc = service(ServiceType::Rule, &[], &["Images"]);
        let inp_clean = input(&["Photos"], None);
        let inp_excluded = input(&["Images"], None);
        assert!(should_run(&svc, &inp_clean, &[EventLabel::Ingest]));
        assert!(!should_run(&svc, &inp_excluded, &[EventLabel::Ingest]));
    }

    #[test]
    fn excludes_triggered_by_ancestor() {
        // picture has Images.Icons; excludes Images → should not run
        let svc = service(ServiceType::Rule, &[], &["Images"]);
        let inp = input(&["Images.Icons"], None);
        assert!(!should_run(&svc, &inp, &[EventLabel::Ingest]));
    }

    #[test]
    fn all_requires_must_be_present() {
        let svc = service(ServiceType::Rule, &["Photos", "Travel"], &[]);
        let inp_partial = input(&["Photos"], None);
        let inp_full = input(&["Photos", "Travel"], None);
        assert!(!should_run(&svc, &inp_partial, &[EventLabel::Ingest]));
        assert!(should_run(&svc, &inp_full, &[EventLabel::Ingest]));
    }

    // ── evaluate_segmentation ─────────────────────────────────────────────────

    fn seg_rule(assign: &str, start: &str, end: &str) -> SegmentationRule {
        SegmentationRule {
            id: Uuid::new_v4(),
            service_id: Uuid::new_v4(),
            name: assign.to_string(),
            date_start: dt(start),
            date_end: dt(end),
            assign_tag: assign.to_string(),
            parent_segment_id: None,
        }
    }

    #[test]
    fn segmentation_assigns_tag_in_range() {
        let rules = vec![seg_rule(
            "Photos.Trip",
            "2024-08-01 00:00:00",
            "2024-08-14 23:59:59",
        )];
        let inp = input(&[], Some("2024-08-05 10:00:00"));
        let result = evaluate_segmentation(&rules, &inp);
        assert_eq!(result.tags_to_add, vec![TagPath::from_ltree("Photos.Trip")]);
    }

    #[test]
    fn segmentation_no_tag_outside_range() {
        let rules = vec![seg_rule(
            "Photos.Trip",
            "2024-08-01 00:00:00",
            "2024-08-14 23:59:59",
        )];
        let inp = input(&[], Some("2024-09-01 00:00:00"));
        let result = evaluate_segmentation(&rules, &inp);
        assert!(result.tags_to_add.is_empty());
    }

    #[test]
    fn segmentation_no_captured_at_produces_no_tags() {
        let rules = vec![seg_rule(
            "Photos.Trip",
            "2024-08-01 00:00:00",
            "2024-08-14 23:59:59",
        )];
        let inp = input(&[], None);
        let result = evaluate_segmentation(&rules, &inp);
        assert!(result.tags_to_add.is_empty());
    }

    #[test]
    fn segmentation_multiple_matching_rules_all_assigned() {
        let rules = vec![
            seg_rule(
                "Photos.Summer",
                "2024-06-01 00:00:00",
                "2024-08-31 23:59:59",
            ),
            seg_rule("Photos.Trip", "2024-08-01 00:00:00", "2024-08-14 23:59:59"),
        ];
        let inp = input(&[], Some("2024-08-05 10:00:00"));
        let result = evaluate_segmentation(&rules, &inp);
        assert_eq!(result.tags_to_add.len(), 2);
    }

    #[test]
    fn segmentation_boundary_inclusive() {
        let rules = vec![seg_rule(
            "Photos.Trip",
            "2024-08-01 00:00:00",
            "2024-08-14 23:59:59",
        )];
        // Start boundary
        let inp_start = input(&[], Some("2024-08-01 00:00:00"));
        assert!(
            !evaluate_segmentation(&rules, &inp_start)
                .tags_to_add
                .is_empty()
        );
        // End boundary
        let inp_end = input(&[], Some("2024-08-14 23:59:59"));
        assert!(
            !evaluate_segmentation(&rules, &inp_end)
                .tags_to_add
                .is_empty()
        );
    }

    // ── evaluate_shared_tag_mapping ───────────────────────────────────────────

    fn mapping_rule(incoming_share_id: Uuid, assign: &str, broken: bool) -> SharedTagMappingRule {
        SharedTagMappingRule {
            id: Uuid::new_v4(),
            service_id: Uuid::new_v4(),
            incoming_share_id,
            assign_tag: assign.to_string(),
            is_broken: broken,
        }
    }

    #[test]
    fn mapping_assigns_tag_for_matching_share() {
        let share_id = Uuid::new_v4();
        let rules = vec![mapping_rule(share_id, "Photos.Holidays.2024", false)];
        let result = evaluate_shared_tag_mapping(&rules, &[share_id]);
        assert_eq!(
            result.tags_to_add,
            vec![TagPath::from_ltree("Photos.Holidays.2024")]
        );
    }

    #[test]
    fn mapping_skips_non_matching_share() {
        let share_id = Uuid::new_v4();
        let other_id = Uuid::new_v4();
        let rules = vec![mapping_rule(share_id, "Photos.Holidays.2024", false)];
        let result = evaluate_shared_tag_mapping(&rules, &[other_id]);
        assert!(result.tags_to_add.is_empty());
    }

    #[test]
    fn mapping_skips_broken_rules() {
        let share_id = Uuid::new_v4();
        let rules = vec![mapping_rule(share_id, "Photos.Holidays.2024", true)];
        let result = evaluate_shared_tag_mapping(&rules, &[share_id]);
        assert!(result.tags_to_add.is_empty());
    }

    #[test]
    fn mapping_multiple_rules_only_matching_assigned() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let rules = vec![
            mapping_rule(id1, "Photos.Alice", false),
            mapping_rule(id2, "Photos.Bob", false),
        ];
        let result = evaluate_shared_tag_mapping(&rules, &[id1]);
        assert_eq!(
            result.tags_to_add,
            vec![TagPath::from_ltree("Photos.Alice")]
        );
    }
}
