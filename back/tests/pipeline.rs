//! End-to-end tagging-pipeline tests: live re-derivation, always-on removal, and the
//! service-lifecycle tag handling (promotion on delete, removal on disable).

mod common;

use archypix_back::domain::tag::TagSource;
use archypix_back::domain::tagging::ServiceType;
use archypix_back::infra::config::Config;
use archypix_back::infra::pipeline;
use archypix_back::repository::tag::TagRepository;
use archypix_back::repository::tagging::{RuleTaggingRuleRepository, TaggingServiceRepository};
use archypix_back::services;
use sqlx::PgPool;
use uuid::Uuid;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Run the pipeline once for `user` with throwaway deps + test config.
async fn run_pipeline(db: &PgPool, user: Uuid) {
    let config = Config::test_defaults();
    let (fed, cache) = common::make_federation(&config);
    let waker = pipeline::PipelineWaker::disconnected();
    pipeline::run_once_for_user(db, &fed, cache.as_ref(), &config, &waker, user)
        .await
        .unwrap();
}

/// Insert a picture captured in 2024 so `capture_year(2024)` rules match it.
async fn seed_picture_2024(db: &PgPool, user_id: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query!(
        "INSERT INTO pictures (id, local_user_id, captured_at) \
         VALUES ($1, $2, '2024-06-01 12:00:00')",
        id,
        user_id,
    )
    .execute(db)
    .await
    .unwrap();
    id
}

/// Create a Rule service with a single `capture_year(2024)` rule assigning `tag`.
async fn seed_rule_service(db: &PgPool, owner: Uuid, tag: &str) -> Uuid {
    let svc = TaggingServiceRepository::create(db, owner, ServiceType::Rule, &[], &[])
        .await
        .unwrap();
    RuleTaggingRuleRepository::create(db, svc.id, "capture_year(2024)", tag)
        .await
        .unwrap();
    svc.id
}

fn has_tag(tags: &[archypix_back::domain::tag::Tag], path: &str) -> bool {
    tags.iter().any(|t| t.tag_path == path)
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn pipeline_assigns_matching_rule_tag(db: PgPool) {
    let user = common::seed_user(&db, "alice", "pass").await;
    let pic = seed_picture_2024(&db, user).await;
    seed_rule_service(&db, user, "Photos.Y2024").await;

    run_pipeline(&db, user).await;

    let tags = TagRepository::list_for_picture(&db, user, pic)
        .await
        .unwrap();
    let tag = tags
        .iter()
        .find(|t| t.tag_path == "Photos.Y2024")
        .expect("rule tag assigned");
    assert_eq!(tag.source, TagSource::Rule);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn pipeline_removes_tag_when_rule_no_longer_produces_it(db: PgPool) {
    let user = common::seed_user(&db, "alice", "pass").await;
    let pic = seed_picture_2024(&db, user).await;
    let svc = seed_rule_service(&db, user, "Photos.Y2024").await;

    run_pipeline(&db, user).await;
    assert!(has_tag(
        &TagRepository::list_for_picture(&db, user, pic)
            .await
            .unwrap(),
        "Photos.Y2024"
    ));

    // Drop the rule and re-invalidate — the service now produces nothing.
    let rules = RuleTaggingRuleRepository::list_for_services(&db, &[svc])
        .await
        .unwrap();
    RuleTaggingRuleRepository::delete(&db, user, svc, rules[0].id)
        .await
        .unwrap();
    TaggingServiceRepository::touch_invalidated(&db, svc)
        .await
        .unwrap();

    run_pipeline(&db, user).await;

    assert!(
        !has_tag(
            &TagRepository::list_for_picture(&db, user, pic)
                .await
                .unwrap(),
            "Photos.Y2024"
        ),
        "stale pipeline tag removed"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn pipeline_leaves_manual_tags_untouched(db: PgPool) {
    let user = common::seed_user(&db, "alice", "pass").await;
    let pic = seed_picture_2024(&db, user).await;
    let svc = seed_rule_service(&db, user, "Photos.Y2024").await;
    TagRepository::batch_assign(&db, user, &[pic], &["My.Manual".to_string()])
        .await
        .unwrap();

    run_pipeline(&db, user).await;

    // Disable the service → its tags go, manual survives.
    TaggingServiceRepository::update(&db, user, svc, Some(false), None, None)
        .await
        .unwrap();
    TagRepository::remove_service_tags(&db, svc).await.unwrap();

    let tags = TagRepository::list_for_picture(&db, user, pic)
        .await
        .unwrap();
    assert!(
        !has_tag(&tags, "Photos.Y2024"),
        "disabled service tag removed"
    );
    assert!(has_tag(&tags, "My.Manual"), "manual tag kept");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn deleting_service_promotes_its_tags_to_manual(db: PgPool) {
    let user = common::seed_user(&db, "alice", "pass").await;
    let pic = seed_picture_2024(&db, user).await;
    let svc = seed_rule_service(&db, user, "Photos.Y2024").await;

    run_pipeline(&db, user).await;

    let deleted = services::tagging::delete_service(&db, user, svc, true)
        .await
        .unwrap();
    assert!(deleted);

    let tags = TagRepository::list_for_picture(&db, user, pic)
        .await
        .unwrap();
    let tag = tags
        .iter()
        .find(|t| t.tag_path == "Photos.Y2024")
        .expect("promoted tag still present");
    assert_eq!(tag.source, TagSource::Manual);
    assert!(tag.source_id.is_none());
}
