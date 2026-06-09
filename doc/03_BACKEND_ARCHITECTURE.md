# Backend Architecture

## A) Technology stack

- **Axum** (HTTP framework) + **Tokio** (async runtime) — consistent with the resolver component.
- **SQLx** — compile-time checked SQL, direct Postgres feature access (LTREE, JSONB, custom types), migration support.
- **Redis** — session cache, presigned URL cache, federation token cache, backend domain cache.

## B) Layered architecture

| Layer        | Responsibility                                                                | Can depend on                               | Must NOT depend on                |
|--------------|-------------------------------------------------------------------------------|---------------------------------------------|-----------------------------------|
| `api`        | HTTP handlers, auth extraction, request/response models.                      | `services`, `repository`, `domain`, `infra` | External connectivity details.    |
| `services`   | Multi-step workflows and transaction boundaries.                              | `repository`, `clients`, `domain`, `infra`  | Axum types, HTTP-specific models. |
| `clients`    | Outbound HTTP adapters (federation backends, resolver, S3).                   | `infra`, `domain`                           | `services`, `repository`, `api`.  |
| `repository` | SQL operations only — no business logic.                                      | `domain`, `infra::error`                    | `services`, `clients`.            |
| `domain`     | Business types, invariants, pure transformations, tagging pipeline evaluator. | std + lightweight crates only               | `repository`, `infra`, clients.   |
| `infra`      | Raw connectivity primitives: config, error, Redis, S3, crypto (JWT, hashing). | External SDKs                               | `api`, `services`, `clients`.     |
| `state`      | `AppState` — bootstrap, holds all composed handles.                           | `infra`, `clients`                          | `services`, `repository`, `api`.  |

**Key rules:**

- Repository functions accept `Executor<'e, Database = Postgres>` — callable on pool or transaction.
- Multi-step workflows (user creation, picture upload, share creation, job completion) run in an explicit SQL transaction managed by the service or
  handler. For cross-instance share creation, the outbound federation HTTP calls run while the transaction is still open so that any federation
  failure
  automatically rolls back the `OutgoingShare` insert.
- API handlers call repositories directly only for single-step CRUD with no orchestration.

## C) Module layout (`back/src/`)

```
main.rs / state.rs

domain/
  auth.rs           # TokenType, JwtClaims
  user.rs / user_settings.rs
  picture.rs        # Picture (includes file_hash, file_size), PictureVersion, UploadSession
  tag.rs            # TagPath (newtype), TagSource, Tag
  share.rs          # OutgoingShare, IncomingShare
  federation.rs     # FederationMessage, BackendMapping
  job.rs            # Job (includes claim_token), re-exports from archypix-common
  tagging.rs / pipeline.rs   # pipeline config types + pure evaluator

repository/
  user.rs / picture.rs / picture_version.rs / user_settings.rs
  tag.rs / share.rs / auth.rs / job.rs / tagging.rs
  pipeline.rs     # dirty-picture queries, pipeline tag assignment

clients/
  federation/
    mod.rs          # FederationClient struct + shared protocol types
    handshake.rs    # WebFinger resolution, token request/grant/store/issue
    shares.rs       # announce_share, send_share_accept, send_share_reject, send_revocation, announce_pictures, presign_remote_pictures
  resolver.rs       # self_register, update_mapping, verify_token

services/
  auth.rs / users.rs / pictures.rs / user_settings.rs / shares.rs / jobs.rs
  federation.rs     # inbound federation protocol handlers (receive_share_announcement, receive_share_accept, receive_share_revoke, receive_share_reject, receive_pictures_announcement, presign_batch_for_token)

api/
  middleware/auth_user.rs / auth_admin.rs / auth_resolver.rs / auth_federation.rs / auth_worker.rs
  user/auth.rs / users.rs / pictures.rs / settings.rs / shares.rs / tags.rs / jobs.rs / tagging_services.rs
  admin/handlers.rs + models.rs
  federation/handlers.rs + models.rs
  resolver/handlers.rs + models.rs
  worker/handlers.rs + models.rs

infra/
  config.rs / error.rs / redis.rs / crypto.rs / db.rs / s3.rs
  tasks.rs        # in-process Tokio task queue (tag rename)
  pipeline.rs     # tagging pipeline background loop
  job_watchdog.rs # periodic reset of stale processing jobs
```

