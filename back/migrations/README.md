# Archypix Backend Database Schema

## Quick reference

### After changing a query or adding a new query

Regenerate the SQLx offline query cache (requires a running database):

```bash
cargo sqlx prepare
```

### After modifying the latest migration file (not adding a new one)

Revert the last migration, then re-apply it:

```bash
sqlx migrate revert
sqlx migrate run
cargo sqlx prepare
```

### After adding a new migration file

```bash
sqlx migrate run
cargo sqlx prepare
```

## Overview

PostgreSQL schema for the Archypix decentralized picture management backend. Supports hierarchical tags, federation, sharing, and async job
processing.

## Key Design Decisions

### 1. PostgreSQL Extensions

- `uuid-ossp`: UUID generation for primary keys
- `ltree`: Hierarchical tag path storage and queries (`Photos.Travel.Alps`)
- `JSONB`: Flexible metadata (EXIF, ML results, hierarchy config)

### 2. Tag Hierarchy with ltree

Tags are stored as `LTREE` paths without leading slashes. Only explicitly assigned tags are stored; ancestor tags are derived on read using `ltree`
operators. The `picture_has_tag()` SQL function checks for a tag or any of its descendants stored on a picture:

```sql
tag_path = target_tag        -- exact match
OR tag_path <@ target_tag    -- stored tag is a descendant; target is a virtual ancestor
```

### 3. Composite Picture Key

Pictures use the unique constraint `(local_user_id, picture_id)`:

- `local_user_id`: Local user holding this row — the owner for owned pictures, the recipient for received pictures.
- `picture_id`: UUID assigned by the original owner's instance. Globally unique in practice, so this constraint is also the deduplication key for
  federation.

Cross-instance identity is stored separately in `owner_username` / `owner_instance_domain` (NULL for owned pictures).

### 4. Soft Deletion

Pictures support soft deletion via `deleted_at`:

- **Owned**: Marked deleted, physically removed after retention period.
- **Received**: Marked deleted locally; file lives on sender's storage.

### 5. Tagging Services Pipeline

Three-table inheritance pattern with a shared base:

```
tagging_services (base: gate conditions, service_type, enabled)
├── shared_tag_mapping_services
├── rule_tagging_services
└── segmentation_tagging_services
```

Pipeline order and trigger labels (hardcoded in application logic, not stored):

- `shared_tag_mapping`: order=1, triggers=`[incoming_share]`
- `rule`: order=2, triggers=`[incoming_share, ingest, metadata, manual_tag, rule_edit]`
- `segmentation`: order=3, triggers=`[incoming_share, ingest, metadata, manual_tag, rule_edit, segmentation_edit]`

### 6. Federation Model

- **Outgoing shares**: live on sender's backend; reference a tag path and recipient identity.
- **Incoming shares**: live on recipient's backend; reference sender's `outgoing_share_id` (UUID, no FK — cross-instance).
- **Federation messages**: audit log with JSONB payload and an `idempotency_key` for deduplication.

### 7. Hierarchy (WebDAV)

Hierarchy configuration stored as a single JSONB column (no joins needed):

```json
{
  "roots": [{"path": "Photos", "keepDir": false}],
  "collapsedTags": ["Photos.Travel.Alps.Hiking"],
  "disabledTags": ["Photos.Outdoor"],
  "safeDeleteMode": "singleBranch"
}
```

### 8. Job Queue

Status-based queue with JSONB config. Idempotency key prevents duplicate jobs:

- `pending` → `processing` → `completed` / `failed`

### 9. Enum Types

All PostgreSQL enums use snake_case values and map directly to Rust enums via `sqlx::Type`:

| PG type                   | Values                                                         |
|---------------------------|----------------------------------------------------------------|
| `share_status`            | `active`, `revoked`, `tombstoned`                              |
| `tag_source`              | `manual`, `rule`, `segment`, `share_mapping`, `incoming_share` |
| `job_status`              | `pending`, `processing`, `completed`, `failed`                 |
| `job_type`                | `gen_thumbnail`, `ml_style`, `ml_people`, `ml_group_location`  |
| `federation_message_type` | `share_announcement`, `share_revocation`, `picture_update`     |
| `federation_direction`    | `inbound`, `outbound`                                          |
| `federation_status`       | `pending`, `sent`, `delivered`, `failed`                       |
| `service_type`            | `shared_tag_mapping`, `rule`, `segmentation`                   |

`safe_delete_mode` is defined in PG but used only inside the hierarchy JSONB config.

## Table Relationships

```
users
├── pictures (1:N via local_user_id)
│   └── tags (1:N)
├── outgoing_shares (1:N)
├── incoming_shares (1:N)
├── tagging_services (1:N)
│   ├── shared_tag_mapping_services (1:N)
│   ├── rule_tagging_services (1:N)
│   └── segmentation_tagging_services (1:N)
├── hierarchies (1:N)
└── jobs (1:N)
```

## Helper Functions

- **`picture_has_tag(picture_uuid, target_tag)`** — returns true if the picture has the tag or any stored descendant of it.
- **`get_pictures_under_tag(tag_prefix)`** — returns all non-deleted picture IDs under a tag prefix.
- Ancestor expansion is implemented in Rust using ltree string splitting.
