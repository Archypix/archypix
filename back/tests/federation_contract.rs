//! Federation end-to-end contract tests — Option A.
//!
//! Each test spins up **two full Axum servers** bound to OS-assigned ports and
//! drives cross-instance federation over real TCP using `reqwest`.  A shared
//! `PgPool` (from `#[sqlx::test]`) serves both backends; data is logically
//! partitioned by `global_domain` (`"a.test"` vs `"b.test"`), so the same DB
//! correctly models two separate instances.
//!
//! WebFinger resolution is bypassed by pre-seeding each server's `InMemoryCache`
//! with the other server's `backend_url`, removing the need for a real resolver.
//!
//! Protocol flows covered:
//!   1. Auth handshake          — B grants a federation JWT back to A via the
//!                                auth/request → auth/grant callback.
//!   2. Share announce + accept — Alice creates a share; Bob accepts; Alice
//!                                announces pictures; Bob receives them.
//!   3. Revoke                  — Alice revokes; Bob's received pictures disappear.
//!   4. Reject                  — Bob rejects a pending share; Alice's
//!                                OutgoingShare is tombstoned.

mod common;

use archypix_back::domain::share::ShareStatus;
use archypix_back::infra::config::Config;
use archypix_back::repository::share::{IncomingShareRepository, OutgoingShareRepository};
use reqwest::Client;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::net::SocketAddr;
use uuid::Uuid;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

// ── HTTP helpers ──────────────────────────────────────────────────────────────

async fn post(
    client: &Client,
    addr: SocketAddr,
    path: &str,
    bearer: Option<&str>,
    body: &Value,
) -> reqwest::Response {
    let mut req = client
        .post(format!("http://{addr}{path}"))
        .header("Content-Type", "application/json")
        .json(body);
    if let Some(token) = bearer {
        req = req.bearer_auth(token);
    }
    req.send().await.expect("HTTP POST failed")
}

// ── Shared setup ──────────────────────────────────────────────────────────────

/// Spawn backend A (alice@a.test) and B (bob@b.test) and wire their backend-URL
/// caches so that cross-instance WebFinger lookups are bypassed.
///
/// Returns `(addr_a, addr_b, config_a, config_b, alice_id, bob_id)`.
async fn spawn_pair(db: PgPool) -> (SocketAddr, SocketAddr, Config, Config, Uuid, Uuid) {
    let (addr_a, cache_a, cfg_a) =
        common::federation::spawn_backend(db.clone(), common::federation::config_a()).await;
    let (addr_b, cache_b, cfg_b) =
        common::federation::spawn_backend(db.clone(), common::federation::config_b()).await;

    let url_a = format!("http://{addr_a}");
    let url_b = format!("http://{addr_b}");

    // A needs to know where B is to send auth/request and share announcements.
    common::federation::seed_backend_url(&cache_a, "bob", "b.test", &url_b).await;
    // B needs to know where A is to send the auth/grant callback and accept responses.
    common::federation::seed_backend_url(&cache_b, "alice", "a.test", &url_a).await;

    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let bob_id = common::seed_user(&db, "bob", "pass").await;

    (addr_a, addr_b, cfg_a, cfg_b, alice_id, bob_id)
}

// ── Test 1: auth handshake ────────────────────────────────────────────────────

/// POST to B's `/api/federation/auth/request` as "a.test".
/// B issues a JWT and calls back to A's `/api/federation/auth/grant`.
/// After the round-trip, A's cache must hold `FederationToken("b.test")`.
#[sqlx::test(migrator = "MIGRATOR")]
async fn auth_handshake_grants_token_to_requester(db: PgPool) {
    let (_, addr_b, _, _, _, _) = spawn_pair(db.clone()).await;
    let client = Client::new();

    // POST auth/request to B — no bearer token required on this endpoint.
    // B issues a federation JWT and immediately calls back to A's auth/grant endpoint.
    // The `{"accepted": true}` response proves the round-trip succeeded:
    // if A's auth/grant were unreachable, B's handler would propagate an error.
    let resp = post(
        &client,
        addr_b,
        "/api/federation/auth/request",
        None,
        &json!({
            "requester_instance": "a.test",
            "username":           "alice",
            "scope":              "federation",
            "nonce":              Uuid::new_v4().to_string()
        }),
    )
    .await;

    assert_eq!(resp.status(), 200, "auth/request must succeed");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["accepted"], json!(true));

    // Second request with fresh nonce: confirms B can make a second successful
    // round-trip to A, i.e., A's grant endpoint is stably reachable.
    let resp2 = post(
        &client,
        addr_b,
        "/api/federation/auth/request",
        None,
        &json!({
            "requester_instance": "a.test",
            "username":           "alice",
            "scope":              "federation",
            "nonce":              Uuid::new_v4().to_string()
        }),
    )
    .await;
    assert_eq!(
        resp2.status(),
        200,
        "second auth/request must also succeed — A's grant endpoint is reachable"
    );
}

