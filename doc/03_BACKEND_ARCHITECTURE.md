# Backend Architecture

## A) Technology considerations

### Framework Choice: Axum
- Already proven in resolver component
- Excellent async/await support with Tokio
- Robust routing, middleware (via Tower), and error handling
- Consistent codebase across resolver and backend
- Good performance and scalability for microservices

### Database Access: SQLx
- Excellent PostgreSQL feature support (LTREE, JSONB, custom types, etc.)
- Compile-time checked SQL with macros
- Direct SQL control for performance optimization
- Team familiarity from resolver implementation
- Migration capabilities already in use
- Reduced abstraction overhead compared to ORMs

---

## B) Layered architecture and responsibilities

**Goal:** clean separation between HTTP API, business workflows, domain rules, database access, and infrastructure connectivity.

| Layer        | Responsibility                                                                                             | Can depend on                                                | Must NOT depend on                              |
|--------------|------------------------------------------------------------------------------------------------------------|--------------------------------------------------------------|-------------------------------------------------|
| `api`        | HTTP handlers, auth extraction, request/response models. Calls repositories directly for single-step CRUD. | `services`, `repository`, `domain`, `infra::error`           | External connectivity details outside AppState. |
| `services`   | Multi-step workflows and orchestration. Owns transaction boundaries.                                       | `repository`, `clients`, `domain`, `infra`                   | Axum request types, HTTP-specific models.       |
| `clients`    | Outbound HTTP adapters. Wraps calls to external systems (federation backends, resolver, S3).               | `infra`, `domain`                                            | `services`, `repository`, `api`.                |
| `repository` | SQL operations only — no business logic.                                                                   | `domain` (types), `infra::error`                             | `services`, `clients`.                          |
| `domain`     | Business types, invariants, pure transformations, and the tagging pipeline evaluator.                      | std + lightweight crates (serde, uuid, chrono, sqlx derives) | `repository`, `infra`, external clients.        |
| `infra`      | Raw connectivity primitives: config, error, Redis, S3, crypto (JWT, hashing).                              | External SDKs                                                | `api`, `services`, `clients`.                   |
| `state`      | `AppState` — application bootstrap, holds all composed clients and infra handles.                          | `infra`, `clients`                                           | `services`, `repository`, `api`.                |

**Key rules:**

- Services are oriented by **business capability** (`users`, `auth`, `pictures`, `shares`), not by API emitter.
- All repository functions accept `Executor<'e, Database = Postgres>` so they run on either a `PgPool` or a transaction.
- Multi-step workflows (user creation, picture upload, share creation) **must** run in an explicit SQL transaction managed by a service.
- API handlers call repositories directly **only** for single-step CRUD with no orchestration.

### Dependency graph

```
api       → services, repository, domain, infra
services  → repository, clients, domain, infra
clients   → infra, domain
repository→ domain, infra
domain    → (std, serde, uuid, chrono, sqlx derives only)
infra     → (external crates)
state     → infra, clients        ← no cycle: clients depend on infra only
```

`AppState` lives in the top-level `state` module rather than inside `infra` to avoid a circular dependency (`infra → clients → infra`).

---

## C) Module layout (`back/src/`)

Rust's file-based module convention is followed throughout: a module `foo` with submodules lives at `foo.rs` (the parent file) + `foo/` (the submodule
directory). No `mod.rs` files are used.

