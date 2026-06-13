use crate::domain::tag::TagPath;
use crate::domain::tagging::{SegmentationRule, SharedTagMappingRule, TaggingService};
use chrono::Datelike;
use chrono::NaiveDateTime;
use uuid::Uuid;

// ── Pipeline input ────────────────────────────────────────────────────────────

/// Input fed to the pipeline evaluator for a single picture.
#[derive(Debug, Clone)]
pub struct PipelineInput {
    pub picture_id: Uuid,
    pub captured_at: Option<NaiveDateTime>,
    pub gps_lat: Option<f64>,
    pub gps_lng: Option<f64>,
    pub filename: Option<String>,
    /// Tags currently assigned to this picture; updated in-memory as services run
    /// so later services in the pipeline see tags added by earlier ones.
    pub current_tags: Vec<TagPath>,
}

/// Tags to add as a result of evaluating one service against one picture.
#[derive(Debug, Clone, Default)]
pub struct ServiceResult {
    pub tags_to_add: Vec<TagPath>,
}

// ── should_run ────────────────────────────────────────────────────────────────

/// Return true if `service` should run for `input`.
///
/// Checks `enabled`, `requires` (all must match), and `excludes` (none must match).
/// Services are run for every dirty picture; label-based filtering is not needed
/// because the pipeline only runs pictures that need re-evaluation anyway.
pub fn should_run(service: &TaggingService, input: &PipelineInput) -> bool {
    if !service.enabled {
        return false;
    }
    // All required tags must be present (exact match or descendant).
    let satisfied = service.requires.iter().all(|req| {
        let req_path = TagPath::from_ltree(req);
        input
            .current_tags
            .iter()
            .any(|t| t == &req_path || t.ancestors().contains(&req_path))
    });
    // Any excluded tag suppresses this service.
    let excluded = service.excludes.iter().any(|exc| {
        let exc_path = TagPath::from_ltree(exc);
        input
            .current_tags
            .iter()
            .any(|t| t == &exc_path || t.ancestors().contains(&exc_path))
    });
    satisfied && !excluded
}

// ── Predicate (Rule service) ──────────────────────────────────────────────────

/// A parsed predicate for a `Rule` tagging service rule.
///
/// # Supported syntax
/// | Predicate | Example |
/// |---|---|
/// | GPS bounding box | `gps_within_bbox(45.0, 46.0, 4.0, 5.0)` |
/// | Capture year | `capture_year(2024)` |
/// | Capture month (1–12) | `capture_month(8)` |
/// | Filename substring (case-insensitive) | `filename_contains("vacation")` |
///
/// The format is intentionally simple and may be extended later; the predicate
/// string is stored as-is and re-parsed at evaluation time.
#[derive(Debug, Clone, PartialEq)]
pub enum Predicate {
    /// Picture GPS coordinates fall within [lat_min, lat_max] × [lon_min, lon_max].
    GpsWithinBbox {
        lat_min: f64,
        lat_max: f64,
        lon_min: f64,
        lon_max: f64,
    },
    /// `captured_at` year equals the given value.
    CaptureYear(i32),
    /// `captured_at` month equals the given value (1–12).
    CaptureMonth(u32),
    /// Filename contains the given substring (case-insensitive).
    FilenameContains(String),
}

impl Predicate {
    /// Parse a predicate string. Returns an error message if the syntax is invalid.
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();

        if let Some(inner) = s
            .strip_prefix("gps_within_bbox(")
            .and_then(|s| s.strip_suffix(')'))
        {
            let parts: Vec<f64> = inner
                .split(',')
                .map(|p| p.trim().parse::<f64>())
                .collect::<Result<_, _>>()
                .map_err(|_| {
                    format!("gps_within_bbox: expected 4 decimal numbers, got {inner:?}")
                })?;
            if parts.len() != 4 {
                return Err(format!(
                    "gps_within_bbox: expected 4 arguments (lat_min, lat_max, lon_min, lon_max), got {}",
                    parts.len()
                ));
            }
            if parts[0] > parts[1] {
                return Err("gps_within_bbox: lat_min must be ≤ lat_max".to_string());
            }
            if parts[2] > parts[3] {
                return Err("gps_within_bbox: lon_min must be ≤ lon_max".to_string());
            }
            return Ok(Predicate::GpsWithinBbox {
                lat_min: parts[0],
                lat_max: parts[1],
                lon_min: parts[2],
                lon_max: parts[3],
            });
        }