// ── Test 2: full announce + accept + pictures ─────────────────────────────────

/// Complete cross-instance share lifecycle:
///   Alice creates share → B gets IncomingShare (Pending)
///   Bob accepts         → A marks OutgoingShare Active + announces Alice's pictures
///   Bob's DB            → 1 received picture with SharedToMe tag
#[sqlx::test(migrator = "MIGRATOR")]
async fn cross_instance_share_announce_and_accept_propagates_pictures(db: PgPool) {
    let (addr_a, addr_b, cfg_a, cfg_b, alice_id, bob_id) = spawn_pair(db.clone()).await;
    let client = Client::new();

    // Seed a picture for Alice tagged "vacation".
    let _pic_id = common::seed_picture_with_tag(&db, alice_id, "vacation").await;

    // ── Alice creates the outgoing share ──────────────────────────────────────
    let alice_token = common::federation::user_jwt(&cfg_a, "alice", alice_id);
    let create_resp = post(
        &client,
        addr_a,
        "/api/authenticated/shares/outgoing",
        Some(&alice_token),
        &json!({
            "tag_path":           "vacation",
            "recipient_username": "bob",
            "recipient_instance": "b.test",
            "allow_share_back":   false,
            "future":             false
        }),
    )
    .await;

    assert_eq!(
        create_resp.status(),
        200,
        "Alice must be able to create a cross-instance share"
    );
    let share_body: Value = create_resp.json().await.unwrap();
    let outgoing_id: Uuid = share_body["id"].as_str().unwrap().parse().unwrap();

    // Bob now has a Pending IncomingShare.
    let bob_incoming = IncomingShareRepository::list_by_recipient(&db, bob_id)
        .await
        .unwrap();
    assert_eq!(
        bob_incoming.len(),
        1,
        "Bob must have exactly one incoming share"
    );
    assert_eq!(bob_incoming[0].status, ShareStatus::Pending);
    assert_eq!(bob_incoming[0].outgoing_share_id, outgoing_id);

    // ── Bob accepts ───────────────────────────────────────────────────────────
    let incoming_id = bob_incoming[0].id;
    let bob_token = common::federation::user_jwt(&cfg_b, "bob", bob_id);
    let accept_resp = post(
        &client,
        addr_b,
        &format!("/api/authenticated/shares/incoming/{incoming_id}/accept"),
        Some(&bob_token),
        &json!({}),
    )
    .await;

    assert_eq!(
        accept_resp.status(),
        200,
        "Bob must be able to accept the incoming share"
    );
    let accept_body: Value = accept_resp.json().await.unwrap();
    assert_eq!(accept_body["accepted"], json!(true));

    // Give the async picture announcement a moment to propagate.
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Alice's OutgoingShare is now Active.
    let outgoing = OutgoingShareRepository::get_by_id(&db, outgoing_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        outgoing.status,
        ShareStatus::Active,
        "Alice's OutgoingShare must be Active after Bob's accept"
    );

    // Bob has Alice's picture with the SharedToMe tag.
    assert_eq!(
        common::count_received_pictures(&db, bob_id).await,
        1,
        "Bob must have Alice's picture as a received picture"
    );
    let tags = common::received_picture_tags(&db, bob_id).await;
    let expected_tag = common::shared_to_me_tag("alice", "a.test", "vacation");
    assert!(
        tags.contains(&expected_tag),
        "Bob's received picture must carry the SharedToMe tag '{expected_tag}', got: {tags:?}"
    );
}

// ── Test 3: revoke removes received pictures ──────────────────────────────────

