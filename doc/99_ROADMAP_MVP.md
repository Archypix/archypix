# Backend + Resolver MVP Roadmap

## Completed

- [x] Core infrastructure: layered Rust architecture (domain/repository/services/clients/api/infra), Axum router, SQLx migrations, AppState wiring (
  Postgres, Redis, MinIO, JWT, federation, resolver clients).
- [x] Auth, users, pictures, tags, shares, settings endpoints; federation auth handshake (request/grant) and share announce/revoke; resolver
  user-management endpoints.
- [x] Picture upload pipeline: presigned staging → server-side copy → optional versioning.
- [x] Resolver self-registration and tests with a frontend.
- [x] **Worker pipeline (foundation)** — Postgres-backed job queue (`SELECT FOR UPDATE SKIP LOCKED`); `archypix-worker` crate with HTTP-only design (
  no direct DB/S3 access); worker JWT auth (`WORKER_JWT_SECRET`); `gen_thumbnail` job: EXIF extraction (rexiv2), WebP thumbnails (ImageMagick),
  BlurHash; `edit_picture` job: EXIF override + optional thumbnail regeneration; ML job stubs; in-process Tokio task queue for tag-rename cascade and
  tagging pipeline; new picture columns: `blurhash`, `gps_lat/lng/alt`, `orientation`, `thumbnails_generated_at`.

## To-do

- [ ] **Tags full support** — review tagging endpoints. Add tagging features to the PoC front-test (picture tagging).
- [ ] **Tag sharing full support** — review current federation, verify flows for inter-instance sharing, inter-global-domain sharing, and
  multi-global-instance sharing. Define how transitive sharing should work.
- [ ] **Tagging pipeline execution** — wire `services/tagging.rs` to run the domain pipeline evaluator on ingest/edit/share events; connect the
  in-process `TaskQueue::RunTaggingPipeline` variant.
- [ ] **Tag rename cascade** — expose API endpoint that triggers the in-process `TaskQueue::TagRename` task; add cascade to outgoing shares,
  segmentation configs, and hierarchies (currently only tags table is updated).
- [ ] **ML workers** — implement `ml_style`, `ml_people`, `ml_group_location` job handlers; add per-user ML snapshot storage in MinIO.
- [ ] **Worker job timeout/recovery** — detect stale `processing` jobs (e.g. worker crashed mid-job) and reset them to `pending` via a periodic
  backend task.
- [ ] **Edit picture — visual edits** — add crop, brightness/contrast, and resize support to the `edit_picture` worker job (EXIF-only MVP is done).
- [ ] **WebDAV** — virtual directory tree over tags; full-res reads via presigned redirect; thumbnail proxy; staging-pattern writes; versioning on
  overwrite.
- [ ] **Rate limiting** — Redis-backed limits on auth, federation, and public endpoints; session invalidation on logout.
- [ ] **Federation robustness** — token refresh/rotation schedule, retry logic for failed announce/revoke, presigned URL caching for remote picture
  access.
- [ ] **Admin endpoints** — user list/suspend/delete, job status, instance metrics.
