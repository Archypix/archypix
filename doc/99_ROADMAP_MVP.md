# Backend + Resolver MVP Roadmap

## Completed

- [x] Core infrastructure: layered Rust architecture (domain/repository/services/clients/api/infra), Axum router, SQLx migrations, AppState wiring (
  Postgres, Redis, MinIO, JWT, federation, resolver clients).
- [x] Auth, users, pictures, tags, shares, settings endpoints; federation auth handshake (request/grant) and share announce/revoke; resolver
  user-management endpoints.
- [x] Picture upload pipeline: presigned staging → server-side copy → optional versioning (S3 copy + DB record in one transaction, `version_id`
  matches S3 key).
- [x] Resolver self-registration and tests with a frontend.
- [x] Worker pipeline (foundation): Postgres-backed job queue, HTTP-only worker crate.
- [x] Tag sharing full support: accept flow, pictures announcement, same-backend short-circuit, received picture rows, `/SharedToMe/…` tag assignment,
  presign for same-backend and cross-instance received pictures.
- [x] Tests: domain unit tests, repository integration tests, service integration tests, worker HTTP contract tests, federation end-to-end and
  security tests.

## To-do for the PoC

- [x] **Tagging pipeline CRUD** — API to define tagging services (rules and segmentation).
- [x] **Tagging pipeline execution** — wire `services/tagging.rs` to run the domain pipeline evaluator on ingest/edit/share events; connect the
  in-process `TaskQueue::RunTaggingPipeline` variant.
- [ ] **Tagging pipeline tags removal** — if the service enabled this feature (new service boolean column), the service would be able to remove a tag
  from a picture if this tag has the same `source` as the service's rule and that no other service has assigned to it that tag
- [ ] **Better sharing support** — create a tagging rule to associate tags with an incoming share, share back support, announce/unannounce on shared
  tag add/remove or on picture edit/remove. Test transitive
  sharing.
- [ ] **Exif edition** — exif update api endpoint, triggering a worker job editing the s3 picture metadata.
- [ ] **Admin endpoints** — user list/suspend/delete, job status, instance metrics.
- [ ] **Full frontend** — v1 of a user-friendly frontend, with super simple code for a PoC, but with a realistic user experience that could give an
  idea of what the MVP could look like.
- [ ] **Hierarchies** — CRUD operations for managing hierarchies.
- [ ] **WebDAV** — virtual directory tree over tags; full-res/thumbnail reads via presigned redirect or back proxy; staging-pattern writes; versioning
  on overwrite. Use `pictures.file_hash` as the WebDAV ETag.
  Two things from the specs have no roadmap item:
- [ ] **Trash & restore** — pictures deletion, announcement to sharing recipients setting their `deleted_at` too. Adding an endpoint allowing to copy
  the picture physically to keep it even if the owner trashed it.

## To-do for the MVP

- [ ] **Tag rename cascade** — expose API endpoint that triggers the in-process `TaskQueue::TagRename` task; add cascade to outgoing shares,
  segmentation configs, and hierarchies (currently only tags table is updated).
- [ ] **Federation robustness** — token refresh/rotation schedule, retry logic for failed announce/revoke, presigned URL caching for remote picture
  access.
- [ ] **Rate limiting and validators** — Redis-backed limits on auth, federation, and public endpoints; session invalidation on logout. Password,
  emails, usernames validators.
- [ ] **Full Frontend** — v2 of a user-friendly frontend, ready for production.

## To-do for the v1.0

- [ ] **ML workers** — implement `ml_style`, `ml_people`, `ml_group_location` job handlers; add per-user ML snapshot storage in MinIO.
- [ ] **Edit picture — visual edits** — add crop, brightness/contrast, and resize support to the `edit_picture` worker job.
- [ ] **Adavanced Frontend** — upgraded v2, or a v3 frontend with a more advanced user experience.
