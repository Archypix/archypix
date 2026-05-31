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
      - Job producer/consumer: publish job messages to NATS JetStream; consume and persist results from workers.
      - Local caches: Redis for sessions, presigned URLs, federation tokens, and backend domain mappings.

- Workers (central pool: Rust worker processes)
    - Purpose: perform CPU/GPU work and publish compact results back to owning backends; never write to backend databases directly.
    - Task types: thumbnail generation, ML style/object tagging, face detection and embedding, geo/time clustering.
    - Results are published to the backend via NATS JetStream; large artifacts (thumbnails, crops, snapshots) go to MinIO.

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
- Workers publish results; backends persist — workers never write backend databases directly.
- All persistent storage (DB fields, JWT claims, federation messages) uses the **global domain**. Backend domains are resolved on demand via WebFinger
  and cached in Redis.
- NATS JetStream for jobs + results; messages are compact (IDs and pointers), large data lives in MinIO.
