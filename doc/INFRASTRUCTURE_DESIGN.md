- Resolver (Rust service)
    - Purpose: map username → owning backend domain (implements WebFinger).
    - Processes / roles:
        - WebFinger endpoint: answer /.well-known/webfinger requests.
        - Registry access: read/write authoritative mapping in Postgres.
        - Local cache: in‑process TTL cache (moka) for fast lookups.
        - Admin/update API: allow backends to register users.
- Backend (Rust backend instance, per domain)
    - Purpose: authoritative per‑instance application server and metadata store.
    - Processes / roles:
        - HTTP API & WebDAV: serve user requests, uploads, sync client endpoints.
        - WebFinger client: consult resolver when needed for cross‑instance discovery.
        - Postgres: authoritative metadata (users, jobs, faces, persons).
        - Job producer: create job record in Postgres, publish compact job
          message to job stream (NATS JetStream) including job_id, user_id,
          s3_key, snapshot_version, result_subject and ephemeral token.
        - Result consumer: durable consumer on jobs.results.<instance> — verify worker signature/token, persist results (faces, embeddings, thumbnails) into Postgres/vector index.
        - Local caches: optional Redis / in‑process caches for hot data and sessions.
        - Federation endpoints: handle inbound/outbound federation messages.
- Workers (central pool: Rust worker processes)
    - Purpose: perform CPU/GPU work (thumbnails, ML) and publish compact results back to owning backend.
    - Processes / roles:
        - Job consumer: durable JetStream consumer of jobs.requests (or subject); ack / nack semantics, DLQ handling.
        - Snapshot cache: on job, load per‑user snapshot (from MinIO) keyed by user_id+version; keep in memory/disk LRU.
        - Task types:
            - gen_thumbnail: download image from S3, generate thumbnails, upload derivatives.
            - ml_style: compute style/object metadata and return structured tags.
            - ml_people: detect faces, compute embeddings, run ANN match against cached user snapshot, return matched person ids + scores.
            - ml_group_location: cluster images by geo/time and return clustering data.
        - Result publisher: upload derivatives/crops to MinIO, publish compact result message to jobs.results.<instance> (job_id, s3_keys, matches, signature/token).
        - Security: use scoped credentials or presigned URLs to access MinIO; sign results or use broker auth.
- S3 / MinIO (object storage)
    - Purpose: durable blob store for original images, derivatives, snapshots, and exports.
    - Roles:
        - Store originals, thumbnails, face crops, per‑user snapshot files (e.g., snapshots/{user}/{version}.bin).
        - Provide presigned URLs for upload/download to clients and workers.
        - Serve as the medium for large bulk transfers (workers fetch snapshots from MinIO rather than inlining them in messages).
- Frontend (static CDN + clients)
    - Purpose: single static frontend site + sync clients that reach the proper backend instance.
    - Roles:
        - Static assets served via CDN / frontend pool (single codebase for all instances).
        - Discovery: browser and sync clients call WebFinger to resolve username → backend domain.
        - API/WebDAV calls: interact with the resolved backend for uploads, sync, gallery, etc.
        - UI flows: trigger upload ➜ backend enqueues job ➜ worker processes ➜ backend displays result when result message persisted.
          Developer notes (important invariants)
- Authority: each backend is authoritative for its users (Postgres is
  single source of truth per instance). Workers do not write backend DB
  directly — workers publish results and backends persist.
- Messaging: NATS JetStream for jobs + results; messages are compact (pointers/versions), snapshots live in MinIO.
- Security: ephemeral tokens or signed results + broker auth; presigned URLs for S3 access; authenticate resolver/backend calls.
- Caching & invalidation: use versioned snapshots and publish
  invalidation events so workers/ resolvers can evict caches safely.
- Reliability: JetStream durable consumers + DLQ; backends must verify
  and idempotently upsert job results (use job_id as idempotency key).
