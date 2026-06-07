mod common;

use archypix_back::infra::error::AppError;
use archypix_back::repository::tag::TagRepository;
use archypix_back::services::tags;
use sqlx::PgPool;
use uuid::Uuid;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[sqlx::test(migrator = "MIGRATOR")]
async fn edit_picture_tags_rejects_empty_picture_ids(db: PgPool) {
    let user_id = Uuid::new_v4();
    let result = tags::edit_picture_tags(&db, user_id, &[], &["vacation".to_string()], &[]).await;
    assert!(matches!(result, Err(AppError::BadRequest(_))));
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn edit_picture_tags_rejects_no_add_and_no_remove(db: PgPool) {
    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let pic_id = common::seed_picture(&db, alice_id).await;

    let result = tags::edit_picture_tags(&db, alice_id, &[pic_id], &[], &[]).await;
    assert!(matches!(result, Err(AppError::BadRequest(_))));
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn edit_picture_tags_add_is_applied(db: PgPool) {
    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let pic_id = common::seed_picture(&db, alice_id).await;

    tags::edit_picture_tags(&db, alice_id, &[pic_id], &["vacation".to_string()], &[])
        .await
        .unwrap();

    let stored = TagRepository::list_for_picture(&db, alice_id, pic_id)
        .await
        .unwrap();
    assert!(
        stored.iter().any(|t| t.tag_path == "vacation"),
        "tag must be present after add"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn edit_picture_tags_remove_is_applied(db: PgPool) {
    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let pic_id = common::seed_picture_with_tag(&db, alice_id, "vacation").await;

    tags::edit_picture_tags(&db, alice_id, &[pic_id], &[], &["vacation".to_string()])
        .await
        .unwrap();

    let stored = TagRepository::list_for_picture(&db, alice_id, pic_id)
        .await
        .unwrap();
    assert!(
        !stored.iter().any(|t| t.tag_path == "vacation"),
        "tag must be gone after remove"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn edit_picture_tags_add_and_remove_are_atomic(db: PgPool) {
    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let pic_id = common::seed_picture_with_tag(&db, alice_id, "old").await;

    tags::edit_picture_tags(
        &db,
        alice_id,
        &[pic_id],
        &["new".to_string()],
        &["old".to_string()],
    )
    .await
    .unwrap();

    let stored = TagRepository::list_for_picture(&db, alice_id, pic_id)
        .await
        .unwrap();
    let paths: Vec<&str> = stored.iter().map(|t| t.tag_path.as_str()).collect();
    assert!(paths.contains(&"new"), "new tag must be present");
    assert!(!paths.contains(&"old"), "old tag must be removed");
}
