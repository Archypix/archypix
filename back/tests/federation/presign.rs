//! Presign endpoint tests.
//!
//! `POST /api/federation/pictures/presign` is called by the recipient's client
//! directly — it is authorised by `share_token`, not a federation JWT.
//! A single in-process server via `oneshot` is sufficient.

use crate::common;
use crate::{body_json, cfg_a, post_no_auth};

use archypix_back::domain::share::ShareStatus;
use archypix_back::repository::share::OutgoingShareRepository;
use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

#[sqlx::test(migrator = "MIGRATOR")]
async fn presign_valid_token_returns_mock_urls(db: PgPool) {
    let cfg = cfg_a();
    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let pic_id = common::seed_picture(&db, alice_id).await;

    let share =
        OutgoingShareRepository::create(&db, alice_id, "vacation", "bob", "b.test", true, false)
            .await
            .unwrap();
    OutgoingShareRepository::set_status(&db, share.id, ShareStatus::Active)
        .await
        .unwrap();

    let app = archypix_back::api::routes(&cfg).with_state(common::test_app_state(db.clone(), &cfg));

    let resp = app
        .oneshot(post_no_auth(
            "/api/federation/pictures/presign",
            &json!({
                "share_token":    share.share_token,
                "owner_username": "alice",
                "owner_instance": "a.test",
                "pictures": [{ "picture_id": pic_id.to_string(), "variant": "original" }]
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let urls = body["urls"].as_array().unwrap();
    assert_eq!(urls.len(), 1);
    assert_eq!(urls[0]["picture_id"].as_str().unwrap(), pic_id.to_string());
    assert!(
        urls[0]["url"]
            .as_str()
            .unwrap()
            .starts_with("http://mock-s3"),
        "expected MockStorage URL, got: {}",
        urls[0]["url"]
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn presign_invalid_token_returns_unauthorized(db: PgPool) {
    let cfg = cfg_a();
    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let pic_id = common::seed_picture(&db, alice_id).await;

    let app = archypix_back::api::routes(&cfg).with_state(common::test_app_state(db.clone(), &cfg));

    let resp = app
        .oneshot(post_no_auth(
            "/api/federation/pictures/presign",
            &json!({
                "share_token":    Uuid::new_v4(),   // random — not in DB
                "owner_username": "alice",
                "owner_instance": "a.test",
                "pictures": [{ "picture_id": pic_id.to_string(), "variant": "original" }]
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