```
back/src/
  main.rs              # Bootstrap: build AppState, start Axum server
  state.rs             # AppState definition

  domain.rs            # pub mod declarations
  domain/
    auth.rs            # TokenType, JwtClaims
    user.rs            # User, UserCredential, RefreshToken
    user_settings.rs   # UserSettings, VersioningMode enum
    picture.rs         # Picture, PictureVersion, UploadSession
    tag.rs             # TagPath (newtype), TagSource, Tag
    share.rs           # ShareStatus, OutgoingShare, IncomingShare
    federation.rs      # FederationMessage, direction/status enums, BackendMapping
    job.rs             # Job, JobStatus, JobType
    tagging.rs         # TaggingService config types, Hierarchy; declares tagging/
    tagging/
      pipeline.rs      # Pure pipeline evaluator (no I/O)

  repository.rs
  repository/
    user.rs            # UserRepository
    picture.rs         # PictureRepository + PictureListFilter/SortField/SortOrder
    picture_version.rs # PictureVersionRepository
    user_settings.rs   # UserSettingsRepository (get_or_default, upsert)
    tag.rs             # TagRepository
    share.rs           # OutgoingShareRepository, IncomingShareRepository
    auth.rs            # CredentialRepository, RefreshTokenRepository

  clients.rs
  clients/
    federation.rs      # FederationClient (WebFinger, token lifecycle, announce_share)
    resolver.rs        # ResolverClient (self_register, update_mapping, verify_token)

  services.rs
  services/
    auth.rs            # login(), refresh(), logout()
    users.rs           # create_user()
    pictures.rs        # begin_upload(), complete_upload(), list_pictures(), presign_picture_variant()
                       # + PictureVariant (original|small|medium|large), UploadMetadata, PictureListParams
    user_settings.rs   # get(), update()
    shares.rs          # create_outgoing_share()

  api.rs               # Router composition
  api/
    middleware.rs      # bearer_token() helper
    middleware/
      auth_user.rs     # AuthUser extractor (user/admin JWT)
      auth_admin.rs    # AuthAdmin extractor (admin JWT)
      auth_resolver.rs # AuthResolver extractor (resolver JWT)
      auth_federation.rs # AuthFederation extractor (federation JWT)
    user.rs            # auth_routes(), public_routes(), authenticated_routes()
    user/
      auth.rs          # login, refresh, logout, me handlers
      users.rs         # register, get_public, update_me handlers
      pictures.rs      # create_upload, complete_upload, list, details, picture_url handlers
      settings.rs      # get_settings, update_settings handlers
      shares.rs        # share create/list/accept/reject handlers
      tags.rs          # tag list/assign/remove handlers
    admin.rs           # routes()
    admin/
      handlers.rs
      models.rs        # CreateUserRequest, UpdateUserRequest, UserResponse
    federation.rs      # routes()
    federation/
      handlers.rs
      models.rs        # FederationAuthRequest, ShareRevokeRequest, PresignRequest
    resolver.rs        # routes()
    resolver/
      handlers.rs
      models.rs

  infra.rs
  infra/
    config.rs          # Config (loaded from env)
    error.rs           # AppError, map_sqlx_error
    redis.rs           # RedisClient, connect()
    crypto.rs          # JwtService, hash_password(), verify_password(),
                       # generate_refresh_token(), hash_refresh_token()
    db.rs              # connect(), run_migrations()
    s3.rs              # StorageClient (presign_get/put/copy/delete), connect(),
                       # picture_key(), version_key(), bucket setup + lifecycle rule
```

---

## D) What belongs where — decision guide

| Code                                                 | Layer                           | Reasoning                                                                                                         |
|------------------------------------------------------|---------------------------------|-------------------------------------------------------------------------------------------------------------------|
| `TokenType`, `JwtClaims`                             | `domain/auth.rs`                | Pure value types, no I/O                                                                                          |
| `TagPath` newtype + methods                          | `domain/tag.rs`                 | Business invariants, pure                                                                                         |
| `OutgoingShare::would_loop_to()`                     | `domain/share.rs`               | Domain rule, pure                                                                                                 |
| Pipeline evaluator                                   | `domain/tagging/pipeline.rs`    | Pure business logic, testable without I/O                                                                         |
| SQL queries                                          | `repository/`                   | SQL only, no business logic                                                                                       |
| WebFinger resolution + global→backend domain mapping | `clients/federation.rs`         | Outbound HTTP adapter; caches backend domains in Redis                                                            |
| Login/refresh workflow                               | `services/auth.rs`              | Multi-step: DB + crypto + token generation                                                                        |
| Upload workflow                                      | `services/pictures.rs`          | Multi-step: Redis session + S3 presign (staging) + server-side copy to pictures + optional versioning + DB record |
| User settings read/update                            | `services/user_settings.rs`     | DB get-or-default + upsert; not embedded in JWT (changes too frequently)                                          |
| Share creation                                       | `services/shares.rs`            | Multi-step: DB + federation announce                                                                              |
| JWT signing keys, argon2 hashing                     | `infra/crypto.rs`               | Infrastructure crypto primitives                                                                                  |
| S3 presigned URLs                                    | `infra/s3.rs` → `StorageClient` | Infrastructure adapter                                                                                            |
| HTTP handler                                         | `api/*/`                        | Thin: extract input → call service/repo → serialize output                                                        |

---

## E) Transactions and database access

**Rule:** all repository functions accept `Executor<'e, Database = Postgres>`:

```rust
pub async fn create<'e, E>(ex: E, ...) -> Result<Entity, AppError>
where
    E: Executor<'e, Database = Postgres>,
```

This allows calling them on either `&PgPool` (single-step handlers) or `&mut PgTransaction` (multi-step services):

```rust
// services/users.rs — transaction managed by the service
let mut tx = db.begin().await?;
let user = UserRepository::create( & mut * tx,...).await?;
CredentialRepository::upsert_password( & mut * tx, user.id, & hash).await?;
tx.commit().await?;
```

