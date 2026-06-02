> **Maintenance notice** — Do not add more details on the work you did compared to the existing documentation. The same level of precision and depth
> must be maintained in this document.

# Worker Architecture

## Worker endpoints (`/api/worker/*`)

Auth: `Authorization: Bearer <worker_jwt>` — short-lived JWT (HS256, 300 s TTL) signed with `WORKER_JWT_SECRET` (`token_type: worker`). Workers cache
the token and refresh 30 s before expiry.

| Method | Path                             | Description                                                                                                                                                                     |
|--------|----------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `GET`  | `/api/worker/jobs/next`          | Atomically claim next pending job (`SELECT FOR UPDATE SKIP LOCKED`). Returns the job + presigned S3 URLs + a one-time `claim_token`, or `null`. Query: `types=gen_thumbnail,…`. |
| `POST` | `/api/worker/jobs/{id}/complete` | Report job success. Body: `CompleteJobRequest` (see below). Backend applies picture updates and marks job `completed` in one transaction.                                       |
| `POST` | `/api/worker/jobs/{id}/fail`     | Report job failure. Body: `FailJobRequest` (see below). Backend auto-retries up to `max_retries` (default 3) unless `permanent: true`.                                          |

**`GET /api/worker/jobs/next` response shape**

```json
{
  "job_id": "uuid",
  "job_type": "gen_thumbnail",
  "picture_id": "uuid",
  "mime_type": "image/jpeg",
  "claim_token": "uuid",
  "config": {
    "type": "gen_thumbnail",
    "picture_id": "uuid",
    "is_initial": true
  },
  "presigned_read": "https://minio/…",
  "presigned_writes": {
    "small": "https://minio/…",
    "medium": "https://minio/…",
    "large": "https://minio/…"
  }
}
```

`presigned_writes` keys by job type:

| Job type                   | `presigned_writes` fields            |
|----------------------------|--------------------------------------|
| `gen_thumbnail`            | `small`, `medium`, `large`           |
| `edit_picture` (EXIF only) | `output`                             |
| `edit_picture` (visual)    | `output`, `small`, `medium`, `large` |
| ML types                   | _(none)_                             |

**`POST /api/worker/jobs/{id}/complete` request body (`CompleteJobRequest`)**

```json
{
  "claim_token": "uuid",
  "exif": {
    "width": 4000,
    "height": 3000,
    "captured_at": "2024:08:03 14:22:00"
    …
  },
  "blurhash": "LKO2?U%2Tw=w]~RBVZRi};RPxuwH",
  "thumbnails_generated": true,
  "file_size": 8473621,
  "file_hash": "e3b0c44298fc1c149afb…"
}
```

| Field                  | Required when                           | Description                                                                        |
|------------------------|-----------------------------------------|------------------------------------------------------------------------------------|
| `claim_token`          | Always                                  | Must match the token issued at claim. Backend rejects mismatches (409).            |
| `exif`                 | `gen_thumbnail` with `is_initial: true` | EXIF extracted from the original file.                                             |
| `blurhash`             | Optional                                | BlurHash computed from the original or modified file.                              |
| `thumbnails_generated` | Always                                  | `true` when small/medium/large were generated and uploaded.                        |
| `file_size`            | Always when available                   | Byte count of the file as stored in S3 after any EXIF writes or visual transforms. |
| `file_hash`            | Always when available                   | SHA-256 hex digest of the stored file. Used as WebDAV ETag.                        |

**`POST /api/worker/jobs/{id}/fail` request body (`FailJobRequest`)**

```json
{
  "claim_token": "uuid",
  "error": "unsupported MIME type: image/gif",
  "permanent": true
}
```

`claim_token` must match the issued token. `permanent: true` skips the retry counter and marks the job `failed` immediately.

## Worker architecture

Workers are standalone Rust processes (`archypix-worker`) that poll the backend for jobs over HTTP and never touch the database or S3 directly.

### Module layout (`worker/src/`)