        if let Some(inner) = s
            .strip_prefix("capture_year(")
            .and_then(|s| s.strip_suffix(')'))
        {
            let year: i32 = inner
                .trim()
                .parse()
                .map_err(|_| format!("capture_year: expected an integer year, got {inner:?}"))?;
            return Ok(Predicate::CaptureYear(year));
        }

        if let Some(inner) = s
            .strip_prefix("capture_month(")
            .and_then(|s| s.strip_suffix(')'))
        {
            let month: u32 = inner
                .trim()
                .parse()
                .map_err(|_| format!("capture_month: expected an integer 1–12, got {inner:?}"))?;
            if !(1..=12).contains(&month) {
                return Err(format!("capture_month: month must be 1–12, got {month}"));
            }
            return Ok(Predicate::CaptureMonth(month));
        }

        if let Some(inner) = s
            .strip_prefix("filename_contains(\"")
            .and_then(|s| s.strip_suffix("\")"))
        {
            return Ok(Predicate::FilenameContains(inner.to_string()));
        }

        Err(format!(
            "unknown predicate {s:?} — supported: gps_within_bbox, capture_year, capture_month, filename_contains"
        ))
    }

    /// Evaluate the predicate against the picture input.
    pub fn matches(&self, input: &PipelineInput) -> bool {
        match self {
            Predicate::GpsWithinBbox {
                lat_min,
                lat_max,
                lon_min,
                lon_max,
            } => {
                if let (Some(lat), Some(lng)) = (input.gps_lat, input.gps_lng) {
                    lat >= *lat_min && lat <= *lat_max && lng >= *lon_min && lng <= *lon_max
                } else {
                    false
                }
            }
            Predicate::CaptureYear(year) => {
                input.captured_at.map_or(false, |dt| dt.year() == *year)
            }
            Predicate::CaptureMonth(month) => {
                input.captured_at.map_or(false, |dt| dt.month() == *month)
            }
            Predicate::FilenameContains(needle) => input.filename.as_ref().map_or(false, |f| {
                f.to_lowercase().contains(needle.to_lowercase().as_str())
            }),
        }
    }
}

// ── Evaluators ────────────────────────────────────────────────────────────────

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
    ServiceResult { tags_to_add }
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
    ServiceResult { tags_to_add }
}