---

## F) AppState

`AppState` is defined in `state.rs` at the crate root. It holds all composed infrastructure handles — no business logic lives here.

```rust
pub struct AppState {
    pub config: Config,
    pub db: PgPool,
    pub redis: RedisClient,
    pub jwt: JwtService,        // user/admin/federation token verification
    pub storage: StorageClient, // S3 presigned URL operations
    pub federation: FederationClient, // outbound federation HTTP
    pub resolver: ResolverClient,     // outbound resolver HTTP
}
```

`FederationClient` and `ResolverClient` are constructed once in `main.rs` and cloned into each handler via Axum's `State` extractor.

---

# Backend REST API Structure

## 1) API layout and base paths

The router is composed in `src/api.rs` (no `routes.rs` files).

| Section                      | Base path                      | Auth                                    | Purpose                                                                    |
|------------------------------|--------------------------------|-----------------------------------------|----------------------------------------------------------------------------|
| Resolver endpoints           | `/api/resolver/*`              | JWT signed with `RESOLVER_ADMIN_SECRET` | Endpoints called by the Resolver to create users when `USE_RESOLVER=true`. |
| Admin endpoints              | `/api/admin/*`                 | Admin JWT                               | Instance-level operations.                                                 |
| Public/auth endpoints        | `/api/auth/*`, `/api/public/*` | Mixed                                   | Login/refresh and public lookups.                                          |
| Authenticated user endpoints | `/api/authenticated/*`         | User JWT                                | Main user API (pictures, tags, shares).                                    |
| Federation endpoints         | `/api/federation/*`            | Federation JWT (pairwise)               | Cross-instance messaging.                                                  |

## 2) Domain terminology

Two domain concepts are used throughout the system:

| Term               | Env var         | Example                | Description                                                                                                                                              |
|--------------------|-----------------|------------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------|
| **Global domain**  | `GLOBAL_DOMAIN` | `example.com`          | Public identity domain. Used in `@user:example.com`, stored in JWTs, database, and all federation messages. Never changes from the user's perspective.   |
| **Backend domain** | `BACK_DOMAIN`   | `backend1.example.com` | Actual API server domain. Resolved at request time via WebFinger. Never stored persistently — it may change if users are migrated to a different server. |

`GLOBAL_DOMAIN` does **not** need to equal `BACK_DOMAIN`. You can host your backend at `backend1.example.com` while identities are `@user:example.com`
by forwarding `/.well-known/webfinger` on the global domain to the backend via a reverse proxy — the same delegation pattern used by Matrix.

**Invariant:** all persistent storage (database fields, JWT claims) uses the **global domain**. The backend domain is derived from the global domain
on demand via WebFinger and cached in Redis.

---

## 3) JWT tokens

All auth types use JWT with a shared claim shape:

| Claim               | Description                                                                           |
|---------------------|---------------------------------------------------------------------------------------|
| `sub`               | Username (for user/admin tokens) or global domain (for federation tokens).            |
| `uid`               | User UUID (for user/admin tokens).                                                    |
| `instance`          | **Global (WebFinger) domain** of the issuing instance.                                |
| `token_type`        | `user` \| `admin` \| `resolver` \| `federation`.                                      |
| `is_admin`          | Boolean (true for admin tokens).                                                      |
| `aud`               | **Backend domain** of the verifying instance (checked locally against `BACK_DOMAIN`). |
| `exp`, `iat`, `jti` | Standard JWT lifecycle and replay protection.                                         |

The separation between `instance` (global domain) and `aud` (backend domain) means a token correctly identifies the issuing instance by its public
identity while still being verifiable by the specific backend that received it.

## 4) Federation authentication protocol (pairwise JWT)

We use a pairwise token scheme: the **recipient instance** issues a JWT to the **requesting instance**.

All domains in federation messages are **global (WebFinger) domains**. Backend domains are never included in federation messages — they are resolved
via WebFinger at request time and cached in Redis.

### Handshake

1. **Token request**  
   `A.backend -> B.backend`: `POST /api/federation/auth/request`  
   Body: `{ requester_instance (A's global domain), username (a user on A, for B to resolve A's backend), scope, nonce }`

2. **Backend resolution**  
   B resolves A's backend domain: `WebFinger(username@A_global_domain)` → `A.backend_domain`  
   (Result cached in Redis under `federation:backend:{username}@{A_global_domain}`.)

3. **Token grant**  
   `B -> A.backend` (resolved via WebFinger): `POST /api/federation/auth/grant`  
   Body: `{ issuer_instance (B's global domain), token, expires_at, scope, nonce }`

