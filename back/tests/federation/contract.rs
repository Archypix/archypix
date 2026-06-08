//! End-to-end federation protocol tests.
//!
//! Two real Axum servers share a `PgPool` (logically partitioned by `global_domain`).
//! Tests call service functions directly — A's `FederationClient` makes real TCP calls
//! to B, and vice versa. The test itself never constructs HTTP requests.

use crate::common;

use archypix_back::domain::share::ShareStatus;
use archypix_back::infra::config::Config;
use archypix_back::repository::share::{IncomingShareRepository, OutgoingShareRepository};
use archypix_back::services::shares::{
    accept_incoming_share, create_outgoing_share, reject_incoming_share, revoke_outgoing_share,
};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

// ── Setup ─────────────────────────────────────────────────────────────────────

/// Spawn both backends, wire their backend-URL caches to bypass WebFinger,
/// and seed alice (on A) and bob (on B).
///
/// Returns `(cache_a, cfg_a, cache_b, cfg_b, alice_id, bob_id)`.
async fn spawn_pair(
    db: PgPool,
) -> (
    Arc<common::InMemoryCache>,
    Config,
    Arc<common::InMemoryCache>,
    Config,
    Uuid,
    Uuid,
) {
    let (addr_a, cache_a, cfg_a) =
        common::federation::spawn_backend(db.clone(), common::federation::config_a()).await;
    let (addr_b, cache_b, cfg_b) =
        common::federation::spawn_backend(db.clone(), common::federation::config_b()).await;

    common::federation::seed_backend_url(&cache_a, "bob", "b.test", &format!("http://{addr_b}"))
        .await;
    common::federation::seed_backend_url(&cache_b, "alice", "a.test", &format!("http://{addr_a}"))
        .await;

    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let bob_id = common::seed_user(&db, "bob", "pass").await;

    (cache_a, cfg_a, cache_b, cfg_b, alice_id, bob_id)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// A's `FederationClient::get_or_wait_federation_token` drives the full handshake:
///   A → auth/request on B → B issues JWT → B → auth/grant on A → token in A's cache.
#[sqlx::test(migrator = "MIGRATOR")]
async fn auth_handshake_grants_token_to_requester(db: PgPool) {
    let (cache_a, cfg_a, _, _, _, _) = spawn_pair(db).await;
    let fed_a = common::federation::make_client(&cfg_a, &cache_a);

    let token = fed_a
        .get_or_wait_federation_token("alice", "bob", "b.test")
        .await
        .expect("handshake must complete and return a token");

    assert!(
        !token.is_empty(),
        "returned federation token must be non-empty"
    );
}

/// Alice creates a share → Bob gets a Pending IncomingShare → Bob accepts →
/// A marks the OutgoingShare Active and announces pictures → Bob has the picture.
#[sqlx::test(migrator = "MIGRATOR")]
async fn cross_instance_share_announce_and_accept_propagates_pictures(db: PgPool) {
    let (cache_a, cfg_a, cache_b, cfg_b, alice_id, bob_id) = spawn_pair(db.clone()).await;
    let fed_a = common::federation::make_client(&cfg_a, &cache_a);
    let fed_b = common::federation::make_client(&cfg_b, &cache_b);

    common::seed_picture_with_tag(&db, alice_id, "vacation").await;

    let share = create_outgoing_share(
        &db, &*cache_a, &fed_a, &cfg_a, alice_id, "alice", "vacation", "bob", "b.test", false,
        false, None,
    )
    .await
    .unwrap();

    let bob_incoming = IncomingShareRepository::list_by_recipient(&db, bob_id)
        .await
        .unwrap();
    assert_eq!(bob_incoming.len(), 1);
    assert_eq!(bob_incoming[0].status, ShareStatus::Pending);
    assert_eq!(bob_incoming[0].outgoing_share_id, share.id);

    accept_incoming_share(
        &db,
        &*cache_b,
        &fed_b,
        &cfg_b,
        bob_id,
        "bob",
        bob_incoming[0].id,
    )
    .await
    .unwrap();

    // Give A time to announce pictures asynchronously.
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let outgoing = OutgoingShareRepository::get_by_id(&db, share.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(outgoing.status, ShareStatus::Active);

    assert_eq!(common::count_received_pictures(&db, bob_id).await, 1);
    let tags = common::received_picture_tags(&db, bob_id).await;
    let expected = common::shared_to_me_tag("alice", "a.test", "vacation");
    assert!(tags.contains(&expected), "got: {tags:?}");
}

/// After the full announce + accept + pictures flow, Alice revokes.
/// Bob's received pictures must be deleted.
#[sqlx::test(migrator = "MIGRATOR")]
async fn cross_instance_revoke_removes_received_pictures(db: PgPool) {
    let (cache_a, cfg_a, cache_b, cfg_b, alice_id, bob_id) = spawn_pair(db.clone()).await;
    let fed_a = common::federation::make_client(&cfg_a, &cache_a);
    let fed_b = common::federation::make_client(&cfg_b, &cache_b);

    common::seed_picture_with_tag(&db, alice_id, "vacation").await;

    let share = create_outgoing_share(
        &db, &*cache_a, &fed_a, &cfg_a, alice_id, "alice", "vacation", "bob", "b.test", false,
        false, None,
    )
    .await
    .unwrap();

    let incoming_id = IncomingShareRepository::list_by_recipient(&db, bob_id)
        .await
        .unwrap()[0]
        .id;

    accept_incoming_share(&db, &*cache_b, &fed_b, &cfg_b, bob_id, "bob", incoming_id)
        .await
        .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
    assert_eq!(
        common::count_received_pictures(&db, bob_id).await,
        1,
        "precondition"
    );

    revoke_outgoing_share(&db, &*cache_a, &fed_a, &cfg_a, alice_id, "alice", share.id)
        .await
        .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    let outgoing = OutgoingShareRepository::get_by_id(&db, share.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(outgoing.status, ShareStatus::Revoked);
    assert_eq!(common::count_received_pictures(&db, bob_id).await, 0);
}

/// Alice announces a share, Bob accepts and receives pictures, then Bob rejects it
/// Expected outcome: received pictures deleted, both shares Tombstoned.
#[sqlx::test(migrator = "MIGRATOR")]
async fn cross_instance_reject_active_share_cleans_up_received_pictures(db: PgPool) {
    let (cache_a, cfg_a, cache_b, cfg_b, alice_id, bob_id) = spawn_pair(db.clone()).await;
    let fed_a = common::federation::make_client(&cfg_a, &cache_a);
    let fed_b = common::federation::make_client(&cfg_b, &cache_b);

    common::seed_picture_with_tag(&db, alice_id, "vacation").await;

    // Bring the share to Active with pictures propagated (same setup as the revoke test).
    let share = create_outgoing_share(
        &db, &*cache_a, &fed_a, &cfg_a, alice_id, "alice", "vacation", "bob", "b.test", false,
        false, None,
    )
    .await
    .unwrap();

    let incoming_id = IncomingShareRepository::list_by_recipient(&db, bob_id)
        .await
        .unwrap()[0]
        .id;

    accept_incoming_share(&db, &*cache_b, &fed_b, &cfg_b, bob_id, "bob", incoming_id)
        .await
        .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
    assert_eq!(
        common::count_received_pictures(&db, bob_id).await,
        1,
        "precondition"
    );

    // Bob rejects the now-Active share — triggers cleanup then federation notify.
    reject_incoming_share(&db, &*cache_b, &fed_b, &cfg_b, bob_id, "bob", incoming_id)
        .await
        .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    // Bob's received pictures are gone.
    assert_eq!(common::count_received_pictures(&db, bob_id).await, 0);

    // Bob's IncomingShare is Tombstoned.
    let bob_incoming = IncomingShareRepository::list_by_recipient(&db, bob_id)
        .await
        .unwrap();
    assert_eq!(bob_incoming[0].status, ShareStatus::Tombstoned);

    // Alice's OutgoingShare is Tombstoned.
    let outgoing = OutgoingShareRepository::get_by_id(&db, share.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(outgoing.status, ShareStatus::Tombstoned);
}

/// Alice announces a share; Bob rejects it.
/// Alice's OutgoingShare must be Tombstoned.
#[sqlx::test(migrator = "MIGRATOR")]
async fn cross_instance_reject_tombstones_outgoing_share(db: PgPool) {
    let (cache_a, cfg_a, cache_b, cfg_b, alice_id, bob_id) = spawn_pair(db.clone()).await;
    let fed_a = common::federation::make_client(&cfg_a, &cache_a);
    let fed_b = common::federation::make_client(&cfg_b, &cache_b);

    let share = create_outgoing_share(
        &db, &*cache_a, &fed_a, &cfg_a, alice_id, "alice", "vacation", "bob", "b.test", false,
        false, None,
    )
    .await
    .unwrap();

    let incoming_id = IncomingShareRepository::list_by_recipient(&db, bob_id)
        .await
        .unwrap()[0]
        .id;

    reject_incoming_share(&db, &*cache_b, &fed_b, &cfg_b, bob_id, "bob", incoming_id)
        .await
        .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    let bob_incoming = IncomingShareRepository::list_by_recipient(&db, bob_id)
        .await
        .unwrap();
    assert_eq!(bob_incoming[0].status, ShareStatus::Tombstoned);

    let outgoing = OutgoingShareRepository::get_by_id(&db, share.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(outgoing.status, ShareStatus::Tombstoned);
    assert_eq!(common::count_received_pictures(&db, bob_id).await, 0);
}
