//! Federation handler tests — Option B.
//!
//! Each test drives a **single** in-process Axum router via `tower::ServiceExt::oneshot`.
//! No second server or real network is needed because every handler tested here is
//! purely receiver-side: it writes to the DB and returns a response without making
//! outbound HTTP calls.
//!
//! Covered handlers:
//!   POST /api/federation/shares/announce   — receive an inbound share
//!   POST /api/federation/shares/revoke     — receive a revocation
//!   POST /api/federation/shares/reject     — receive a rejection (on the sender's server)
//!   POST /api/federation/shares/accept     — receive an acceptance (empty tag → no pictures call)
//!   POST /api/federation/pictures/announce — receive a pictures announcement
//!   POST /api/federation/pictures/presign  — presign owned pictures via share_token
//!
//! Security invariants verified:
//!   • wrong `recipient_instance` → 400
//!   • JWT sub ≠ sender/recipient instance → 401
//!   • unknown resource → 404

mod common;

use archypix_back::domain::share::ShareStatus;
use archypix_back::infra::config::Config;
use archypix_back::repository::share::{IncomingShareRepository, OutgoingShareRepository};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

// ── Config helpers ────────────────────────────────────────────────────────────

/// Server B — the recipient of shares from A.
fn cfg_b() -> Config {
    Config {
        global_domain: "b.test".to_string(),
        back_domain: "backend-b.test".to_string(),
        ..Config::test_defaults()
    }
}

/// Server A — the sender/owner of shares to B.
fn cfg_a() -> Config {
    Config {
        global_domain: "a.test".to_string(),
        back_domain: "backend-a.test".to_string(),
        ..Config::test_defaults()
    }
}

// ── Request builders ──────────────────────────────────────────────────────────

