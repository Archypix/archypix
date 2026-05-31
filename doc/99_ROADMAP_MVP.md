# Backend + Resolver MVP Roadmap

## Completed

- [x] Core infrastructure: layered Rust architecture (domain/repository/services/clients/api/infra), Axum router, SQLx migrations, AppState wiring (
  Postgres, Redis, MinIO, JWT, federation, resolver clients).
- [x] Auth, users, pictures, tags, shares, settings endpoints; federation auth handshake (request/grant) and share announce/revoke; resolver
  user-management endpoints.
- [x] Picture upload pipeline: presigned staging → server-side copy → optional versioning; `PictureVariant` presign endpoint (
  `GET /pictures/{id}/url?variant=original|small|medium|large`); Redis-cached presigned URLs using `S3_PUBLIC_ENDPOINT`.
- [x] Resolver self-registration: backend calls `POST /api/backends` at startup with `{ back_domain, use_https, internal_url }`; resolver routes
  registrations via `internal_url`; `back_domain` is the PK, WebFinger URLs derived at query time from `back_domain + use_https`.
- [x] Config overhaul: split DB/Redis URLs into individual vars; `BACK_DOMAIN`, `GLOBAL_DOMAIN`, `CORS_ORIGINS`, `BACK_USE_HTTPS`,
  `FEDERATION_USE_HTTPS`, `RESOLVER_JWT_SECRET`, `S3_PUBLIC_ENDPOINT`; safe production defaults.
- [x] RFC 7033 WebFinger compliance: `application/jrd+json` content type; `domain:port` resource parsing (`splitn` fix) on both resolver and
  standalone backend handler; domain validation with 404 on mismatch.
- [x] Dev compose: 3-backend federation topology (2 resolver-backed + 1 standalone), shared `.env.back.common`, hot-reload source mounts.

## To-do

- [ ] **Worker pipeline** — job queue (NATS JetStream or Postgres-backed) for thumbnail generation and ML inference; worker publishes results back to
  backend; metadata backfill after worker completes.
- [ ] **WebDAV** — virtual directory tree over tags; full-res reads via presigned redirect; thumbnail proxy; staging-pattern writes; versioning on
  overwrite.
- [ ] **Tagging pipeline execution** — wire `services/tagging.rs` to run the domain pipeline evaluator on ingest/edit/share events.
- [ ] **Rate limiting** — Redis-backed limits on auth, federation, and public endpoints; session invalidation on logout.
- [ ] **Federation robustness** — token refresh/rotation schedule, retry logic for failed announce/revoke, presigned URL caching for remote picture
  access.
- [ ] **Admin endpoints** — user list/suspend/delete, job status, instance metrics.
