mod common;

use archypix_back::domain::share::ShareStatus;
use archypix_back::infra::config::Config;
use archypix_back::repository::share::{IncomingShareRepository, OutgoingShareRepository};
use archypix_back::services::shares;
use sqlx::PgPool;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

// ── helpers ───────────────────────────────────────────────────────────────────

fn config() -> Config {
    Config::test_defaults()
}

/// Share Alice's `tag_path` with Bob (same backend — recipient_instance = global_domain).
async fn alice_shares_with_bob(
    db: &PgPool,
    alice_id: uuid::Uuid,
    tag_path: &str,
) -> archypix_back::domain::share::OutgoingShare {
    let config = config();
    let (fed, cache) = common::make_federation(&config);

    shares::create_outgoing_share(
        db,
        cache.as_ref(),
        &fed,
        &config,
        alice_id,
        "alice",
        tag_path,
        "bob",
        "test.com", // same as global_domain → same-backend path
        false,
        false,
        None,
    )
    .await
    .unwrap()
}

// ── create_outgoing_share ─────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR")]
async fn create_outgoing_share_same_backend_creates_incoming_share(db: PgPool) {
    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let _bob_id = common::seed_user(&db, "bob", "pass").await;

    let share = alice_shares_with_bob(&db, alice_id, "vacation").await;

    let outgoing = OutgoingShareRepository::get_by_id(&db, share.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(outgoing.status, ShareStatus::Pending);
    assert_eq!(outgoing.recipient_username, "bob");

    // The IncomingShare must be auto-created in the same transaction.
    let incoming = IncomingShareRepository::find_by_outgoing_share(&db, share.id, "test.com")
        .await
        .unwrap()
        .expect("incoming share must be created for same-backend recipient");
    assert_eq!(incoming.status, ShareStatus::Pending);
    assert_eq!(incoming.sender_username, "alice");
}

// ── accept_incoming_share ─────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR")]
async fn accept_incoming_share_registers_pictures(db: PgPool) {
    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let bob_id = common::seed_user(&db, "bob", "pass").await;

    // Alice has a picture tagged "vacation".
    common::seed_picture_with_tag(&db, alice_id, "vacation").await;

    let share = alice_shares_with_bob(&db, alice_id, "vacation").await;
    let incoming = IncomingShareRepository::find_by_outgoing_share(&db, share.id, "test.com")
        .await
        .unwrap()
        .unwrap();

    let config = config();
    let (fed, cache) = common::make_federation(&config);

    let count = shares::accept_incoming_share(
        &db,
        cache.as_ref(),
        &fed,
        &config,
        bob_id,
        "bob",
        incoming.id,
    )
    .await
    .unwrap();

    assert_eq!(count, 1, "one picture must be registered");
    assert_eq!(
        common::count_received_pictures(&db, bob_id).await,
        1,
        "Bob must have one received picture"
    );

    let expected_tag = common::shared_to_me_tag("alice", "test.com", "vacation");
    let tags = common::received_picture_tags(&db, bob_id).await;
    assert!(
        tags.contains(&expected_tag),
        "SharedToMe tag must be assigned; got: {:?}",
        tags
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn accept_incoming_share_is_idempotent(db: PgPool) {
    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let bob_id = common::seed_user(&db, "bob", "pass").await;
    common::seed_picture_with_tag(&db, alice_id, "vacation").await;

    let share = alice_shares_with_bob(&db, alice_id, "vacation").await;
    let incoming = IncomingShareRepository::find_by_outgoing_share(&db, share.id, "test.com")
        .await
        .unwrap()
        .unwrap();

    let config = config();
    let (fed, cache) = common::make_federation(&config);

    // First accept
    shares::accept_incoming_share(
        &db,
        cache.as_ref(),
        &fed,
        &config,
        bob_id,
        "bob",
        incoming.id,
    )
    .await
    .unwrap();

    // Second accept — must be a no-op (returns 0, no duplicate pictures)
    let second = shares::accept_incoming_share(
        &db,
        cache.as_ref(),
        &fed,
        &config,
        bob_id,
        "bob",
        incoming.id,
    )
    .await
    .unwrap();

    assert_eq!(second, 0, "second accept must be a no-op");
    assert_eq!(
        common::count_received_pictures(&db, bob_id).await,
        1,
        "still exactly one received picture"
    );
}

// ── revoke_outgoing_share ─────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR")]
async fn revoke_outgoing_share_removes_shared_tags(db: PgPool) {
    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let bob_id = common::seed_user(&db, "bob", "pass").await;
    common::seed_picture_with_tag(&db, alice_id, "vacation").await;

    let share = alice_shares_with_bob(&db, alice_id, "vacation").await;
    let incoming = IncomingShareRepository::find_by_outgoing_share(&db, share.id, "test.com")
        .await
        .unwrap()
        .unwrap();

    let config = config();
    let (fed, cache) = common::make_federation(&config);

    // Bob accepts → picture + tag appear
    shares::accept_incoming_share(
        &db,
        cache.as_ref(),
        &fed,
        &config,
        bob_id,
        "bob",
        incoming.id,
    )
    .await
    .unwrap();
    assert_eq!(common::count_received_pictures(&db, bob_id).await, 1);

    // Alice revokes → tag removed, unreachable received picture deleted
    shares::revoke_outgoing_share(
        &db,
        cache.as_ref(),
        &fed,
        &config,
        alice_id,
        "alice",
        share.id,
    )
    .await
    .unwrap();

    assert_eq!(
        common::count_received_pictures(&db, bob_id).await,
        0,
        "received picture must be deleted after revocation"
    );
    assert!(
        common::received_picture_tags(&db, bob_id).await.is_empty(),
        "SharedToMe tags must be gone"
    );

    let outgoing = OutgoingShareRepository::get_by_id(&db, share.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(outgoing.status, ShareStatus::Revoked);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn revoke_outgoing_share_before_accept_leaves_no_incoming_tags(db: PgPool) {
    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let _bob_id = common::seed_user(&db, "bob", "pass").await;

    let share = alice_shares_with_bob(&db, alice_id, "vacation").await;
    let config = config();
    let (fed, cache) = common::make_federation(&config);

    // Revoke immediately, before Bob accepts (no received pictures yet)
    shares::revoke_outgoing_share(
        &db,
        cache.as_ref(),
        &fed,
        &config,
        alice_id,
        "alice",
        share.id,
    )
    .await
    .unwrap();

    let outgoing = OutgoingShareRepository::get_by_id(&db, share.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(outgoing.status, ShareStatus::Revoked);
}

// ── reject_incoming_share ─────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR")]
async fn reject_incoming_share_pending_tombstones_outgoing(db: PgPool) {
    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let bob_id = common::seed_user(&db, "bob", "pass").await;

    let share = alice_shares_with_bob(&db, alice_id, "vacation").await;
    let incoming = IncomingShareRepository::find_by_outgoing_share(&db, share.id, "test.com")
        .await
        .unwrap()
        .unwrap();

    let config = config();
    let (fed, cache) = common::make_federation(&config);

    // Bob rejects a pending share
    shares::reject_incoming_share(
        &db,
        cache.as_ref(),
        &fed,
        &config,
        bob_id,
        "bob",
        incoming.id,
    )
    .await
    .unwrap();

    let incoming_after = IncomingShareRepository::get_by_id(&db, incoming.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(incoming_after.status, ShareStatus::Tombstoned);

    // Same-backend: sender's OutgoingShare must also be tombstoned
    let outgoing_after = OutgoingShareRepository::get_by_id(&db, share.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(outgoing_after.status, ShareStatus::Tombstoned);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn reject_incoming_share_active_removes_tags(db: PgPool) {
    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let bob_id = common::seed_user(&db, "bob", "pass").await;
    common::seed_picture_with_tag(&db, alice_id, "vacation").await;

    let share = alice_shares_with_bob(&db, alice_id, "vacation").await;
    let incoming = IncomingShareRepository::find_by_outgoing_share(&db, share.id, "test.com")
        .await
        .unwrap()
        .unwrap();

    let config = config();
    let (fed, cache) = common::make_federation(&config);

    // Bob accepts first
    shares::accept_incoming_share(
        &db,
        cache.as_ref(),
        &fed,
        &config,
        bob_id,
        "bob",
        incoming.id,
    )
    .await
    .unwrap();
    assert_eq!(common::count_received_pictures(&db, bob_id).await, 1);

    // Then rejects → cleanup must run
    shares::reject_incoming_share(
        &db,
        cache.as_ref(),
        &fed,
        &config,
        bob_id,
        "bob",
        incoming.id,
    )
    .await
    .unwrap();

    assert_eq!(
        common::count_received_pictures(&db, bob_id).await,
        0,
        "received picture must be deleted on rejection"
    );
    let incoming_after = IncomingShareRepository::get_by_id(&db, incoming.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(incoming_after.status, ShareStatus::Tombstoned);
}

// ── register_received_pictures ────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR")]
async fn register_received_pictures_is_idempotent(db: PgPool) {
    use archypix_back::domain::tag::TagPath;
    use archypix_back::services::shares::{ReceivedPictureInfo, register_received_pictures};
    use uuid::Uuid;

    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let bob_id = common::seed_user(&db, "bob", "pass").await;

    let share = alice_shares_with_bob(&db, alice_id, "vacation").await;
    let incoming = IncomingShareRepository::find_by_outgoing_share(&db, share.id, "test.com")
        .await
        .unwrap()
        .unwrap();

    let shared_tag = TagPath::shared_to_me("alice", "test.com", &TagPath::from_ltree("vacation"));
    let pics = vec![ReceivedPictureInfo {
        remote_picture_id: Uuid::new_v4().to_string(),
        owner_username: "alice".to_string(),
        owner_instance_domain: "test.com".to_string(),
        filename: None,
        mime_type: None,
        file_size: None,
        width: None,
        height: None,
        captured_at: None,
    }];

    // Register twice — second call must be a no-op (ON CONFLICT DO UPDATE / DO NOTHING)
    let n1 = register_received_pictures(&db, bob_id, incoming.id, &shared_tag, &pics)
        .await
        .unwrap();
    let n2 = register_received_pictures(&db, bob_id, incoming.id, &shared_tag, &pics)
        .await
        .unwrap();

    assert_eq!(n1, 1);
    assert_eq!(n2, 1); // function returns slice length, not inserted count
    assert_eq!(
        common::count_received_pictures(&db, bob_id).await,
        1,
        "only one picture row must exist"
    );
}

// ── cleanup_incoming_share ────────────────────────────────────────────────────

/// A picture tagged with both "vacation" and "trip" appears in two separate shares.
/// Revoking share1 ("vacation") removes its incoming_share tag, but the picture
/// still has share2's ("trip") incoming_share tag, so it must NOT be deleted.
#[sqlx::test(migrator = "MIGRATOR")]
async fn cleanup_incoming_share_deletes_unreachable_pictures_only(db: PgPool) {
    use archypix_back::repository::tag::TagRepository;

    let alice_id = common::seed_user(&db, "alice", "pass").await;
    let bob_id = common::seed_user(&db, "bob", "pass").await;

    // One picture tagged with both "vacation" and "trip"
    let pic_id = common::seed_picture(&db, alice_id).await;
    TagRepository::batch_assign(
        &db,
        alice_id,
        &[pic_id],
        &["vacation".to_string(), "trip".to_string()],
    )
    .await
    .unwrap();

    // Share 1: "vacation" → Bob
    let share1 = alice_shares_with_bob(&db, alice_id, "vacation").await;
    let incoming1 = IncomingShareRepository::find_by_outgoing_share(&db, share1.id, "test.com")
        .await
        .unwrap()
        .unwrap();

    let config = config();
    let (fed, cache) = common::make_federation(&config);

    // Share 2: "trip" → Bob (different tag — no unique-constraint conflict)
    let share2 = shares::create_outgoing_share(
        &db,
        cache.as_ref(),
        &fed,
        &config,
        alice_id,
        "alice",
        "trip",
        "bob",
        "test.com",
        false,
        false,
        None,
    )
    .await
    .unwrap();
    let incoming2 = IncomingShareRepository::find_by_outgoing_share(&db, share2.id, "test.com")
        .await
        .unwrap()
        .unwrap();

    // Bob accepts both → same received picture row, two incoming_share tags
    shares::accept_incoming_share(
        &db,
        cache.as_ref(),
        &fed,
        &config,
        bob_id,
        "bob",
        incoming1.id,
    )
    .await
    .unwrap();
    shares::accept_incoming_share(
        &db,
        cache.as_ref(),
        &fed,
        &config,
        bob_id,
        "bob",
        incoming2.id,
    )
    .await
    .unwrap();

    assert_eq!(
        common::count_received_pictures(&db, bob_id).await,
        1,
        "same remote picture → only one received row"
    );

    // Revoke share1 → removes its incoming_share tag, but share2's tag remains
    shares::revoke_outgoing_share(
        &db,
        cache.as_ref(),
        &fed,
        &config,
        alice_id,
        "alice",
        share1.id,
    )
    .await
    .unwrap();

    assert_eq!(
        common::count_received_pictures(&db, bob_id).await,
        1,
        "picture still reachable via share2 must not be deleted"
    );
}