4. **Usage**  
   A stores the token in Redis under `federation:token:{B_global_domain}` and uses it for subsequent calls to B's federation endpoints.

**JWT claim values for federation tokens:**

| Claim      | Value                                   |
|------------|-----------------------------------------|
| `sub`      | Requester's global domain (A)           |
| `instance` | Issuer's global domain (B)              |
| `aud`      | Issuer's **backend** domain (B.backend) |

**Notes:**

- The grant is sent server-to-server to A's resolved backend, ensuring only the real instance receives it.
- Tokens are short-lived; re-requested as needed.
- Token lifecycle is managed by `FederationClient` (resolve, cache, request, store).
- Token cache is keyed by **global domain**, not backend domain.

## 5) Middleware stack (Axum/Tower)

| Middleware  | Applies to                           | Purpose                                               |
|-------------|--------------------------------------|-------------------------------------------------------|
| Request ID  | All                                  | Correlate logs across services.                       |
| Trace       | All                                  | Structured request logs with latency and status.      |
| Timeout     | All                                  | Bound request time (e.g., 30s).                       |
| Body limit  | Upload endpoints                     | Prevent oversized JSON payloads.                      |
| CORS        | `/api/*`                             | Browser access from origins listed in `CORS_ORIGINS`. |
| Compression | All (except uploads)                 | Reduce JSON response size.                            |
| Rate limit  | Auth + federation + public endpoints | Abuse control (Redis-backed).                         |
| Auth        | By route group                       | Enforce identity type.                                |

## 6) Endpoint layout (initial set)

### 6.1 Resolver endpoints

These endpoints are on the **backend** and are called by the Resolver service (authenticated with a resolver JWT signed by `RESOLVER_JWT_SECRET`).

| Method | Path                             | Description                                                  |
|--------|----------------------------------|--------------------------------------------------------------|
| `POST` | `/api/resolver/users`            | Create user on this backend (only when `USE_RESOLVER=true`). |
| `GET`  | `/api/resolver/users/{username}` | Fetch user by username for resolver validation.              |

### 6.1b Resolver service endpoints

These endpoints are on the **Resolver service** itself (`resolver/`, port 8080).

**Public (no auth)**

| Method | Path                                                    | Description                                                                                 |
|--------|---------------------------------------------------------|---------------------------------------------------------------------------------------------|
| `GET`  | `/.well-known/webfinger?resource=archypix:@user:domain` | Resolve username to backend URL. Returns links with `rel: backend_url`.                     |
| `POST` | `/api/register`                                         | Register a new user. Picks the least-loaded backend, forwards registration, stores mapping. |
| `GET`  | `/health`                                               | Health check.                                                                               |

**Admin (resolver JWT required)**

| Method | Path            | Description                                                                                  |
|--------|-----------------|----------------------------------------------------------------------------------------------|
| `POST` | `/api/update`   | Update a `username → back_domain` mapping (called by backends when a user changes instance). |
| `POST` | `/api/backends` | Backend self-registration at startup. Body: `{ back_domain, use_https, internal_url }`.      |
| `GET`  | `/api/backends` | List all registered backend domains.                                                         |

**Resolver database schema (backends table):**

| Column         | Type    | Description                                                                                                      |
|----------------|---------|------------------------------------------------------------------------------------------------------------------|
| `back_domain`  | VARCHAR | **Primary key.** Public domain:port, used as JWT audience.                                                       |
| `use_https`    | BOOLEAN | Whether the backend is served over HTTPS. Combined with `back_domain` to produce the URL in WebFinger responses. |
| `internal_url` | VARCHAR | URL the resolver uses internally to forward registrations (e.g. Docker hostname).                                |

The `user_mappings` table stores `back_domain` (FK → `backends.back_domain`) rather than a full URL; WebFinger responses derive the URL from
`back_domain + use_https` at query time.

**Backend self-registration:** at startup, each backend calls `POST /api/backends` with its own `back_domain`, `use_https`, and `internal_url`. The
resolver upserts the record. If no backends are registered when a user tries to register, the resolver returns 503.

**`POST /api/register` body:** `{ username, display_name, email, password }`  
**Algorithm:** queries `backends` ordered by fewest users; picks the least-loaded one; generates a resolver JWT with `aud = back_domain`; forwards to
`POST {internal_url}/api/resolver/users`; stores `username → back_domain` in `user_mappings`.

### 6.2 Admin endpoints

| Method   | Path                    | Description                   |
|----------|-------------------------|-------------------------------|
| `GET`    | `/api/admin/users`      | List users.                   |
| `POST`   | `/api/admin/users`      | Create user (admin override). |
| `PATCH`  | `/api/admin/users/{id}` | Suspend/restore, set role.    |
| `DELETE` | `/api/admin/users/{id}` | Delete user.                  |

