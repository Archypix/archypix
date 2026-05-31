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
- Multi-step workflows (user creation, picture upload, share creation) run in an explicit SQL transaction managed by the service.
- API handlers call repositories directly only for single-step CRUD with no orchestration.

## C) Module layout (`back/src/`)

```
main.rs / state.rs

domain/
  auth.rs           # TokenType, JwtClaims
  user.rs / user_settings.rs
  picture.rs        # Picture, PictureVersion, UploadSession
  tag.rs            # TagPath (newtype), TagSource, Tag
  share.rs          # OutgoingShare, IncomingShare
  federation.rs     # FederationMessage, BackendMapping
  job.rs
  tagging.rs / tagging/pipeline.rs   # pipeline config types + pure evaluator

repository/
  user.rs / picture.rs / picture_version.rs / user_settings.rs
  tag.rs / share.rs / auth.rs

clients/
  federation.rs     # WebFinger resolution, token lifecycle, federation calls
  resolver.rs       # self_register, update_mapping, verify_token

services/
  auth.rs / users.rs / pictures.rs / user_settings.rs / shares.rs

api/
  middleware/auth_user.rs / auth_admin.rs / auth_resolver.rs / auth_federation.rs
  user/auth.rs / users.rs / pictures.rs / settings.rs / shares.rs / tags.rs
  admin/handlers.rs + models.rs
  federation/handlers.rs + models.rs
  resolver/handlers.rs + models.rs

infra/
  config.rs / error.rs / redis.rs / crypto.rs / db.rs / s3.rs
```

## D) AppState

```rust
pub struct AppState {
    pub config: Config,
    pub db: PgPool,
    pub redis: RedisClient,
    pub jwt: JwtService,
    pub storage: StorageClient,
    pub federation: FederationClient,
    pub resolver: ResolverClient,
}
```

# Backend REST API

## 1) API layout

| Section                      | Base path                      | Auth                      |
|------------------------------|--------------------------------|---------------------------|
| Resolver endpoints           | `/api/resolver/*`              | Resolver JWT              |
| Admin endpoints              | `/api/admin/*`                 | User JWT with `is_admin`  |
| Public/auth endpoints        | `/api/auth/*`, `/api/public/*` | Mixed                     |
| Authenticated user endpoints | `/api/authenticated/*`         | User JWT                  |
| Federation endpoints         | `/api/federation/*`            | Federation JWT (pairwise) |

## 2) Domain terminology

| Term               | Env var         | Example                | Description                                                                                                     |
|--------------------|-----------------|------------------------|-----------------------------------------------------------------------------------------------------------------|
| **Global domain**  | `GLOBAL_DOMAIN` | `example.com`          | Public identity domain. Used in `@user:example.com`, JWTs, DB, federation. Never changes from user perspective. |
| **Backend domain** | `BACK_DOMAIN`   | `backend1.example.com` | Actual API server. Resolved via WebFinger, cached in Redis. Never stored persistently.                          |

All persistent storage uses the **global domain**. Backend domains are derived on demand and cached.

## 3) JWT tokens

| Claim        | Description                                                               |
|--------------|---------------------------------------------------------------------------|
| `sub`        | Username (user tokens) or global domain (federation tokens).              |
| `uid`        | User UUID (user tokens only).                                             |
| `is_admin`   | Boolean. Admin endpoints check this, not a separate token type.           |
| `instance`   | Global domain of the issuing instance.                                    |
| `token_type` | `user` \| `resolver` \| `federation`. There is no `admin` token type.     |
| `aud`        | Backend domain of the verifying instance (checked against `BACK_DOMAIN`). |

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

| Method | Path                                                        | Description                                                                   |
|--------|-------------------------------------------------------------|-------------------------------------------------------------------------------|
| `POST` | `/api/authenticated/pictures/uploads`                       | Begin upload. Returns `{ picture_id, presigned_url }` (staging bucket).       |
| `POST` | `/api/authenticated/pictures/uploads/{picture_id}/complete` | Confirm upload. Optional body: `{ mime_type, file_size, width, height, ... }` |

**Pictures — list & details**

