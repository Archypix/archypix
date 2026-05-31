# Archypix — Backend

The authoritative per-instance API server for Archypix. It owns user accounts, pictures, tags, and shares for one deployment, and communicates with
other backends through the federation protocol.

For a full overview of the project, see the [root README](https://github.com/ClementGre/Archypix).

## Tech stack

| Concern            | Crate                                                                                                          |
|--------------------|----------------------------------------------------------------------------------------------------------------|
| HTTP server        | [Axum](https://github.com/tokio-rs/axum) + [Tokio](https://tokio.rs/)                                          |
| Database           | [SQLx](https://github.com/launchbadis/sqlx) + PostgreSQL (compile-time checked queries)                        |
| Session cache      | [bb8-redis](https://github.com/djc/bb8) + Redis                                                                |
| Object storage     | [aws-sdk-s3](https://github.com/awslabs/aws-sdk-rust) (MinIO / AWS S3)                                         |
| Auth               | [jsonwebtoken](https://github.com/Keats/jsonwebtoken), [argon2](https://github.com/RustCrypto/password-hashes) |
| Federation HTTP    | [reqwest](https://github.com/seanmonstar/reqwest)                                                              |
| Structured logging | [tracing](https://github.com/tokio-rs/tracing) + tracing-subscriber                                            |

## Configuration

Copy `.env.example` to `.env`. The file is fully commented and lists all available variables with their defaults.

```bash
cp .env.example .env
```

A few key concepts to be aware of:

- **`BACK_DOMAIN`** is the public domain of this specific backend instance (e.g. `backend1.example.com`), used as the JWT audience.
- **`GLOBAL_DOMAIN`** is the shared identity domain that appears in user handles (`@user:example.com`). It can differ from `BACK_DOMAIN`, which is
  common when a reverse proxy forwards WebFinger requests from the public domain to this backend.
- **`USE_RESOLVER`**: set to `true` when multiple backends share the same `GLOBAL_DOMAIN` via a Resolver. Set to `false` for a standalone instance.

Log level:

```bash
RUST_LOG=info,archypix_back=debug    # default
RUST_LOG=info,archypix_back=trace    # verbose: service calls, cache hits
```

SQLx query logs appear at the `sqlx=debug` level.

## Building

Prerequisites: Rust (stable, edition 2024) via [rustup](https://rustup.rs/), PostgreSQL, Redis, and an S3-compatible store.

```bash
# Development
cargo run

# Release
cargo build --release
./target/release/archypix-back

# Docker
docker compose up
```

## Database migrations

Migrations in `migrations/` are applied automatically at startup. To run them manually or create new ones:

```bash
sqlx migrate run --database-url "postgres://user:password@host/archypix_back"
sqlx migrate add <migration_name>
```

The `.sqlx/` directory contains cached query metadata for offline builds (CI without a live database). Regenerate it after any SQL change:

```bash
cargo sqlx prepare
```

---

## Code structure

- `domain/` — pure business types and rules, no I/O
- `repository/` — SQL queries only, no business logic
- `services/` — multi-step workflows with explicit transaction boundaries
- `clients/` — outbound HTTP adapters (federation, resolver)
- `infra/` — raw connectivity primitives (config, DB, Redis, S3, crypto, error)
- `state.rs` — `AppState`: holds all infra handles, no business logic
- `api/` — HTTP handlers, auth extractors, request/response models
    - `middleware/` — JWT extractors for each token type (user, resolver, federation)
    - `user/` — `/api/auth/*`, `/api/public/*`, `/api/authenticated/*`
    - `admin/` — `/api/admin/*`
    - `federation/` — `/api/federation/*`
    - `resolver/` — `/api/resolver/*`
    - `webfinger.rs` — `/.well-known/webfinger` (standalone mode only)

Dependency rule: `api → services → repository → domain`. No layer may reach upward. `clients` and `infra` are horizontal utilities.

## API surface

| Route group        | Base path                      | Auth            |
|--------------------|--------------------------------|-----------------|
| Auth and public    | `/api/auth/*`, `/api/public/*` | None / user JWT |
| Authenticated      | `/api/authenticated/*`         | User JWT        |
| Admin              | `/api/admin/*`                 | Admin User JWT  |
| Resolver callbacks | `/api/resolver/*`              | Resolver JWT    |
| Federation         | `/api/federation/*`            | Federation JWT  |
| WebDAV *(planned)* | `/dav/*`                       | User JWT        |

## License

[AGPL-3.0](https://github.com/ClementGre/Archypix/blob/main/LICENSE)