## D) AppState

```rust
pub struct AppState {
    pub config: Config,
    pub db: PgPool,
    pub redis: RedisClient,
    pub jwt: JwtService,
    pub worker_jwt: JwtService,      // verifies inbound worker tokens
    pub storage: StorageClient,
    pub federation: FederationClient,
    pub resolver: ResolverClient,
    pub task_queue: TaskQueue,       // in-process task queue (tag rename)
    pub pipeline_notify: Arc<Notify>, // wakes the tagging pipeline loop
}
```

## E) Tagging pipeline

The pipeline runs as a background Tokio task (`infra/pipeline.rs`). It evaluates all enabled tagging services against dirty pictures and applies the
resulting tag assignments.

**Dirty picture detection** — two schema columns drive this:

- `pictures.last_pipeline_run_at` — `NULL` on new/invalidated pictures, set to `NOW()` after a successful run.
- `tagging_services.last_invalidated_at` — bumped on any configuration change (rule/segment/mapping add or remove, enable/disable).

A picture is dirty when `last_pipeline_run_at IS NULL OR last_pipeline_run_at < last_invalidated_at` for any of its user's enabled services.

**Wake model** — the loop uses a `tokio::sync::Notify` for immediate event-driven wakes, with a configurable polling interval as a recovery fallback (
`PIPELINE_POLL_INTERVAL_SECS`, default 1 hour). Callsites call `pipeline_notify.notify_one()` after:

