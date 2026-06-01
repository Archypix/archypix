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
      - Postgres: authoritative metadata (users, pictures, tags, shares, jobs).
      - Federation endpoints: handle inbound/outbound federation messages (share announce/revoke, presign requests).
      - Job queue owner: writes `pending` jobs to the `jobs` table; exposes `/api/worker/*` endpoints for workers to claim and complete them.
      - In-process task queue: lightweight Tokio-based queue for DB-only tasks (tag-rename cascade, tagging pipeline evaluation) that do not require
        external compute.
      - Local caches: Redis for sessions, presigned URLs, federation tokens, and backend domain mappings.

- Workers (`archypix-worker`, one or more Rust processes)
    - Purpose: perform CPU/GPU-intensive work; never access the database or S3 directly.
    - Job transport: **Postgres-backed queue** using `SELECT FOR UPDATE SKIP LOCKED`. Workers poll `GET /api/worker/jobs/next` on the backend; the
      backend atomically claims and returns a job with all presigned S3 URLs needed.
    - S3 access: workers download originals and upload results exclusively via presigned URLs issued by the backend — no S3 credentials required.
    - Auth: workers authenticate to the backend with a short-lived JWT signed with `WORKER_JWT_SECRET` (`token_type: worker`), identical pattern to
      the resolver JWT.
    - Implemented job types:
        - `gen_thumbnail` — download original from MinIO, extract EXIF (rexiv2), generate small/medium/large WebP thumbnails (ImageMagick), compute
          BlurHash, upload via presigned PUT URLs, report extracted metadata to backend.
        - `edit_picture` — download original, apply EXIF overrides, optionally regenerate thumbnails, report updated metadata.
    - Stub job types (infrastructure ready, processing not yet implemented): `ml_style`, `ml_people`, `ml_group_location`.
    - Results are POSTed to `/api/worker/jobs/{id}/complete`; the backend updates the `pictures` row and marks the job completed. Failed jobs
      auto-retry up to `max_retries` (default 3).

- MinIO (S3-compatible object storage)
    - Purpose: durable blob store for original images, derivatives, ML snapshots, and exports.
    - Buckets: staging (short-lived uploads), pictures (confirmed originals), versions (version snapshots), small/medium/large (thumbnails).
    - S3 keys are derived deterministically (`{user_id}/{picture_id}`) and never stored in the database.
    - Two S3 endpoints: `S3_ENDPOINT` for server-side operations; `S3_PUBLIC_ENDPOINT` for presigned URLs returned to clients (differs in Docker
      setups).

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
- S3 keys are derived deterministically (`{user_id}/{picture_id}`) and never stored in the database; presigned URLs are issued on demand.
