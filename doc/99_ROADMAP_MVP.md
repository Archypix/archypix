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
- [x] Refactor architecture: generalized DB executors for transactions, created shared UserAccountService, updated rules for services vs
  infrastructure.

## Next steps (detailed)

- [ ] Pictures: repository layer
    - create `PictureRepository` (create/list/get, lookup by `picture_id` for owner)
    - ensure queries return full picture row with JSONB fields
- [ ] Storage service:
    - add `presign_put` helper in `StorageService`
    - reuse existing `presign_get` for download
- [ ] Upload flow:
    - `POST /api/authenticated/pictures/uploads` creates redis upload session + presigned PUT URL
    - `POST /api/authenticated/pictures/uploads/{id}/complete` persists picture metadata
- [ ] Pictures endpoints:
    - `GET /api/authenticated/pictures` list owned pictures
    - `GET /api/authenticated/pictures/{id}` get metadata
    - `GET /api/authenticated/pictures/{id}/download` return presigned URL
- [ ] Federation presign:
    - `POST /api/federation/pictures/presign` validates share + returns presigned URL
    - request includes `{ owner_username, owner_instance, picture_id }`
- [ ] Tags endpoints:
    - list tags for owner
    - assign/remove tags (batch + per picture)
    - use `ltree` paths and `tag_source = manual`
- [ ] Federation announcements:
    - handle `pictures/announce` to ingest shared picture refs + tags
- [ ] Run tests after migration and fix issues.