### 6.3 Public/auth endpoints

| Method | Path                           | Description                                       |
|--------|--------------------------------|---------------------------------------------------|
| `POST` | `/api/auth/login`              | Login (username + password).                      |
| `POST` | `/api/auth/refresh`            | Refresh access token.                             |
| `POST` | `/api/auth/logout`             | Revoke session/refresh token.                     |
| `GET`  | `/api/auth/me`                 | Current user profile (requires user JWT).         |
| `GET`  | `/api/public/users/{username}` | Public profile lookup.                            |
| `POST` | `/api/public/users`            | Register user (**only if `USE_RESOLVER=false`**). |

### 6.4 Authenticated user endpoints (`/api/authenticated/*`)

**Users**

| Method  | Path                          | Description              |
|---------|-------------------------------|--------------------------|
| `PATCH` | `/api/authenticated/users/me` | Update profile/settings. |

**Settings**

| Method  | Path                          | Description                                                                                  |
|---------|-------------------------------|----------------------------------------------------------------------------------------------|
| `GET`   | `/api/authenticated/settings` | Get current user settings.                                                                   |
| `PATCH` | `/api/authenticated/settings` | Update settings. Body: `{ versioning_mode: "none" \| "original_copy" \| "full_versioning" }` |

User settings are **not** embedded in the JWT — they are loaded from the database on demand. A default row is created automatically on first access.

**Pictures — upload**

| Method | Path                                                        | Description                                                                                                                                                 |
|--------|-------------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `POST` | `/api/authenticated/pictures/uploads`                       | Begin upload. Body: `{ filename }`. Returns `{ picture_id, presigned_url }`. The `picture_id` UUID becomes the DB primary key and S3 key component.         |
| `POST` | `/api/authenticated/pictures/uploads/{picture_id}/complete` | Confirm upload. Body: all fields optional — `{ mime_type, file_size, width, height, exif_data, captured_at }`. Workers will verify/complete metadata later. |

**Pictures — list & details**

| Method | Path                                   | Description                                                                                                                                                        |
|--------|----------------------------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `GET`  | `/api/authenticated/pictures`          | Paginated picture list (see query params below).                                                                                                                   |
| `GET`  | `/api/authenticated/pictures/{id}`     | Full picture details including version history.                                                                                                                    |
| `GET`  | `/api/authenticated/pictures/{id}/url` | Get a presigned URL for a specific variant. Query param: `variant=original\|small\|medium\|large`. Returns `{ url, variant }`. URL points at `S3_PUBLIC_ENDPOINT`. |

List query parameters:

| Parameter         | Type                                           | Default       | Description                                                                                                                                                     |
|-------------------|------------------------------------------------|---------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `page`            | integer                                        | `1`           | Page number.                                                                                                                                                    |
| `page_size`       | integer (max 200)                              | `50`          | Items per page.                                                                                                                                                 |
| `sort`            | `captured_at` \| `ingested_at` \| `updated_at` | `ingested_at` | Sort field.                                                                                                                                                     |
| `order`           | `asc` \| `desc`                                | `desc`        | Sort direction.                                                                                                                                                 |
| `tag`             | ltree path string                              | —             | Filter to pictures under this tag path (inclusive descendants).                                                                                                 |
| `owned_only`      | boolean                                        | `false`       | Restrict to pictures owned by this user.                                                                                                                        |
| `shared_with_me`  | boolean                                        | `false`       | Restrict to pictures received via a share.                                                                                                                      |
| `include_deleted` | boolean                                        | `false`       | Include soft-deleted pictures.                                                                                                                                  |
| `captured_after`  | ISO-8601 datetime                              | —             | Filter by capture date (inclusive lower bound).                                                                                                                 |
| `captured_before` | ISO-8601 datetime                              | —             | Filter by capture date (inclusive upper bound).                                                                                                                 |
| `thumbnail`       | `small` \| `medium` \| `large`                 | —             | If set, each item includes a `thumbnail_url` presigned URL pointing at `S3_PUBLIC_ENDPOINT`. URLs are cached in Redis for `presign_ttl − cache_margin` seconds. |

List response: `{ total, page, page_size, items: [{ id, filename, width, height, captured_at, ingested_at, thumbnail_url? }] }`

Details response: all picture fields + `versions: [{ id, version_number, file_size, mime_type, created_at }]`

**Tags**

