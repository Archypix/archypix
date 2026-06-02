> **Maintenance notice** — Do not add more details on the work you did compared to the existing documentation. The same level of precision and depth
> must be maintained in this document.

- Resolver (Rust service)
    - Purpose: map username → owning backend domain (implements WebFinger). Enables multiple backends to share one global identity domain.
  - Roles:
      - WebFinger endpoint: answer `/.well-known/webfinger` requests with the resolved backend URL.
      - User registration routing: `POST /api/register` — picks least-loaded backend, forwards registration, stores `username → back_domain` mapping.
      - Backend self-registration: `POST /api/backends` — backends call this at startup; the resolver stores `back_domain`, `use_https`, and
        `internal_url`.
      - Mapping update: `POST /api/update` — called by backends when a user migrates to another instance.
    - Key env vars: `GLOBAL_DOMAIN`, `RESOLVER_JWT_SECRET`, `DB_HOST/DB_USER/DB_PASSWORD/DB_NAME`.

- Backend (Rust backend instance, per domain)
    - Purpose: authoritative per-instance application server and metadata store.
    - Roles:
        - HTTP API & WebDAV: serve user requests, uploads, sync client endpoints.
      - WebFinger client: consult resolver/WebFinger when needed for cross-instance discovery; caches backend domains in Redis.
      - Postgres: authoritative metadata (users, pictures, tags, shares, jobs). Key picture columns include `file_hash` (SHA-256, WebDAV ETag) and
        `file_size` (kept accurate after every worker processing run).
      - Federation endpoints: handle inbound/outbound federation messages (share announce/revoke, presign requests).
      - Job queue owner: writes `pending` jobs to the `jobs` table; exposes `/api/worker/*` endpoints for workers to claim and complete them. Issues a
        one-time `claim_token` per claim to prevent stale workers from corrupting re-claimed jobs.
      - In-process task queue: lightweight Tokio-based queue (`infra/tasks.rs`) for DB-only tasks (tag-rename cascade, tagging pipeline evaluation)
        that do not require external compute.
      - Job watchdog: background Tokio task (`infra/job_watchdog.rs`) that periodically resets jobs stuck in `processing` for longer than
        `JOB_PROCESSING_TIMEOUT_SECS` (default 600 s), incrementing `retry_count` and clearing `claim_token`.
      - Local caches: Redis for sessions, presigned URLs, federation tokens, and backend domain mappings.

- Workers (`archypix-worker`, one or more Rust processes)
    - Purpose: perform CPU/GPU-intensive work; never access the database or S3 directly.
  - Job transport: **Postgres-backed queue** (`SELECT FOR UPDATE SKIP LOCKED`). Workers poll `GET /api/worker/jobs/next`; the backend returns a job
    with presigned S3 URLs and a one-time `claim_token`. Workers echo the token in every `complete`/`fail`; mismatches are rejected (409).
  - S3 access: exclusively via presigned URLs.
  - Auth: short-lived JWT (`WORKER_JWT_SECRET`), cached in-process.
    - Implemented job types:
        - `gen_thumbnail` — download original, extract EXIF, compute BlurHash + SHA-256 `file_hash`, generate small/medium/large WebP thumbnails,
          upload, report `exif`/`blurhash`/`file_size`/`file_hash` to backend.
        - `edit_picture` — download original, apply EXIF overrides, compute `file_size`/`file_hash`, upload, optionally regenerate thumbnails.
  - Stub job types (infrastructure ready, not yet implemented): `ml_style`, `ml_people`, `ml_group_location`.
  - Completion: backend applies picture updates + marks job done in one transaction. Auto-retries up to `max_retries` (default 3) on failure.

- MinIO (S3-compatible object storage)
    - Purpose: durable blob store for original images, derivatives, version snapshots, and exports.
    - Buckets: staging (short-lived; auto-expires via lifecycle rule), pictures (confirmed originals), versions (version snapshots),
      small/medium/large (thumbnails).
    - S3 keys are derived deterministically and never stored in the database:
        - Originals and thumbnails: `{user_id}/{picture_id}`.
        - Version snapshots: `{user_id}/{picture_id}/{version_id}`. The `version_id` is generated before the S3 copy and used as the
          `picture_versions.id`.
    - Three S3 endpoint slots: `S3_ENDPOINT` for server-side operations; `S3_PUBLIC_ENDPOINT` for presigned URLs returned to browsers;
      `S3_WORKERS_ENDPOINT` for presigned URLs returned to workers (defaults to `S3_ENDPOINT`). Allows Docker networking where internal and external
      addresses differ.

- Frontend (static CDN + clients)
    - Single static site served from CDN; no per-instance build.
    - Discovery: resolve `@username:domain` → backend URL via WebFinger before making API calls.
    - All API and WebDAV calls go to the resolved backend for that user.

**Invariants**

- Each backend is authoritative for its users (Postgres is the single source of truth per instance).
- Workers publish results; backends persist — workers never write to backend databases or S3 directly.
- All persistent storage (DB fields, JWT claims, federation messages) uses the **global domain**. Backend domains are resolved on demand via WebFinger
  and cached in Redis.
- Job queue transport is Postgres (`SELECT FOR UPDATE SKIP LOCKED`). Workers are stateless HTTP clients that need only a backend URL and
  `WORKER_JWT_SECRET` to operate.
- The `claim_token` protocol prevents stale workers (reset by the watchdog) from overwriting the results of a re-claimed job.
- S3 keys for originals/thumbnails are derived as `{user_id}/{picture_id}`; version keys include the `version_id` UUID that matches the
  `picture_versions.id` DB column.