/// After the full announce + accept + pictures flow, Alice revokes her share.
/// Bob's received pictures and SharedToMe tags must be cleaned up.
#[sqlx::test(migrator = "MIGRATOR")]
async fn cross_instance_revoke_removes_received_pictures(db: PgPool) {
    let (addr_a, addr_b, cfg_a, cfg_b, alice_id, bob_id) = spawn_pair(db.clone()).await;
    let client = Client::new();

    // ── Set up: announce + accept + pictures (same as test 2) ─────────────────
    common::seed_picture_with_tag(&db, alice_id, "vacation").await;

    let alice_token = common::federation::user_jwt(&cfg_a, "alice", alice_id);
    let share_resp: Value = post(
        &client,
        addr_a,
        "/api/authenticated/shares/outgoing",
        Some(&alice_token),
        &json!({
            "tag_path": "vacation", "recipient_username": "bob",
            "recipient_instance": "b.test", "allow_share_back": false, "future": false
        }),
    )
    .await
    .json()
    .await
    .unwrap();
    let outgoing_id: Uuid = share_resp["id"].as_str().unwrap().parse().unwrap();

    let incoming_id = IncomingShareRepository::list_by_recipient(&db, bob_id)
        .await
        .unwrap()[0]
        .id;

    let bob_token = common::federation::user_jwt(&cfg_b, "bob", bob_id);
    post(
        &client,
        addr_b,
        &format!("/api/authenticated/shares/incoming/{incoming_id}/accept"),
        Some(&bob_token),
        &json!({}),
    )
    .await;

    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    assert_eq!(
        common::count_received_pictures(&db, bob_id).await,
        1,
        "Precondition: Bob must have Alice's picture before revocation"
    );

    // ── Alice revokes ─────────────────────────────────────────────────────────
    let revoke_resp = post(
        &client,
        addr_a,
        &format!("/api/authenticated/shares/outgoing/{outgoing_id}/revoke"),
        Some(&alice_token),
        &json!({}),
    )
    .await;

    assert_eq!(revoke_resp.status(), 200, "Revoke must succeed");

    // Allow the revocation message to propagate to B.
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    // Alice's OutgoingShare is Revoked.
    let outgoing = OutgoingShareRepository::get_by_id(&db, outgoing_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(outgoing.status, ShareStatus::Revoked);

    // Bob has no received pictures left.
    assert_eq!(
        common::count_received_pictures(&db, bob_id).await,
        0,
        "Bob's received pictures must be deleted after revocation"
    );
}

// ── Test 4: reject tombstones sender's outgoing share ────────────────────────

/// Alice announces a share; Bob rejects it before accepting.
/// Alice's OutgoingShare must be Tombstoned.
#[sqlx::test(migrator = "MIGRATOR")]
async fn cross_instance_reject_tombstones_outgoing_share(db: PgPool) {
    let (addr_a, addr_b, cfg_a, cfg_b, alice_id, bob_id) = spawn_pair(db.clone()).await;
    let client = Client::new();

    // ── Alice creates the share ───────────────────────────────────────────────
    let alice_token = common::federation::user_jwt(&cfg_a, "alice", alice_id);
    let share_resp: Value = post(
        &client,
        addr_a,
        "/api/authenticated/shares/outgoing",
        Some(&alice_token),
        &json!({
            "tag_path": "vacation", "recipient_username": "bob",
            "recipient_instance": "b.test", "allow_share_back": false, "future": false
        }),
    )
    .await
    .json()
    .await
    .unwrap();
    let outgoing_id: Uuid = share_resp["id"].as_str().unwrap().parse().unwrap();

    let incoming_id = IncomingShareRepository::list_by_recipient(&db, bob_id)
        .await
        .unwrap()[0]
        .id;

    // ── Bob rejects ───────────────────────────────────────────────────────────
    // For the reject to reach A, B needs to know A's backend URL.
    // spawn_pair already seeds `FederationBackend("alice", "a.test")` in B's cache.
    let bob_token = common::federation::user_jwt(&cfg_b, "bob", bob_id);
    let reject_resp = post(
        &client,
        addr_b,
        &format!("/api/authenticated/shares/incoming/{incoming_id}/reject"),
        Some(&bob_token),
        &json!({}),
    )
    .await;

    assert_eq!(reject_resp.status(), 200, "Bob's reject must succeed");

    // Allow A to process the federation reject callback.
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    // Bob's IncomingShare is Tombstoned.
    let bob_incoming = IncomingShareRepository::list_by_recipient(&db, bob_id)
        .await
        .unwrap();
    assert_eq!(bob_incoming[0].status, ShareStatus::Tombstoned);

    // Alice's OutgoingShare is Tombstoned.
    let outgoing = OutgoingShareRepository::get_by_id(&db, outgoing_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        outgoing.status,
        ShareStatus::Tombstoned,
        "Alice's OutgoingShare must be Tombstoned after Bob rejects"
    );

    // No received pictures on Bob's side.
    assert_eq!(common::count_received_pictures(&db, bob_id).await, 0);
}
