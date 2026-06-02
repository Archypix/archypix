# Archypix — Worker

The image-processing worker for Archypix. It polls the backend for pending jobs, processes pictures (thumbnails, EXIF extraction, blurhash), and
reports results back — without ever touching the database or S3 directly.

For a full overview of the project, see the [root README](https://github.com/ClementGre/Archypix).

## Tech stack

| Concern             | Crate / tool                                                                   |
|---------------------|--------------------------------------------------------------------------------|
| Async runtime       | [Tokio](https://tokio.rs/)                                                     |
| HTTP client         | [reqwest](https://github.com/seanmonstar/reqwest)                              |
| Image processing    | [magick_rust](https://github.com/nlfiedler/magick-rust) (ImageMagick bindings) |
| EXIF extraction     | [rexiv2](https://gitlab.gnome.org/GNOME/gexiv2) (GExiv2 / Exiv2 bindings)      |
| BlurHash generation | [blurhash](https://github.com/nicowillis/blurhash)                             |
| Auth                | [jsonwebtoken](https://github.com/Keats/jsonwebtoken)                          |
| Structured logging  | [tracing](https://github.com/tokio-rs/tracing) + tracing-subscriber            |
| Health check server | [Axum](https://github.com/tokio-rs/axum)                                       |

## System prerequisites

The worker links against two native C libraries that must be installed before building:

### ImageMagick (for thumbnail generation)

```bash
# macOS
brew install imagemagick

# Debian / Ubuntu
apt-get install libmagickwand-dev

# Fedora / RHEL
dnf install ImageMagick-devel
```

Verify the installed version:

```bash
pkg-config --modversion MagickWand
```

The `magick_rust` crate version in `Cargo.toml` must match your system ImageMagick version. Adjust `magick_rust = "0.19"` in `Cargo.toml` if needed.

### GExiv2 / Exiv2 (for EXIF extraction)

GExiv2 is a GLib/GObject wrapper around Exiv2.

```bash
# macOS
brew install gexiv2

# Debian / Ubuntu
apt-get install libgexiv2-dev

# Fedora / RHEL
dnf install gexiv2-devel
```

## Configuration

Copy `.env.example` to `.env`. The file is fully commented and lists all available variables with their defaults.

```bash
cp .env.example .env
```

Key variables:

- **`BACK_URL`** — Base URL of the Archypix backend (e.g. `http://backend:8000`).
- **`BACK_DOMAIN`** — The backend's public domain; used as the JWT audience. Must match the backend's `BACK_DOMAIN`.
- **`GLOBAL_DOMAIN`** — Shared identity domain; used in the JWT `instance` field. Must match the backend's `GLOBAL_DOMAIN`.
- **`WORKER_JWT_SECRET`** — Shared HMAC secret for signing worker JWTs. Must be identical to the backend's `WORKER_JWT_SECRET`.
- **`WORKER_ID`** — Unique name for this instance (defaults to a random short ID).
- **`POLL_INTERVAL_MS`** — How often to poll for new jobs when idle (default: `1000`).
- **`MAX_CONCURRENT_JOBS`** — Maximum jobs processed simultaneously (default: `2`).
- **`JOB_TYPES`** — Comma-separated list of accepted job types; empty means all (e.g. `gen_thumbnail,edit_picture`).

Log level:

```bash
RUST_LOG=info,archypix_worker=debug    # default
RUST_LOG=info,archypix_worker=trace    # verbose
```

## Building

Prerequisites: Rust (stable, edition 2024) via [rustup](https://rustup.rs/), plus the system libraries listed above.

```bash
# Development
cargo run

# Release
cargo build --release
./target/release/archypix-worker

# Docker
docker compose up
```

## Job types

### `gen_thumbnail`

Downloads the original picture, extracts EXIF data (when `is_initial` is `true`), generates WebP thumbnails at three sizes, computes a BlurHash, and
uploads all outputs via presigned PUT URLs.

| Variant | Height  | Format |
|---------|---------|--------|
| small   | 100 px  | WebP   |
| medium  | 500 px  | WebP   |
| large   | 1000 px | WebP   |

Width is derived from the original aspect ratio.

### `edit_picture`

Downloads the original picture and re-extracts EXIF as the authoritative source. When `regenerate_thumbnails` is `true` in the job config, thumbnails
are also regenerated. Full edit operations (crop, colour adjustments, etc.) are planned for a future milestone.

### ML jobs (stubs)

`ml_style`, `ml_people`, and `ml_group_location` are accepted but not yet implemented. The worker logs the job and immediately reports completion with
an empty result.

## Code structure

- `src/main.rs` — Entry point: loads config, starts the health server and job polling loop.
- `src/config.rs` — Environment-based configuration with validation.
- `src/error.rs` — Unified `WorkerError` enum and `Result<T>` alias.
- `src/auth.rs` — Short-lived worker JWT generation (HS256, 300 s TTL).
- `src/backend/` — HTTP client for the Archypix backend API.
    - `mod.rs` — `BackendClient`: claim jobs, report completion/failure, download/upload presigned URLs.
    - `models.rs` — Request/response types shared with the backend API contract.
- `src/imaging/` — Native image-processing routines (all blocking; called via `spawn_blocking`).
    - `exif.rs` — EXIF extraction via rexiv2.
    - `resize.rs` — WebP thumbnail generation and BlurHash computation via magick_rust.
- `src/jobs/` — Job dispatch and per-type handlers.
    - `mod.rs` — `run_job_loop`: concurrency-bounded poll loop with backoff on error.
    - `thumbnail.rs` — `gen_thumbnail` handler.
    - `edit_picture.rs` — `edit_picture` handler.
    - `ml.rs` — Stub handler for ML job types.

## Health check

A minimal HTTP server runs on `LISTEN_ADDR` (default `0.0.0.0:80`) and exposes:

```
GET /health  →  200  {"status": "healthy", "service": "archypix-worker"}
```

Use this endpoint for Docker/Kubernetes liveness probes.