| Method   | Path                                    | Description                          |
|----------|-----------------------------------------|--------------------------------------|
| `GET`    | `/api/authenticated/tags`               | List tags (with ancestor expansion). |
| `POST`   | `/api/authenticated/tags`               | Assign tags (batch).                 |
| `DELETE` | `/api/authenticated/tags`               | Remove tags (batch).                 |
| `POST`   | `/api/authenticated/pictures/{id}/tags` | Assign tags to a picture.            |
| `DELETE` | `/api/authenticated/pictures/{id}/tags` | Remove tags from a picture.          |

**Sharing**

| Method | Path                                             | Description            |
|--------|--------------------------------------------------|------------------------|
| `POST` | `/api/authenticated/shares/outgoing`             | Create outgoing share. |
| `GET`  | `/api/authenticated/shares/outgoing`             | List outgoing shares.  |
| `GET`  | `/api/authenticated/shares/incoming`             | List incoming shares.  |
| `POST` | `/api/authenticated/shares/incoming/{id}/accept` | Accept incoming share. |
| `POST` | `/api/authenticated/shares/incoming/{id}/reject` | Reject incoming share. |

### 6.5 Federation endpoints

| Method | Path                                | Description                                            |
|--------|-------------------------------------|--------------------------------------------------------|
| `POST` | `/api/federation/auth/request`      | Request a federation JWT (pairwise).                   |
| `POST` | `/api/federation/auth/grant`        | Receive a federation JWT from another instance.        |
| `POST` | `/api/federation/shares/announce`   | Share announcement (includes optional `shareback_of`). |
| `POST` | `/api/federation/shares/revoke`     | Share revocation.                                      |
| `POST` | `/api/federation/pictures/announce` | Announce pictures for an active share.                 |
| `POST` | `/api/federation/pictures/presign`  | Request presigned URL from original owner backend.     |

### 6.6 WebDAV

WebDAV runs on a separate route, e.g. `/dav/*`, and uses the same user JWT auth.

---

## 7) Main flows

### 7.1 User creation (with and without resolver)

```mermaid
sequenceDiagram
    autonumber
    participant Client
    participant Resolver
    participant Backend

    alt USE_RESOLVER=true
        Client ->> Resolver: Register @user:instance.com
        Resolver ->> Backend: POST /api/resolver/users (resolver JWT)
        Backend -->> Resolver: 201 Created (user_id)
        Resolver -->> Client: Registration complete (backend chosen)
    else USE_RESOLVER=false
        Client ->> Backend: POST /api/public/users
        Backend -->> Client: 201 Created (user_id)
    end
```

### 7.2 Authentication and session flow

```mermaid
sequenceDiagram
    autonumber
    participant Client
    participant Backend
    Client ->> Backend: POST /api/auth/login
    Backend -->> Client: access_token + refresh_token
    Client ->> Backend: GET /api/auth/me (Authorization: Bearer access_token)
    Backend -->> Client: user profile
    Client ->> Backend: POST /api/auth/refresh
    Backend -->> Client: new access_token
```

### 7.3 Picture upload and tagging pipeline

**S3 key design:** Picture keys are never stored in the database. They are derived on-the-fly from `user_id` and `picture_id`:

- `{user_id}/{picture_id}` — used in every bucket (staging, pictures, small, medium, large)
- `{user_id}/{picture_id}/{version_id}` — for the versions bucket

The `picture_id` UUID is generated at `begin_upload` time and serves as both the S3 key component and the database primary key.

**Staging pattern:** Uploads go to `archypix-staging` first. `complete_upload` performs a server-side copy to `archypix-pictures` and deletes the
staging object. Staging objects never confirmed are cleaned up by a 1-day MinIO lifecycle rule applied at startup. The staging bucket name **must not
** be shared with any other bucket (enforced at startup by config validation).

**Versioning:** Controlled per-user by `versioning_mode` in `user_settings`:

- `none` — no version history (default)
- `original_copy` — the first upload is preserved in `archypix-versions` as version 1; subsequent WebDAV overwrites replace `archypix-pictures` while
  the original is kept
- `full_versioning` — every pre-overwrite state is snapshotted to `archypix-versions`

At `complete_upload` time, if versioning is enabled, the confirmed file is also copied to the versions bucket and a `picture_versions` row is
inserted.

**Metadata:** The client supplies optional metadata in `complete_upload` (`mime_type`, `file_size`, `width`, `height`, `exif_data`, `captured_at`).
Workers will verify and overwrite them during processing.

