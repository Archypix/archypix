mod common;

use archypix_back::domain::job::EditPictureConfig;
use archypix_back::infra::error::AppError;
use archypix_back::repository::picture::PictureRepository;
use archypix_back::services::jobs;
use sqlx::PgPool;
use uuid::Uuid;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

fn edit_config(picture_id: Uuid) -> EditPictureConfig {
    EditPictureConfig {
        picture_id,
        exif_overrides: None,
        visual: None,
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn list_picture_jobs_rejects_wrong_owner(db: PgPool) {
    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let bob_id = common::seed_user(&db, "bob", "pass").await;
    let pic_id = common::seed_picture(&db, alice_id).await;

    let result = jobs::list_picture_jobs(&db, pic_id, bob_id).await;
    assert!(
        matches!(result, Err(AppError::NotFound)),
        "bob must not see alice's picture jobs"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn list_picture_jobs_returns_empty_for_owner_with_no_jobs(db: PgPool) {
    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let pic_id = common::seed_picture(&db, alice_id).await;

    let result = jobs::list_picture_jobs(&db, pic_id, alice_id)
        .await
        .unwrap();
    assert!(result.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn enqueue_edit_rejects_received_picture(db: PgPool) {
    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let bob_id = common::seed_user(&db, "bob", "pass").await;
    let alice_pic_id = common::seed_picture(&db, alice_id).await;

    // Create a received picture for Bob that points at Alice's picture
    let received = PictureRepository::create_received(
        &db,
        bob_id,
        &alice_pic_id.to_string(),
        "alice",
        "test.com",
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();

    let result =
        jobs::enqueue_edit_for_user(&db, bob_id, received.id, edit_config(received.id)).await;
    assert!(
        matches!(result, Err(AppError::BadRequest(_))),
        "editing a received picture must return BadRequest"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn enqueue_edit_rejects_picture_not_owned_by_user(db: PgPool) {
    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let bob_id = common::seed_user(&db, "bob", "pass").await;
    let alice_pic_id = common::seed_picture(&db, alice_id).await;

    let result =
        jobs::enqueue_edit_for_user(&db, bob_id, alice_pic_id, edit_config(alice_pic_id)).await;
    assert!(
        matches!(result, Err(AppError::NotFound)),
        "bob must not enqueue edit for alice's picture"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn enqueue_edit_for_owned_picture_creates_job(db: PgPool) {
    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let pic_id = common::seed_picture(&db, alice_id).await;

    let job = jobs::enqueue_edit_for_user(&db, alice_id, pic_id, edit_config(pic_id))
        .await
        .unwrap();

    assert_eq!(job.owner_id, alice_id);
    assert_eq!(job.picture_id, Some(pic_id));
    assert_eq!(job.status, archypix_back::domain::job::JobStatus::Pending);
}