- Ingest (new picture → `last_pipeline_run_at = NULL` by default)
- Manual tag edit (pictures explicitly re-invalidated)
- Service config change (service's `last_invalidated_at` bumped)
- Inbound share announcement (new received pictures → `NULL` by default)

**Evaluation order** — `SharedTagMapping → Rule → Segmentation`. Each service sees tags added by earlier ones (in-memory accumulation per picture), so
downstream services can use upstream results via `requires`.

**Rule predicates** — the `rule` service type supports a simple predicate DSL stored in `rule_tagging_services.predicate`. Supported forms:
`gps_within_bbox(lat_min, lat_max, lon_min, lon_max)`, `capture_year(YYYY)`, `capture_month(M)`, `filename_contains("string")`. Predicates are
validated at rule creation time.

**Tag assignment** — pipeline-assigned tags use `source = rule | segment | share_mapping` (not `manual`), so user tag-remove operations never touch
them. Assignment is idempotent (`ON CONFLICT DO NOTHING`).

**Tag removal** (TODO) — if the service enabled this feature, the service would be able to remove a tag from a picture if this tag has the same
`source` as the service's rule and that no other service has assigned to it that tag (TODO).

# Backend REST API

## 1) API layout

| Section                      | Base path                      | Auth                      |
|------------------------------|--------------------------------|---------------------------|
| Resolver endpoints           | `/api/resolver/*`              | Resolver JWT              |
| Admin endpoints              | `/api/admin/*`                 | User JWT with `is_admin`  |
| Public/auth endpoints        | `/api/auth/*`, `/api/public/*` | Mixed                     |
| Authenticated user endpoints | `/api/authenticated/*`         | User JWT                  |
| Federation endpoints         | `/api/federation/*`            | Federation JWT (pairwise) |
| Worker endpoints             | `/api/worker/*`                | Worker JWT                |

## 2) Domain terminology

| Term               | Env var         | Example                | Description                                                                                                     |
|--------------------|-----------------|------------------------|-----------------------------------------------------------------------------------------------------------------|
| **Global domain**  | `GLOBAL_DOMAIN` | `example.com`          | Public identity domain. Used in `@user:example.com`, JWTs, DB, federation. Never changes from user perspective. |
| **Backend domain** | `BACK_DOMAIN`   | `backend1.example.com` | Actual API server. Resolved via WebFinger, cached in Redis. Never stored persistently.                          |

All persistent storage uses the **global domain**. Backend domains are derived on demand and cached.

## 3) JWT tokens

| Claim        | Description                                                                               |
|--------------|-------------------------------------------------------------------------------------------|
| `sub`        | Username (user tokens) or global domain (federation tokens) or worker_id (worker tokens). |
| `uid`        | User UUID (user tokens only).                                                             |
| `is_admin`   | Boolean. Admin endpoints check this, not a separate token type.                           |
| `instance`   | Global domain of the issuing instance.                                                    |
| `token_type` | `user` \| `resolver` \| `federation` \| `worker`. There is no `admin` token type.         |
| `aud`        | Backend domain of the verifying instance (checked against `BACK_DOMAIN`).                 |

Worker tokens: `sub = worker_id`, `iss = global_domain`, signed with `WORKER_JWT_SECRET` (HS256, 300 s TTL). Workers cache the token and refresh it 30
s before expiry, so at most one token generation per ~270 s per worker process.

## 4) Federation authentication (pairwise JWT)

The recipient instance issues a JWT to the requesting instance. All domains in federation messages are global domains — backend domains are resolved
via WebFinger and cached.

**Handshake:**

1. A → B: `POST /api/federation/auth/request` `{ requester_instance, username, scope, nonce }`
2. B resolves A's backend via WebFinger; sends grant to resolved address.
3. B → A: `POST /api/federation/auth/grant` `{ issuer_instance, token, expires_at, scope, nonce }`
4. A stores token in Redis under `federation:token:{B_global_domain}`.

## 5) Endpoint layout

### Resolver endpoints (on backend, called by Resolver)

| Method | Path                             | Description                                  |
|--------|----------------------------------|----------------------------------------------|
| `POST` | `/api/resolver/users`            | Create user (only when `USE_RESOLVER=true`). |
| `GET`  | `/api/resolver/users/{username}` | Fetch user for resolver validation.          |

### Resolver service endpoints (port 8080)

| Method | Path                                                    | Description                                    |
|--------|---------------------------------------------------------|------------------------------------------------|
| `GET`  | `/.well-known/webfinger?resource=archypix:@user:domain` | Resolve username to backend URL.               |
| `POST` | `/api/register`                                         | Register user; routes to least-loaded backend. |
| `POST` | `/api/backends`                                         | Backend self-registration at startup.          |
| `POST` | `/api/update`                                           | Update `username → back_domain` mapping.       |

### Admin endpoints

| Method   | Path                    | Description                   |
|----------|-------------------------|-------------------------------|
| `GET`    | `/api/admin/users`      | List users.                   |
| `POST`   | `/api/admin/users`      | Create user (admin override). |
| `PATCH`  | `/api/admin/users/{id}` | Suspend/restore, set role.    |
| `DELETE` | `/api/admin/users/{id}` | Delete user.                  |

### Public/auth endpoints

| Method | Path                           | Description                                     |
|--------|--------------------------------|-------------------------------------------------|
| `POST` | `/api/auth/login`              | Login (username + password).                    |
| `POST` | `/api/auth/refresh`            | Refresh access token.                           |
| `POST` | `/api/auth/logout`             | Revoke session.                                 |
| `GET`  | `/api/auth/me`                 | Current user profile (user JWT required).       |
| `GET`  | `/api/public/users/{username}` | Public profile lookup.                          |
| `POST` | `/api/public/users`            | Register user (only when `USE_RESOLVER=false`). |

### Authenticated user endpoints (`/api/authenticated/*`)

**Users & settings**

| Method  | Path                          | Description                                  |
|---------|-------------------------------|----------------------------------------------|
| `PATCH` | `/api/authenticated/users/me` | Update profile.                              |
| `GET`   | `/api/authenticated/settings` | Get user settings.                           |
| `PATCH` | `/api/authenticated/settings` | Update settings. Body: `{ versioning_mode }` |

**Pictures — upload**

| Method | Path                                                        | Description                                                                                                                                                                                    |
|--------|-------------------------------------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `POST` | `/api/authenticated/pictures/uploads`                       | Begin upload. Returns `{ picture_id, presigned_url }` (staging bucket).                                                                                                                        |
| `POST` | `/api/authenticated/pictures/uploads/{picture_id}/complete` | Confirm upload. Optional body: `{ mime_type, file_size, width, height, ... }`. Enqueues a `gen_thumbnail` job; picture row, version record, and job are created atomically in one transaction. |

**Pictures — list & details**

| Method | Path                                   | Description                                                                                                                                                                     |
|--------|----------------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `GET`  | `/api/authenticated/pictures`          | Paginated list. Query params: `page`, `page_size`, `sort`, `order`, `tag`, `owned_only`, `shared_with_me`, `include_deleted`, `captured_after`, `captured_before`, `thumbnail`. |
| `GET`  | `/api/authenticated/pictures/{id}`     | Full picture details + version history.                                                                                                                                         |
| `GET`  | `/api/authenticated/pictures/{id}/url` | Presigned URL for a variant. Query: `variant=original\|small\|medium\|large`.                                                                                                   |

**Pictures — editing**

| Method | Path                                    | Description                                                                                                          |
|--------|-----------------------------------------|----------------------------------------------------------------------------------------------------------------------|
| `POST` | `/api/authenticated/pictures/{id}/edit` | Enqueue an `edit_picture` job. Body: `{ exif_overrides?, visual?, regenerate_thumbnails }`. Only for owned pictures. |
| `GET`  | `/api/authenticated/pictures/{id}/jobs` | List all processing jobs for a picture.                                                                              |

**Jobs**

| Method | Path                           | Description                                           |
|--------|--------------------------------|-------------------------------------------------------|
| `GET`  | `/api/authenticated/jobs/{id}` | Get the status and result of a job (owned by caller). |

**Tags**

| Method  | Path                      | Description                                                                                                            |
|---------|---------------------------|------------------------------------------------------------------------------------------------------------------------|
| `GET`   | `/api/authenticated/tags` | List all tag paths used by this user.                                                                                  |
| `PATCH` | `/api/authenticated/tags` | Batch edit tags. Body: `{ picture_ids, add_tags, remove_tags }`. Applies add/remove atomically to all listed pictures. |

**Tagging pipeline**

| Method   | Path                                                      | Description                                                                                                           |
|----------|-----------------------------------------------------------|-----------------------------------------------------------------------------------------------------------------------|
| `GET`    | `/api/authenticated/tagging-services`                     | List all tagging services with their embedded rules.                                                                  |
| `POST`   | `/api/authenticated/tagging-services`                     | Create a tagging service. Body: `{ service_type, requires?, excludes? }`.                                             |
| `GET`    | `/api/authenticated/tagging-services/{id}`                | Get a specific service with its rules.                                                                                |
| `PATCH`  | `/api/authenticated/tagging-services/{id}`                | Update a service. Body: `{ enabled?, requires?, excludes? }`. Omitted fields are unchanged.                           |
| `DELETE` | `/api/authenticated/tagging-services/{id}`                | Delete a service (cascades to all its rules).                                                                         |
| `POST`   | `/api/authenticated/tagging-services/{id}/mappings`       | Add a mapping rule (shared\_tag\_mapping only). Body: `{ incoming_share_id, assign_tag }`.                            |
| `DELETE` | `/api/authenticated/tagging-services/{id}/mappings/{rid}` | Delete a mapping rule.                                                                                                |
| `POST`   | `/api/authenticated/tagging-services/{id}/rules`          | Add a predicate rule (rule type only). Body: `{ predicate, assign_tag }`.                                             |
| `DELETE` | `/api/authenticated/tagging-services/{id}/rules/{rid}`    | Delete a predicate rule.                                                                                              |
| `POST`   | `/api/authenticated/tagging-services/{id}/segments`       | Add a date-range segment (segmentation only). Body: `{ name, date_start, date_end, assign_tag, parent_segment_id? }`. |
| `DELETE` | `/api/authenticated/tagging-services/{id}/segments/{sid}` | Delete a segment (cascades to child segments).                                                                        |

**Sharing**

| Method | Path                                             | Description                                                                                 |
|--------|--------------------------------------------------|---------------------------------------------------------------------------------------------|
| `POST` | `/api/authenticated/shares/outgoing`             | Create outgoing share.                                                                      |
| `GET`  | `/api/authenticated/shares/outgoing`             | List outgoing shares.                                                                       |
| `POST` | `/api/authenticated/shares/outgoing/{id}/revoke` | Revoke an outgoing share. Notifies the recipient; removes their tags and received pictures. |
| `GET`  | `/api/authenticated/shares/incoming`             | List incoming shares.                                                                       |
| `POST` | `/api/authenticated/shares/incoming/{id}/accept` | Accept incoming share (`pending → active`).                                                 |
| `POST` | `/api/authenticated/shares/incoming/{id}/reject` | Reject incoming share (`pending/active → tombstoned`).                                      |

### Federation endpoints

| Method | Path                                | Description                                                                                                                                                                             |
|--------|-------------------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `POST` | `/api/federation/auth/request`      | Request a federation JWT.                                                                                                                                                               |
| `POST` | `/api/federation/auth/grant`        | Receive a federation JWT from another instance.                                                                                                                                         |
| `POST` | `/api/federation/shares/announce`   | Share announcement. Requires federation JWT.                                                                                                                                            |
| `POST` | `/api/federation/shares/accept`     | Recipient notifies sender that a share was accepted. Sender responds by announcing current pictures. Requires federation JWT.                                                           |
| `POST` | `/api/federation/shares/revoke`     | Share revocation. Body: `{ outgoing_share_id }`. Requires federation JWT.                                                                                                               |
| `POST` | `/api/federation/pictures/announce` | Announce pictures for an active share. Only accepted when `IncomingShare.status == active`. Requires federation JWT.                                                                    |
| `POST` | `/api/federation/pictures/presign`  | Request presigned URLs for a batch of pictures. Auth: `share_token` only — no JWT required. Body: `{ owner_username, owner_instance, share_token, pictures: [{picture_id, variant}] }`. |

### Worker endpoints (`/api/worker/*`)

Auth: `Authorization: Bearer <worker_jwt>` — short-lived JWT (HS256, 300 s TTL) signed with `WORKER_JWT_SECRET` (`token_type: worker`). Workers cache
the token and refresh 30 s before expiry.

| Method | Path                             | Description                                                                                                           |
|--------|----------------------------------|-----------------------------------------------------------------------------------------------------------------------|
| `GET`  | `/api/worker/jobs/next`          | Atomically claim next pending job. Returns job + presigned S3 URLs + `claim_token`, or `null`. Query: `types=…`.      |
| `POST` | `/api/worker/jobs/{id}/complete` | Report success. Backend applies picture updates and marks job `completed` in one transaction. Requires `claim_token`. |
| `POST` | `/api/worker/jobs/{id}/fail`     | Report failure. Auto-retries up to `max_retries` (default 3) unless `permanent: true`. Requires `claim_token`.        |

Wire shapes are defined in `archypix-common/transfer.rs`. See `04_WORKER_ARCHITECTURE.md` for the claim-token protocol and job loop.

## 6) Key flows

### Picture upload

1. Client → `POST /uploads` → gets `{ picture_id, presigned_url }` (staging bucket).
2. Client → MinIO: `PUT` binary to presigned URL.
3. Client → `POST /uploads/{id}/complete` → backend: copies staging → pictures bucket (+ versions bucket if versioning enabled); **single DB
   transaction** creates `pictures` row, `picture_versions` row, and `gen_thumbnail` job.
4. Worker claims job, processes the original (EXIF, thumbnails, BlurHash, SHA-256), and calls `POST /api/worker/jobs/{id}/complete`. Backend updates
   the picture row and marks the job done in one transaction; rejects on `claim_token` mismatch (409).

S3 keys: `{user_id}/{picture_id}` for originals/thumbnails; `{user_id}/{picture_id}/{version_id}` for versions. Keys are never stored — derived on
demand.

### Federation handshake

1. Alice’s backend resolves Bob's backend url via WebFinger.
2. Alice’s backend requests a Federation JWT to Bob’s backend at `POST /api/federation/auth/request`.
3. Bob’s backend resolves Alice’s backend via WebFinger.
4. Bob’s backend sends a JWT to Alice’s backend at `POST /api/federation/auth/grant`.

### Federation share announce

1. Alice creates `OutgoingShare` (`status = pending`). The `OutgoingShare` insert and the federation delivery run in a single transaction: if the
   federation call fails the insert is rolled back.
   - **Same-backend** (`recipient_instance == global_domain`): `IncomingShare` is created in the same transaction (`status = pending`); no HTTP
     federation.
   - **Cross-instance**: federation handshake (or JWT from cache), then `POST /api/federation/shares/announce` to Bob’s backend. Bob’s backend creates
     `IncomingShare` (`status = pending`).
2. Bob accepts the share via `POST /api/authenticated/shares/incoming/{id}/accept`. Bob’s backend **immediately transitions `IncomingShare`
   to `active`** (Bob’s consent), then handles delivery:
   - **Same-backend**: Alice’s pictures under the tag are queried locally; received-picture rows + `/SharedToMe/…` tags are created in a single
     transaction. Alice’s `OutgoingShare` is also transitioned to `active`.
   - **Cross-instance**: sends `POST /api/federation/shares/accept` to Alice’s backend.
3. Alice’s backend (on receiving accept): transitions `OutgoingShare` to `active`, queries her owned pictures under the shared tag, sends
   `POST /api/federation/pictures/announce` to Bob.
4. Bob’s `announce_pictures` handler registers each announced picture (`PictureRepository::create_received`) and assigns the
   `/SharedToMe/alice_AT_instance_DOT_com/…` tag (`source = incoming_share`). It only accepts `active` shares — `pending` shares are rejected (
   prevents picture injection into unaccepted shares).
5. When Bob accesses a picture, `presign_for_picture` checks Redis cache first, then:
    - **Same-backend owner**: derives S3 key from `remote_picture_id` and owner’s local `user_id`.
    - **Cross-instance owner**: looks up `origin_share_token` (cached in Redis), calls `POST /api/federation/pictures/presign` on Alice’s backend.

### Federation share revocation

1. Alice calls `POST /api/authenticated/shares/outgoing/{id}/revoke`. Alice’s backend sets `OutgoingShare` status to `revoked` and notifies the
   recipient.
   - **Same-backend**: directly removes `/SharedToMe/…` tags, deletes unreachable received pictures, sets `IncomingShare` status to `revoked`,
     invalidates Redis presign-token cache.
   - **Cross-instance**: sends `POST /api/federation/shares/revoke` (keyed by `outgoing_share_id`) to Bob’s backend, which performs the same cleanup
     locally.
2. Bob’s backend propagates revocation downstream to any transitive recipients.