```mermaid
sequenceDiagram
    autonumber
    participant Client
    participant Backend
    participant MinIO
    participant Worker
   Client ->> Backend: POST /api/authenticated/pictures/uploads (filename)
   Backend -->> Client: { picture_id, presigned_url } (staging bucket)
   Client ->> MinIO: PUT binary file to staging (presigned, body = binary)
   Client ->> Backend: POST /uploads/{picture_id}/complete (optional metadata)
   Backend ->> MinIO: server-side copy staging → archypix-pictures
   Backend ->> MinIO: delete staging object
   opt versioning_mode != none
      Backend ->> MinIO: copy pictures → archypix-versions (version_id key)
      Backend ->> Backend: insert picture_versions row
   end
   Backend ->> Backend: insert picture row (id = picture_id)
   Backend -->> Client: Picture JSON
   Note over Backend, Worker: async — not yet implemented
   Backend ->> Worker: enqueue job (thumbnails / ML)
   Worker ->> MinIO: write small/medium/large thumbnails
   Worker ->> Backend: publish results (metadata, face embeddings, etc.)
```

**Presigned URL caching:** presigned GET URLs (list thumbnails and `GET /pictures/{id}/url`) are cached in Redis under `presign:{bucket}:{key}` for
`S3_PRESIGN_TTL_SECS − S3_PRESIGN_CACHE_MARGIN_SECS` seconds. If the TTL is ≤ 0, caching is skipped entirely. Default: 3600s − 600s = 3000s cache.

**Public vs internal S3 endpoint:** `S3_ENDPOINT` is used for all server-side S3 operations (uploads, copies, bucket management).
`S3_PUBLIC_ENDPOINT` (defaults to `S3_ENDPOINT`) is used when generating presigned URLs, so the URLs embedded in API responses are reachable by
browsers. Override when the backend reaches MinIO via an internal network address (e.g. Docker container hostname) but clients need to reach it via a
publicly exposed port.

**WebDAV presigned strategy (not yet implemented):** WebDAV clients authenticate against the backend and are unaware of MinIO. For full-resolution
reads the backend issues an HTTP redirect to a presigned GET URL. For thumbnails the backend proxies the stream. WebDAV writes go through the backend,
which handles staging and versioning transparently.

### 7.4 Federation auth handshake

All domains in federation messages are **global (WebFinger) domains**. Backend domains are never
transmitted — they are resolved via WebFinger on each side and cached in Redis.

```mermaid
sequenceDiagram
    autonumber
   participant ABack as A backend (backend.a.com)
   participant WebFingerB as B WebFinger (b.com)
   participant BBack as B backend (backend.b.com)
   participant WebFingerA as A WebFinger (a.com)
   ABack ->> WebFingerB: GET /.well-known/webfinger?resource=archypix:@bob:b.com
   WebFingerB -->> ABack: backend_url = backend.b.com
   ABack ->> BBack: POST /api/federation/auth/request<br/>{ requester_instance: "a.com", username: "alice", scope, nonce }
   BBack ->> WebFingerA: GET /.well-known/webfinger?resource=archypix:@alice:a.com
   WebFingerA -->> BBack: backend_url = backend.a.com
   BBack ->> ABack: POST /api/federation/auth/grant<br/>{ issuer_instance: "b.com", token, expires_at, scope, nonce }
   Note over ABack: stores token under federation:token:b.com
   ABack ->> BBack: POST /api/federation/shares/announce (Bearer token)
```

### 7.5 Federation: share announcement and receipt

```mermaid
sequenceDiagram
    autonumber
    participant AliceBackend
    participant BobBackend
    AliceBackend ->> BobBackend: POST /api/federation/shares/announce (signed JWT)
    BobBackend ->> BobBackend: create IncomingShare + /SharedToMe/... tags
    BobBackend -->> AliceBackend: 202 Accepted
```

### 7.6 Federation: share revocation

```mermaid
sequenceDiagram
    autonumber
    participant AliceBackend
    participant BobBackend
    participant CarolBackend
    AliceBackend ->> BobBackend: POST /api/federation/shares/revoke
    BobBackend ->> BobBackend: tombstone IncomingShare + remove /SharedToMe tags
    BobBackend ->> CarolBackend: POST /api/federation/shares/revoke
    CarolBackend ->> CarolBackend: tombstone derived IncomingShare
```

---

## 8) WebDAV + storage behavior

- WebDAV lives under `/dav/*` and maps tags to virtual directories (see hierarchy spec).
- **Full-resolution reads:** backend issues an HTTP redirect to a presigned GET URL (`archypix-pictures`). No data is proxied.
- **Thumbnail reads:** backend proxies the stream from `archypix-small/medium/large`.
- **Writes (upload/overwrite):** backend receives the file, writes to staging (`archypix-staging`), confirms, and copies to `archypix-pictures`.
  Versioning logic runs identically to the REST upload flow.
- REST clients use presigned PUT URLs for direct MinIO uploads (staging pattern), then call `complete_upload` separately.

---

## 9) Share consistency and deduplication (spec)

