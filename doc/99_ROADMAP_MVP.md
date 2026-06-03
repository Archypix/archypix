> **Maintenance notice** — Do not add more details on the work you did compared to the existing documentation. The same level of precision and depth
> must be maintained in this document.

# Backend + Resolver MVP Roadmap

## Completed

- [x] Core infrastructure: layered Rust architecture (domain/repository/services/clients/api/infra), Axum router, SQLx migrations, AppState wiring (
  Postgres, Redis, MinIO, JWT, federation, resolver clients).
- [x] Auth, users, pictures, tags, shares, settings endpoints; federation auth handshake (request/grant) and share announce/revoke; resolver
  user-management endpoints.
- [x] Picture upload pipeline: presigned staging → server-side copy → optional versioning (S3 copy + DB record in one transaction, `version_id`
  matches S3 key).
- [x] Resolver self-registration and tests with a frontend.
- [x] Worker pipeline (foundation) Postgres-backed job queue, crate with HTTP-only design.

## To-do

- [x] **Tag sharing full support** — federation client split into `handshake.rs` / `shares.rs`; accept incoming share flow wired (federation message →
  pictures announcement); `POST /api/federation/shares/accept` added; `announce_pictures` handler implemented; same-backend short-circuit in
  `create_outgoing_share`; `PictureRepository::create_received`; `TagRepository::assign_incoming_share_tag` / `remove_incoming_share_tags`;
  `batch_remove` protected to `source = manual` only; Redis caching for `find_token_by_sender` and `find_by_username`; presign fixed to use
  `remote_picture_id` for both same-backend and cross-instance received pictures.
- [ ] **Tests** — Add tests
- [ ] **Tagging pipeline execution** — wire `services/tagging.rs` to run the domain pipeline evaluator on ingest/edit/share events; connect the
  in-process `TaskQueue::RunTaggingPipeline` variant.
- [ ] **Tag rename cascade** — expose API endpoint that triggers the in-process `TaskQueue::TagRename` task; add cascade to outgoing shares,
  segmentation configs, and hierarchies (currently only tags table is updated).
- [ ] **ML workers** — implement `ml_style`, `ml_people`, `ml_group_location` job handlers; add per-user ML snapshot storage in MinIO.
- [ ] **Edit picture — visual edits** — add crop, brightness/contrast, and resize support to the `edit_picture` worker job (EXIF-only MVP is done).
- [ ] **WebDAV** — virtual directory tree over tags; full-res reads via presigned redirect; thumbnail proxy; staging-pattern writes; versioning on
  overwrite. Use `pictures.file_hash` as the WebDAV ETag.
- [ ] **Rate limiting** — Redis-backed limits on auth, federation, and public endpoints; session invalidation on logout.
- [ ] **Federation robustness** — token refresh/rotation schedule, retry logic for failed announce/revoke, presigned URL caching for remote picture
  access.
- [ ] **Admin endpoints** — user list/suspend/delete, job status, instance metrics.