/// Evaluate a rule service against an input — pure, no I/O.
///
/// Rules whose predicate fails to parse are silently skipped (the predicate is
/// validated at creation time, so this should not happen in practice).
pub fn evaluate_rule(
    rules: &[crate::domain::tagging::RuleTaggingRule],
    input: &PipelineInput,
) -> ServiceResult {
    let mut tags_to_add = Vec::new();
    for rule in rules {
        match Predicate::parse(&rule.predicate) {
            Ok(pred) if pred.matches(input) => {
                tags_to_add.push(TagPath::from_ltree(&rule.assign_tag));
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(
                    predicate = %rule.predicate,
                    error = %e,
                    "rule predicate failed to parse during evaluation — skipping"
                );
            }
        }
    }
    ServiceResult { tags_to_add }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tagging::{
        RuleTaggingRule, SegmentationRule, ServiceType, SharedTagMappingRule, TaggingService,
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
            position: 0,
            last_invalidated_at: dt("2024-01-01 00:00:00"),
            last_error_at: None,
            last_error_msg: None,
            created_at: dt("2024-01-01 00:00:00"),
            updated_at: dt("2024-01-01 00:00:00"),
        }
    }

    fn input(tags: &[&str], captured_at: Option<&str>) -> PipelineInput {
        PipelineInput {
            picture_id: Uuid::new_v4(),
            captured_at: captured_at.map(|s| dt(s)),
            gps_lat: None,
            gps_lng: None,
            filename: None,
            current_tags: tags.iter().map(|s| TagPath::from_ltree(*s)).collect(),
        }
    }

    fn input_with_gps(lat: f64, lng: f64) -> PipelineInput {
        PipelineInput {
            picture_id: Uuid::new_v4(),
            captured_at: None,
            gps_lat: Some(lat),
            gps_lng: Some(lng),
            filename: None,
            current_tags: vec![],
        }
    }

    // ── should_run ────────────────────────────────────────────────────────────

    #[test]
    fn disabled_service_never_runs() {
        let mut svc = service(ServiceType::Segmentation, &[], &[]);
        svc.enabled = false;
        assert!(!should_run(&svc, &input(&["Photos"], None)));
    }

    #[test]
    fn requires_exact_tag_match() {
        let svc = service(ServiceType::Rule, &["Photos"], &[]);
        assert!(!should_run(&svc, &input(&["Images"], None)));
        assert!(should_run(&svc, &input(&["Photos"], None)));
    }

    #[test]
    fn requires_satisfied_by_ancestor() {
        let svc = service(ServiceType::Rule, &["Photos"], &[]);
        assert!(should_run(&svc, &input(&["Photos.Travel.Alps"], None)));
    }

    #[test]
    fn excludes_suppresses_service() {
        let svc = service(ServiceType::Rule, &[], &["Images"]);
        assert!(should_run(&svc, &input(&["Photos"], None)));
        assert!(!should_run(&svc, &input(&["Images"], None)));
    }

    #[test]
    fn excludes_triggered_by_ancestor() {
        let svc = service(ServiceType::Rule, &[], &["Images"]);
        assert!(!should_run(&svc, &input(&["Images.Icons"], None)));
    }

    #[test]
    fn all_requires_must_be_present() {
        let svc = service(ServiceType::Rule, &["Photos", "Travel"], &[]);
        assert!(!should_run(&svc, &input(&["Photos"], None)));
        assert!(should_run(&svc, &input(&["Photos", "Travel"], None)));
    }

    // ── Predicate::parse ─────────────────────────────────────────────────────

    #[test]
    fn parse_gps_within_bbox() {
        let p = Predicate::parse("gps_within_bbox(45.0, 46.0, 4.0, 5.0)").unwrap();
        assert_eq!(
            p,
            Predicate::GpsWithinBbox {
                lat_min: 45.0,
                lat_max: 46.0,
                lon_min: 4.0,
                lon_max: 5.0
            }
        );
    }

    #[test]
    fn parse_capture_year() {
        assert_eq!(
            Predicate::parse("capture_year(2024)").unwrap(),
            Predicate::CaptureYear(2024)
        );
    }

    #[test]
    fn parse_capture_month() {
        assert_eq!(
            Predicate::parse("capture_month(8)").unwrap(),
            Predicate::CaptureMonth(8)
        );
    }

    #[test]
    fn parse_filename_contains() {
        assert_eq!(
            Predicate::parse("filename_contains(\"vacation\")").unwrap(),
            Predicate::FilenameContains("vacation".to_string())
        );
    }

    #[test]
    fn parse_rejects_invalid_month() {
        assert!(Predicate::parse("capture_month(13)").is_err());
        assert!(Predicate::parse("capture_month(0)").is_err());
    }

    #[test]
    fn parse_rejects_bbox_lat_min_gt_max() {
        assert!(Predicate::parse("gps_within_bbox(46.0, 45.0, 4.0, 5.0)").is_err());
    }

    #[test]
    fn parse_rejects_unknown_predicate() {
        assert!(Predicate::parse("unknown_predicate(42)").is_err());
    }

    // ── Predicate::matches ───────────────────────────────────────────────────

    #[test]
    fn gps_matches_inside_bbox() {
        let p = Predicate::GpsWithinBbox {
            lat_min: 45.0,
            lat_max: 46.0,
            lon_min: 4.0,
            lon_max: 5.0,
        };
        assert!(p.matches(&input_with_gps(45.5, 4.5)));
    }

    #[test]
    fn gps_rejects_outside_bbox() {
        let p = Predicate::GpsWithinBbox {
            lat_min: 45.0,
            lat_max: 46.0,
            lon_min: 4.0,
            lon_max: 5.0,
        };
        assert!(!p.matches(&input_with_gps(47.0, 4.5)));
    }

    #[test]
    fn gps_rejects_no_coords() {
        let p = Predicate::GpsWithinBbox {
            lat_min: 45.0,
            lat_max: 46.0,
            lon_min: 4.0,
            lon_max: 5.0,
        };
        assert!(!p.matches(&input(&[], None)));
    }

    #[test]
    fn capture_year_matches() {
        let p = Predicate::CaptureYear(2024);
        assert!(p.matches(&input(&[], Some("2024-08-05 10:00:00"))));
        assert!(!p.matches(&input(&[], Some("2023-08-05 10:00:00"))));
        assert!(!p.matches(&input(&[], None)));
    }

    #[test]
    fn capture_month_matches() {
        let p = Predicate::CaptureMonth(8);
        assert!(p.matches(&input(&[], Some("2024-08-05 10:00:00"))));
        assert!(!p.matches(&input(&[], Some("2024-09-05 10:00:00"))));
    }

    #[test]
    fn filename_contains_case_insensitive() {
        let p = Predicate::FilenameContains("vacation".to_string());
        let mut inp = input(&[], None);
        inp.filename = Some("Summer_Vacation_2024.jpg".to_string());
        assert!(p.matches(&inp));
        inp.filename = Some("birthday.jpg".to_string());
        assert!(!p.matches(&inp));
        inp.filename = None;
        assert!(!p.matches(&inp));
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
        let result = evaluate_segmentation(&rules, &input(&[], Some("2024-08-05 10:00:00")));
        assert_eq!(result.tags_to_add, vec![TagPath::from_ltree("Photos.Trip")]);
    }

    #[test]
    fn segmentation_no_tag_outside_range() {
        let rules = vec![seg_rule(
            "Photos.Trip",
            "2024-08-01 00:00:00",
            "2024-08-14 23:59:59",
        )];
        let result = evaluate_segmentation(&rules, &input(&[], Some("2024-09-01 00:00:00")));
        assert!(result.tags_to_add.is_empty());
    }

    #[test]
    fn segmentation_no_captured_at_produces_no_tags() {
        let rules = vec![seg_rule(
            "Photos.Trip",
            "2024-08-01 00:00:00",
            "2024-08-14 23:59:59",
        )];
        let result = evaluate_segmentation(&rules, &input(&[], None));
        assert!(result.tags_to_add.is_empty());
    }

    #[test]
    fn segmentation_boundary_inclusive() {
        let rules = vec![seg_rule(
            "Photos.Trip",
            "2024-08-01 00:00:00",
            "2024-08-14 23:59:59",
        )];
        assert!(
            !evaluate_segmentation(&rules, &input(&[], Some("2024-08-01 00:00:00")))
                .tags_to_add
                .is_empty()
        );
        assert!(
            !evaluate_segmentation(&rules, &input(&[], Some("2024-08-14 23:59:59")))
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
        let rules = vec![mapping_rule(share_id, "Photos.Holidays.2024", false)];
        let result = evaluate_shared_tag_mapping(&rules, &[Uuid::new_v4()]);
        assert!(result.tags_to_add.is_empty());
    }

    #[test]
    fn mapping_skips_broken_rules() {
        let share_id = Uuid::new_v4();
        let rules = vec![mapping_rule(share_id, "Photos.Holidays.2024", true)];
        let result = evaluate_shared_tag_mapping(&rules, &[share_id]);
        assert!(result.tags_to_add.is_empty());
    }

    // ── evaluate_rule ─────────────────────────────────────────────────────────

    fn rule(predicate: &str, assign: &str) -> RuleTaggingRule {
        RuleTaggingRule {
            id: Uuid::new_v4(),
            service_id: Uuid::new_v4(),
            predicate: predicate.to_string(),
            assign_tag: assign.to_string(),
        }
    }

    #[test]
    fn evaluate_rule_gps_match() {
        let rules = vec![rule("gps_within_bbox(45.0, 46.0, 4.0, 5.0)", "Photos.Alps")];
        let result = evaluate_rule(&rules, &input_with_gps(45.5, 4.5));
        assert_eq!(result.tags_to_add, vec![TagPath::from_ltree("Photos.Alps")]);
    }

    #[test]
    fn evaluate_rule_gps_no_match() {
        let rules = vec![rule("gps_within_bbox(45.0, 46.0, 4.0, 5.0)", "Photos.Alps")];
        let result = evaluate_rule(&rules, &input_with_gps(48.0, 2.0));
        assert!(result.tags_to_add.is_empty());
    }

    #[test]
    fn evaluate_rule_skips_bad_predicate() {
        let rules = vec![rule("not_a_valid_predicate()", "Photos.Bad")];
        let result = evaluate_rule(&rules, &input(&[], None));
        assert!(result.tags_to_add.is_empty());
    }

    // ── ancestor satisfaction ─────────────────────────────────────────────────

    #[test]
    fn deep_tag_satisfies_ancestor_require() {
        let svc = service(ServiceType::Rule, &["Photos"], &[]);
        assert!(should_run(&svc, &input(&["Photos.Travel.Alps"], None)));
    }
}