### 9.1 Picture identity

- **Owned picture:** `local_user_id = owner`, `owner_username/owner_instance_domain = NULL`.
- **Received picture:** `local_user_id = recipient`, `owner_username/owner_instance_domain = original owner`.
- **Global identity:** `(owner_username, owner_instance_domain, picture_id)` for received pictures.

### 9.2 Deduplication rules

When a share announcement includes picture IDs:

1. Attempt `INSERT INTO pictures … ON CONFLICT (local_user_id, picture_id) DO UPDATE` (or `DO NOTHING`).
2. On conflict, only update tags/shares for the existing row; do not overwrite storage fields.

Idempotency at the federation message level is enforced via `federation_messages.idempotency_key`.

### 9.3 Storage and access

- S3 keys are **never stored** in the database. They are always derived as `{user_id}/{picture_id}` (or `{user_id}/{picture_id}/{version_id}` for
  versions). The bucket is selected by context (pictures, small, medium, large, versions).
- The receiving backend stores no S3 reference for foreign pictures — the file lives on the owner's instance.
- To access a remote file, the receiver calls `POST /api/federation/pictures/presign` on the owner's backend, which generates and returns a
  short-lived presigned GET URL. That URL can be cached in Redis for the same duration as local presigns.

### 9.4 Transitive sharing

Transitive shares **never** re-upload or re-host blobs. Announcements always reference the original owner identity and `picture_id`. Recipients fetch
blobs directly from the original owner via presigned URLs.

### 9.5 Loop prevention

`OutgoingShare::would_loop_to(owner_instance)` detects when an announcement would loop back to the original owner. Checked before sending any
federation announcement.

---

## 10) Local dev setup (Docker)

Two compose files are provided:

- `docker-compose.dev.yml` — hot-reloading dev stack (source mounted, no rebuild needed for code changes)
- `docker-compose.yml` — production-like stack

```
docker compose -f docker-compose.dev.yml up --build
```

### Dev stack topology

| Service    | Port      | `GLOBAL_DOMAIN`  | `USE_RESOLVER` | Description                           |
|------------|-----------|------------------|----------------|---------------------------------------|
| `resolver` | 8080      | `archypix.local` | —              | WebFinger + user registration routing |
| `backend1` | 8001      | `archypix.local` | `true`         | Uses resolver (shared domain)         |
| `backend2` | 8002      | `archypix.local` | `true`         | Uses resolver (shared domain)         |
| `backend3` | 8003      | `localhost:8003` | `false`        | Standalone, self-resolves WebFinger   |
| `minio`    | 9000/9001 | —                | —              | Shared MinIO (API / console)          |
| `postgres` | 5432      | —                | —              | Single Postgres with separate DBs     |
| `redis`    | 6379      | —                | —              | Single Redis with separate DB indices |

All inter-service traffic uses HTTP (`BACK_USE_HTTPS=false`, `FEDERATION_USE_HTTPS=false`). Common settings (S3, CORS, logging) are shared via
`.env.back.common`.

**Backend self-registration:** at startup each backend (when `USE_RESOLVER=true`) calls `POST {RESOLVER_INTERNAL_URL}/api/backends` with
`{ back_domain, use_https, internal_url }`. The `internal_url` is the Docker-network URL the resolver uses to forward user registrations (e.g.
`http://backend1:8000`), which differs from the `BACK_DOMAIN` (`localhost:8001`) visible externally.

**Testing federation:** Open `front-test/index.html` in two browser tabs. Set one to `http://localhost:8001` and another to `http://localhost:8002` or
`:8003`. Use sharing endpoints to send pictures across instances. Register via the resolver to test multi-backend routing.

---

## 11) Not-yet-developed items

1. Full tagging pipeline execution in `services/tagging.rs` (domain evaluators are ready in `domain/tagging/pipeline.rs`).
2. Federation token storage rotation schedule and retry logic.
3. Redis-backed rate limits and session invalidation.
4. NATS JetStream job publishing and result consumption.
5. WebDAV implementation (redirect strategy for full-res reads, proxy for thumbnails, staging pattern for writes).
6. Picture file-update flow (WebDAV overwrite) triggering versioning: copy current `archypix-pictures` object to `archypix-versions` before overwrite,
   insert a new `picture_versions` row, then replace the object. Triggered by WebDAV `PUT` on an existing picture path.
7. Worker-driven metadata backfill: after workers generate thumbnails and run ML, they update the `pictures` row with verified `mime_type`,
   `file_size`, `width`, `height`, and `exif_data`.
8. Presigned URL caching for federation picture access (Redis TTL matching local presign caching strategy).
9. Admin job and metrics endpoints.