fn post_fed(path: &str, bearer: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

fn post_no_auth(path: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

// ── /api/federation/shares/announce ──────────────────────────────────────────

/// Valid announce: sender "alice@a.test" → recipient "bob@b.test".
/// Expecting IncomingShare to be created in Bob's DB.
#[sqlx::test(migrator = "MIGRATOR")]
async fn announce_share_valid_creates_incoming_share(db: PgPool) {
    let cfg = cfg_b();
    let bob_id = common::seed_user(&db, "bob", "pass").await;
    let outgoing_id = Uuid::new_v4();

    let token = common::federation::federation_jwt(&cfg, "a.test");
    let app = archypix_back::api::routes(&cfg).with_state(common::test_app_state(db.clone(), &cfg));

    let resp = app
        .oneshot(post_fed(
            "/api/federation/shares/announce",
            &token,
            &json!({
                "sender_username":    "alice",
                "sender_instance":    "a.test",
                "recipient_username": "bob",
                "recipient_instance": "b.test",
                "outgoing_share_id":  outgoing_id,
                "tag_path":           "vacation",
                "allow_share_back":   false,
                "future":             false,
                "shareback_of":       null,
                "share_token":        Uuid::new_v4()
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["accepted"], json!(true));

    // IncomingShare must exist for Bob.
    let shares = IncomingShareRepository::list_by_recipient(&db, bob_id)
        .await
        .unwrap();
    assert_eq!(shares.len(), 1);
    assert_eq!(shares[0].sender_username, "alice");
    assert_eq!(shares[0].sender_instance, "a.test");
    assert_eq!(shares[0].outgoing_share_id, outgoing_id);
    assert_eq!(shares[0].status, ShareStatus::Pending);
}

/// Wrong `recipient_instance` (not this server's `global_domain`) → 400.
#[sqlx::test(migrator = "MIGRATOR")]
async fn announce_share_rejects_wrong_recipient_instance(db: PgPool) {
    let cfg = cfg_b();
    common::seed_user(&db, "bob", "pass").await;
    let token = common::federation::federation_jwt(&cfg, "a.test");
    let app = archypix_back::api::routes(&cfg).with_state(common::test_app_state(db.clone(), &cfg));

    let resp = app
        .oneshot(post_fed(
            "/api/federation/shares/announce",
            &token,
            &json!({
                "sender_username":    "alice",
                "sender_instance":    "a.test",
                "recipient_username": "bob",
                "recipient_instance": "wrong.com",   // ← not this server
                "outgoing_share_id":  Uuid::new_v4(),
                "tag_path":           "vacation",
                "allow_share_back":   false,
                "future":             false,
                "shareback_of":       null,
                "share_token":        Uuid::new_v4()
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// JWT `sub` ("c.test") ≠ `sender_instance` ("a.test") → 401.
#[sqlx::test(migrator = "MIGRATOR")]
async fn announce_share_rejects_sender_instance_mismatch(db: PgPool) {
    let cfg = cfg_b();
    common::seed_user(&db, "bob", "pass").await;
    // JWT issued for "c.test" but payload claims sender is "a.test".
    let token = common::federation::federation_jwt(&cfg, "c.test");
    let app = archypix_back::api::routes(&cfg).with_state(common::test_app_state(db.clone(), &cfg));

    let resp = app
        .oneshot(post_fed(
            "/api/federation/shares/announce",
            &token,
            &json!({
                "sender_username":    "alice",
                "sender_instance":    "a.test",   // ← mismatch with JWT sub
                "recipient_username": "bob",
                "recipient_instance": "b.test",
                "outgoing_share_id":  Uuid::new_v4(),
                "tag_path":           "vacation",
                "allow_share_back":   false,
                "future":             false,
                "shareback_of":       null,
                "share_token":        Uuid::new_v4()
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// Recipient not in DB → 404.
#[sqlx::test(migrator = "MIGRATOR")]
async fn announce_share_rejects_unknown_recipient(db: PgPool) {
    let cfg = cfg_b();
    let token = common::federation::federation_jwt(&cfg, "a.test");
    let app = archypix_back::api::routes(&cfg).with_state(common::test_app_state(db.clone(), &cfg));

    let resp = app
        .oneshot(post_fed(
            "/api/federation/shares/announce",
            &token,
            &json!({
                "sender_username":    "alice",
                "sender_instance":    "a.test",
                "recipient_username": "nobody",   // ← not in DB
                "recipient_instance": "b.test",
                "outgoing_share_id":  Uuid::new_v4(),
                "tag_path":           "vacation",
                "allow_share_back":   false,
                "future":             false,
                "shareback_of":       null,
                "share_token":        Uuid::new_v4()
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── /api/federation/shares/revoke ────────────────────────────────────────────

/// Revoke a pending IncomingShare → status becomes Revoked.
#[sqlx::test(migrator = "MIGRATOR")]
async fn revoke_share_transitions_incoming_to_revoked(db: PgPool) {
    let cfg = cfg_b();
    let bob_id = common::seed_user(&db, "bob", "pass").await;
    let outgoing_id = Uuid::new_v4();

    // Create a pending IncomingShare as if Alice had announced it.
    let incoming = IncomingShareRepository::create(
        &db,
        bob_id,
        "alice",
        "a.test",
        outgoing_id,
        Some(Uuid::new_v4()),
    )
    .await
    .unwrap();

    let token = common::federation::federation_jwt(&cfg, "a.test");
    let app = archypix_back::api::routes(&cfg).with_state(common::test_app_state(db.clone(), &cfg));

    let resp = app
        .oneshot(post_fed(
            "/api/federation/shares/revoke",
            &token,
            &json!({ "outgoing_share_id": outgoing_id }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["revoked"], json!(true));

    // IncomingShare must be Revoked.
    let updated = IncomingShareRepository::get_by_id(&db, incoming.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.status, ShareStatus::Revoked);
}

/// Revoke with unknown `outgoing_share_id` → 404.
#[sqlx::test(migrator = "MIGRATOR")]
async fn revoke_share_not_found_for_unknown_id(db: PgPool) {
    let cfg = cfg_b();
    let token = common::federation::federation_jwt(&cfg, "a.test");
    let app = archypix_back::api::routes(&cfg).with_state(common::test_app_state(db.clone(), &cfg));

    let resp = app
        .oneshot(post_fed(
            "/api/federation/shares/revoke",
            &token,
            &json!({ "outgoing_share_id": Uuid::new_v4() }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── /api/federation/shares/reject ────────────────────────────────────────────
//
// This endpoint lives on the SENDER's server (A). Bob (on B) sends a rejection
// to Alice's server (A) saying he doesn't want the share.

/// Valid rejection of a pending OutgoingShare → OutgoingShare becomes Tombstoned.
#[sqlx::test(migrator = "MIGRATOR")]
async fn reject_share_pending_tombstones_outgoing(db: PgPool) {
    let cfg = cfg_a(); // Alice's server
    let alice_id = common::seed_user(&db, "alice", "pass").await;

    let share =
        OutgoingShareRepository::create(&db, alice_id, "vacation", "bob", "b.test", true, false)
            .await
            .unwrap();

    // JWT from B: Bob's instance is the authenticated caller.
    let token = common::federation::federation_jwt(&cfg, "b.test");
    let app = archypix_back::api::routes(&cfg).with_state(common::test_app_state(db.clone(), &cfg));

    let resp = app
        .oneshot(post_fed(
            "/api/federation/shares/reject",
            &token,
            &json!({ "outgoing_share_id": share.id }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["rejected"], json!(true));

    let updated = OutgoingShareRepository::get_by_id(&db, share.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.status, ShareStatus::Tombstoned);
}

/// JWT sub ("c.test") ≠ share.recipient_instance ("b.test") → 401.
#[sqlx::test(migrator = "MIGRATOR")]
async fn reject_share_rejects_instance_mismatch(db: PgPool) {
    let cfg = cfg_a();
    let alice_id = common::seed_user(&db, "alice", "pass").await;

    let share =
        OutgoingShareRepository::create(&db, alice_id, "vacation", "bob", "b.test", true, false)
            .await
            .unwrap();

    // JWT from "c.test" but the share's recipient is "b.test".
    let token = common::federation::federation_jwt(&cfg, "c.test");
    let app = archypix_back::api::routes(&cfg).with_state(common::test_app_state(db.clone(), &cfg));

    let resp = app
        .oneshot(post_fed(
            "/api/federation/shares/reject",
            &token,
            &json!({ "outgoing_share_id": share.id }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── /api/federation/shares/accept ────────────────────────────────────────────
//
// This endpoint lives on the SENDER's server (A). Bob accepts Alice's share, so
// Alice's server transitions the OutgoingShare to Active and would announce
// pictures. We use an empty tag to avoid triggering an outbound HTTP call.

/// Accept a share with an empty tag → OutgoingShare Active, 0 pictures announced.
#[sqlx::test(migrator = "MIGRATOR")]
async fn accept_share_no_pictures_activates_outgoing(db: PgPool) {
    let cfg = cfg_a();
    let alice_id = common::seed_user(&db, "alice", "pass").await;

    // "party" tag has no pictures in DB.
    let share =
        OutgoingShareRepository::create(&db, alice_id, "party", "bob", "b.test", true, false)
            .await
            .unwrap();

    let token = common::federation::federation_jwt(&cfg, "b.test");
    let app = archypix_back::api::routes(&cfg).with_state(common::test_app_state(db.clone(), &cfg));

    let resp = app
        .oneshot(post_fed(
            "/api/federation/shares/accept",
            &token,
            &json!({ "outgoing_share_id": share.id }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["announced"], json!(0));

    let updated = OutgoingShareRepository::get_by_id(&db, share.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.status, ShareStatus::Active);
}

/// JWT sub ("c.test") ≠ share.recipient_instance ("b.test") → 401.
#[sqlx::test(migrator = "MIGRATOR")]
async fn accept_share_rejects_instance_mismatch(db: PgPool) {
    let cfg = cfg_a();
    let alice_id = common::seed_user(&db, "alice", "pass").await;

    let share =
        OutgoingShareRepository::create(&db, alice_id, "vacation", "bob", "b.test", true, false)
            .await
            .unwrap();

    let token = common::federation::federation_jwt(&cfg, "c.test");
    let app = archypix_back::api::routes(&cfg).with_state(common::test_app_state(db.clone(), &cfg));

    let resp = app
        .oneshot(post_fed(
            "/api/federation/shares/accept",
            &token,
            &json!({ "outgoing_share_id": share.id }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── /api/federation/pictures/announce ────────────────────────────────────────

/// Pictures announcement on an active IncomingShare → received pictures created
/// with the correct `SharedToMe.<sender>.<tag>` ltree path.
#[sqlx::test(migrator = "MIGRATOR")]
async fn announce_pictures_registers_received_with_sharedtome_tags(db: PgPool) {
    let cfg = cfg_b();
    let bob_id = common::seed_user(&db, "bob", "pass").await;
    let outgoing_id = Uuid::new_v4();
    let remote_pic_id = Uuid::new_v4().to_string();

    // Seed an Active IncomingShare (no real OutgoingShare needed in DB).
    let incoming = IncomingShareRepository::create(
        &db,
        bob_id,
        "alice",
        "a.test",
        outgoing_id,
        Some(Uuid::new_v4()),
    )
    .await
    .unwrap();
    IncomingShareRepository::set_status(&db, incoming.id, ShareStatus::Active)
        .await
        .unwrap();

    let token = common::federation::federation_jwt(&cfg, "a.test");
    let app = archypix_back::api::routes(&cfg).with_state(common::test_app_state(db.clone(), &cfg));

    let resp = app
        .oneshot(post_fed(
            "/api/federation/pictures/announce",
            &token,
            &json!({
                "outgoing_share_id": outgoing_id,
                "tag_path":          "vacation",
                "sender_username":   "alice",
                "sender_instance":   "a.test",
                "pictures": [{
                    "picture_id":          remote_pic_id,
                    "owner_username":      "alice",
                    "owner_instance_domain": "a.test",
                    "filename":            null,
                    "mime_type":           null,
                    "file_size":           null,
                    "width":               null,
                    "height":              null,
                    "captured_at":         null
                }]
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["registered"], json!(1));

    // Bob must have 1 received picture with the expected SharedToMe tag.
    assert_eq!(common::count_received_pictures(&db, bob_id).await, 1);
    let tags = common::received_picture_tags(&db, bob_id).await;
    let expected = common::shared_to_me_tag("alice", "a.test", "vacation");
    assert!(
        tags.contains(&expected),
        "expected SharedToMe tag '{expected}', got: {tags:?}"
    );
}

/// Pictures announcement on a Pending (not yet Active) share → 404.
#[sqlx::test(migrator = "MIGRATOR")]
async fn announce_pictures_rejects_pending_share(db: PgPool) {
    let cfg = cfg_b();
    let bob_id = common::seed_user(&db, "bob", "pass").await;
    let outgoing_id = Uuid::new_v4();

    // Share is Pending — pictures must be rejected until Bob accepts.
    IncomingShareRepository::create(
        &db,
        bob_id,
        "alice",
        "a.test",
        outgoing_id,
        Some(Uuid::new_v4()),
    )
    .await
    .unwrap();

    let token = common::federation::federation_jwt(&cfg, "a.test");
    let app = archypix_back::api::routes(&cfg).with_state(common::test_app_state(db.clone(), &cfg));

    let resp = app
        .oneshot(post_fed(
            "/api/federation/pictures/announce",
            &token,
            &json!({
                "outgoing_share_id": outgoing_id,
                "tag_path":          "vacation",
                "sender_username":   "alice",
                "sender_instance":   "a.test",
                "pictures": [{
                    "picture_id":          Uuid::new_v4().to_string(),
                    "owner_username":      "alice",
                    "owner_instance_domain": "a.test",
                    "filename": null, "mime_type": null,
                    "file_size": null, "width": null,
                    "height": null, "captured_at": null
                }]
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── /api/federation/pictures/presign ─────────────────────────────────────────
//
// No federation JWT required — authorised by `share_token` alone.

/// Valid share_token for Alice's owned picture → MockStorage presigned URLs returned.
#[sqlx::test(migrator = "MIGRATOR")]
async fn presign_valid_token_returns_mock_urls(db: PgPool) {
    let cfg = cfg_a();
    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let pic_id = common::seed_picture(&db, alice_id).await;

    // Create an Active outgoing share so has_active_share_for_token returns true.
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
                "pictures": [{"picture_id": pic_id.to_string(), "variant": "original"}]
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

/// Invalid (random) share_token → 401.
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
                "pictures": [{"picture_id": pic_id.to_string(), "variant": "original"}]
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