```
main.rs              — tokio entry-point; starts health server + job loop
config.rs            — Config::from_env(); all settings with documented defaults
auth.rs              — generate_token(): HS256 JWT generation; cached via BackendClient
error.rs             — WorkerError; is_retriable() classifies transient vs permanent failures
backend.rs           — BackendClient: two separate HTTP clients (api_http 10 s timeout,
                       presign_http connect-only timeout for large-file transfers);
                       JWT token cache (refreshed 30 s before expiry);
                       claim_next_job / complete_job / fail_job /
                       download_presigned (streaming) / upload_presigned

jobs.rs              — run_job_loop(): acquire semaphore → poll → spawn; dispatch()
jobs/thumbnail.rs    — gen_thumbnail: MIME preflight → download → EXIF → hash → thumbnails → complete
jobs/edit_picture.rs — edit_picture: download → EXIF write → hash → upload → thumbnail regen → complete
jobs/ml.rs           — stub for ml_* jobs (log + complete with empty result)

imaging/exif.rs      — extract_exif() / write_exif_overrides() (rexiv2, blocking)
imaging/hash.rs      — hash_file(): SHA-256 hex digest in 64 KiB chunks (blocking)
imaging/resize.rs    — generate_thumbnail() (ImageMagick/WebP), generate_blurhash();
                       THUMBNAIL_VARIANTS const: single source of truth for sizes
imaging/thumbnailer.rs — run(): spawn_blocking for CPU work, async upload per variant
```

### Claim-token protocol

When a job is claimed, the backend generates a fresh `claim_token` UUID and stores it on the job row. The token is returned in `ClaimJobResponse`.
Every subsequent `complete` and `fail` call must include the same `claim_token`.

The backend's SQL guards `AND claim_token = $x AND status = 'processing'` on both UPDATE operations. If the watchdog resets a stale job (clearing
`claim_token`) and a second worker re-claims it, the first worker's late `complete` or `fail` call will find no matching row and receive a 409. This
prevents stale workers from corrupting re-claimed jobs.

### Job loop

```
loop {
  sem.acquire_owned().await           ← blocks until a slot is free; no sleep-poll
  claim_next_job():
    None  → drop permit, sleep poll_interval_ms
    Some  → tokio::spawn dispatch(job) (permit dropped when task exits)
    Err   → drop permit, sleep 5 × poll_interval_ms
}
```

The semaphore is acquired before polling, so when a running job finishes and drops its permit, the next claim happens immediately without waiting for
a poll interval.

### Error policy

Some errors are transient and can be retried, others are permanent and should be marked `failed` permanently. `is_retriable()` on `WorkerError`
classifies them.
On back, the watchdog (`infra/job_watchdog.rs`) runs every `JOB_WATCHDOG_INTERVAL_SECS` (default 60 s) and resets jobs stuck in `processing` for
longer than `JOB_PROCESSING_TIMEOUT_SECS` (default 600 s) by incrementing `retry_count` and returning them to `pending` (or `failed` if retries
exhausted). It also clears `claim_token` on reset.

### Shared types (`archypix-common`)

Library crate shared between `back/` and `worker/` so wire shapes never drift:

| Module           | Key types                                                                                                                                                                   |
|------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `job.rs`         | `JobType`, `JobConfig`, `GenThumbnailConfig`, `EditPictureConfig`, `ExtractedExif`, `ExifOverrides`                                                                         |
| `transfer.rs`    | `ClaimQuery`, `ClaimJobResponse` (+ `claim_token`), `PresignedWrites`, `CompleteJobRequest` (+ `claim_token`, `file_size`, `file_hash`), `FailJobRequest` (+ `claim_token`) |
| `mime.rs`        | `MIME_TYPES_EXIF`, `MIME_TYPES_THUMBNAIL`, `supports_exif()`, `supports_thumbnail()`                                                                                        |
| `serde_utils.rs` | `csv` serde module for comma-separated `Vec<T>` query params                                                                                                                |
