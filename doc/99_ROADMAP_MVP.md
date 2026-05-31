# Backend + Resolver MVP Roadmap

## Completed

- [x] Core infrastructure: layered Rust architecture (domain/repository/services/clients/api/infra), Axum router, SQLx migrations, AppState wiring (
  Postgres, Redis, MinIO, JWT, federation, resolver clients).
- [x] Auth, users, pictures, tags, shares, settings endpoints; federation auth handshake (request/grant) and share announce/revoke; resolver
  user-management endpoints.
- [x] Picture upload pipeline: presigned staging → server-side copy → optional versioning.
- [x] Resolver self-registration and tests with a frontend.

## To-do

- [ ] **Tags full support** — review tagging endpoints. Add tagging features to the PoC front-test (picture tagging).
- [ ] **Tag sharing full support** — review current federation, verify flows for inter-instance sharing, inter-global-domain sharing, and
  multi-global-instance sharing. Define how transitive sharing should work.
- [ ] **Worker pipeline** — job queue (NATS JetStream or Postgres-backed) for thumbnail generation and ML inference; worker publishes results back to
  backend; metadata backfill after worker completes.
- [ ] **Tagging pipeline execution** — wire `services/tagging.rs` to run the domain pipeline evaluator on ingest/edit/share events.
- [ ] **WebDAV** — virtual directory tree over tags; full-res reads via presigned redirect; thumbnail proxy; staging-pattern writes; versioning on
  overwrite.
- [ ] **Rate limiting** — Redis-backed limits on auth, federation, and public endpoints; session invalidation on logout.
- [ ] **Federation robustness** — token refresh/rotation schedule, retry logic for failed announce/revoke, presigned URL caching for remote picture
  access.
- [ ] **Admin endpoints** — user list/suspend/delete, job status, instance metrics.