| Method | Path                                   | Description                                                                                                                                                                     |
|--------|----------------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `GET`  | `/api/authenticated/pictures`          | Paginated list. Query params: `page`, `page_size`, `sort`, `order`, `tag`, `owned_only`, `shared_with_me`, `include_deleted`, `captured_after`, `captured_before`, `thumbnail`. |
| `GET`  | `/api/authenticated/pictures/{id}`     | Full picture details + version history.                                                                                                                                         |
| `GET`  | `/api/authenticated/pictures/{id}/url` | Presigned URL for a variant. Query: `variant=original\|small\|medium\|large`.                                                                                                   |

**Tags**

| Method  | Path                      | Description                                                                                                            |
|---------|---------------------------|------------------------------------------------------------------------------------------------------------------------|
| `GET`   | `/api/authenticated/tags` | List all tag paths used by this user.                                                                                  |
| `PATCH` | `/api/authenticated/tags` | Batch edit tags. Body: `{ picture_ids, add_tags, remove_tags }`. Applies add/remove atomically to all listed pictures. |

**Sharing**

| Method | Path                                             | Description            |
|--------|--------------------------------------------------|------------------------|
| `POST` | `/api/authenticated/shares/outgoing`             | Create outgoing share. |
| `GET`  | `/api/authenticated/shares/outgoing`             | List outgoing shares.  |
| `GET`  | `/api/authenticated/shares/incoming`             | List incoming shares.  |
| `POST` | `/api/authenticated/shares/incoming/{id}/accept` | Accept incoming share. |
| `POST` | `/api/authenticated/shares/incoming/{id}/reject` | Reject incoming share. |

### Federation endpoints

| Method | Path                                | Description                                                        |
|--------|-------------------------------------|--------------------------------------------------------------------|
| `POST` | `/api/federation/auth/request`      | Request a federation JWT.                                          |
| `POST` | `/api/federation/auth/grant`        | Receive a federation JWT from another instance.                    |
| `POST` | `/api/federation/shares/announce`   | Share announcement. Requires federation JWT.                       |
| `POST` | `/api/federation/shares/revoke`     | Share revocation. Requires federation JWT.                         |
| `POST` | `/api/federation/pictures/announce` | Announce pictures for an active share. Requires federation JWT.    |
| `POST` | `/api/federation/pictures/presign`  | Request presigned URL. Auth: `share_token` only — no JWT required. |

## 6) Key flows

### Picture upload

1. Client → `POST /uploads` → gets `{ picture_id, presigned_url }` (staging bucket).
2. Client → MinIO: `PUT` binary to presigned URL.
3. Client → `POST /uploads/{id}/complete` → backend server-copies staging → pictures bucket, optionally versions, inserts DB row.
4. (Async) Backend enqueues job → Worker generates thumbnails + ML metadata → results persisted.

S3 keys are derived as `{user_id}/{picture_id}` and never stored in the database.

### Federation share announce

1. Alice creates `OutgoingShare`; backend federates the announcement to Bob's backend.
2. Bob's backend creates `IncomingShare` + `/SharedToMe/alice@instance.com/...` tags on each announced picture.
3. When Bob accesses a picture, his backend resolves Alice's backend (WebFinger, cached) and calls `POST /api/federation/pictures/presign` with the
   `share_token`. Alice's backend returns a presigned S3 URL; Bob's backend caches it and returns it to the client. The actual blob is fetched
   directly from Alice's S3.

### Federation share revocation

1. Alice's backend sends revocation to Bob's backend.
2. Bob's backend tombstones `IncomingShare`, marks `/SharedToMe/...` tags broken.
3. Bob's backend propagates revocation downstream to any transitive recipients.

## 7) Not-yet-developed items

1. Full tagging pipeline execution (`services/tagging.rs`).
2. Federation token rotation schedule and retry logic.
3. Redis-backed rate limiting and session invalidation.
4. NATS JetStream job publishing and result consumption.
5. WebDAV implementation (presigned redirect for reads, staging pattern for writes).
6. Worker-driven metadata backfill after thumbnail/ML processing.
7. Admin job status and instance metrics endpoints.
