# Backend + Resolver MVP Roadmap

## Completed

- [x] Define REST API structure and federation flows.
- [x] Backend router layout in `api.rs` with resolver/admin/user/federation modules.
- [x] JWT service + middleware (user/admin/resolver/federation) and token claims.
- [x] Redis + MinIO clients wired into `AppState`.
- [x] User auth endpoints (login/refresh/logout/me) + password hashing.
- [x] Admin/resolver user management endpoints.
- [x] Share endpoints (outgoing/incoming) + federation token acquisition.
- [x] Federation auth endpoints (request/grant).
- [x] Resolver JWT validation on update endpoint.
- [x] Docs/env examples updated for new config.

## To-do

- Pictures workers ant NATS-Jetstream OR Postgres to orchestrate workers
