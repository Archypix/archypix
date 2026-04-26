# Archypix Backend Database Schema

## Overview

This document describes the PostgreSQL schema design for the Archypix decentralized picture management backend. The schema is designed to support hierarchical tags, federation, sharing, and async job processing.

## Key Design Decisions

### 1. PostgreSQL with Extensions

**Why PostgreSQL:**
- Strong ACID transactions for tag rename cascades and share revocations
- Complex relational queries for tag hierarchies and picture lookups
- `ltree` extension for efficient hierarchical tag path operations
- `JSONB` for flexible metadata (EXIF, ML results, predicates)
- Mature Rust ecosystem (`sqlx`, `diesel`)

**Extensions Used:**
- `uuid-ossp`: UUID generation for primary keys
- `ltree`: Hierarchical tag path storage and queries

### 2. Tag Hierarchy with ltree

Tags are stored as `LTREE` paths (e.g., `Photos.Travel.Alps`) without leading slashes. The `ltree` extension provides:

- **Ancestor queries**: `tag_path <@ 'Photos.Travel'` finds all descendants
- **Descendant queries**: `'Photos.Travel' <@ tag_path` finds all ancestors
- **GIN indexing**: Efficient path-based queries

**Virtual Ancestors:**
- Only explicitly assigned tags are stored (`is_virtual = FALSE`)
- Ancestors are derived on read using `ltree` operators
- `get_tag_ancestors()` is implemented in Rust using ltree operators

### 3. Composite Picture Key

Pictures use a composite unique constraint `(owner_id, picture_id)`:
- `owner_id`: References the `users` table (local user who owns/received the picture)
- `picture_id`: Unique identifier within the owner's instance

**Cross-Instance Support:**
- `owner_username` and `owner_instance_domain` store the original owner's identity
- NULL for owned pictures, populated for received pictures
- Enables direct blob fetching from original owner's backend

### 4. Soft Deletion

Pictures support soft deletion via `deleted_at` timestamp:
- **Owned pictures**: Marked deleted, physically removed after retention period
- **Received pictures**: Marked deleted locally, never physically deleted
- All queries filter out `deleted_at IS NOT NULL` records

### 5. Tagging Services Pipeline

The tagging system uses a three-table inheritance pattern:

```
tagging_services (base)
├── shared_tag_mapping_services
├── rule_tagging_services
└── segmentation_tagging_services
```

**Pipeline Order and Triggers (Hardcoded):**
- `shared-tag-mapping`: order=1, triggers=[incoming-share]
- `rule`: order=2, triggers=[incoming-share, ingest, metadata, manual-tag, rule-edit]
- `segmentation`: order=3, triggers=[incoming-share, ingest, metadata, manual-tag, rule-edit, segmentation-edit]

**Benefits:**
- Common gate conditions (requires/excludes) in base table
- Service-specific configuration in child tables
- Easy to add new service types

### 6. Federation Model

**Outgoing Shares:**
- Live on sender's backend
- Reference tag path being shared
- Store recipient identity (`@username:instance.com`)

**Incoming Shares:**
- Live on recipient's backend
- Reference sender's OutgoingShare ID (no FK, cross-instance)
- Link to local SharedTagMappingService

**Cross-Instance References:**
- No foreign keys between instances
- Use `outgoing_share_id` UUID for correlation
- Federation messages track delivery status

### 7. Hierarchy (WebDAV Filesystem)

Hierarchies map tag graphs to filesystem trees using a single `config` JSONB column:

```json
{
  "roots": [
    {"path": "Photos", "keepDir": false},
    {"path": "Images", "keepDir": true}
  ],
  "collapsedTags": ["Photos.Travel.Alps.Hiking"],
  "disabledTags": ["Photos.Outdoor"],
  "safeDeleteMode": "singleBranch"
}
```

**Benefits:**
- Single query to fetch entire hierarchy configuration
- GIN index on JSONB for efficient queries
- Simpler than multiple related tables

**Safe Delete Modes:**
- `singleBranch`: Only removes tag for accessed path (recommended for sync clients)
- `fullDelete`: Marks picture deleted (moved to trash)

### 8. Job Queue

Jobs use a simple status-based queue with JSONB config:

```json
{
  "picture_ids": ["uuid1", "uuid2"],
  "sizes": ["thumb", "medium"]
}
```

**Benefits:**
- Generic design supports any job type
- Config can reference multiple pictures or other entities
- Idempotency key prevents duplicate jobs

**Status Flow:**
- `pending`: Waiting to be processed
- `processing`: Currently being processed by a worker
- `completed`: Successfully completed
- `failed`: Failed after max retries

### 9. Federation Messages

Federation messages are log entries with JSONB payload:

```json
{
  "picture_ids": ["uuid1", "uuid2"],
  "tag_path": "Photos.Travel.Alps"
}
```

**Benefits:**
- Generic design supports any message type
- Payload can reference multiple pictures or other data
- Serves as audit log for federation activity

### 10. Enum Types

PostgreSQL ENUM types are used for status fields instead of VARCHAR:
- `share_status`: active, revoked, tombstoned
- `tag_source`: manual, rule, segment, share-mapping, incoming-share
- `job_status`: pending, processing, completed, failed
- `job_type`: gen_thumbnail, ml_style, ml_people, ml_group_location
- `federation_message_type`: share-announcement, share-revocation, picture-update
- `federation_direction`: inbound, outbound
- `federation_status`: pending, sent, delivered, failed
- `safe_delete_mode`: singleBranch, fullDelete
- `service_type`: shared-tag-mapping, rule, segmentation

**Benefits:**
- Type safety at database level
- More efficient storage than VARCHAR
- Clear documentation of valid values

## Table Relationships

```
users
├── pictures (1:N)
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

### `picture_has_tag(picture_uuid UUID, target_tag LTREE)`
Checks if a picture has a tag including virtual ancestors.

**Example:**
```sql
SELECT picture_has_tag('picture-uuid', 'Photos.Travel');
-- Returns: TRUE if picture has Photos.Travel or any descendant
```

### `get_pictures_under_tag(tag_prefix LTREE)`
Returns all non-deleted pictures under a tag prefix.

**Example:**
```sql
SELECT get_pictures_under_tag('Photos.Travel');
-- Returns: All pictures with Photos.Travel or any descendant tag
```

### `get_tag_ancestors(tag_path LTREE)` (Implemented in Rust)
Returns all ancestor tags for a given tag path.

**Rust Implementation:**
```rust
fn get_tag_ancestors(tag_path: &str) -> Vec<String> {
    let parts: Vec<&str> = tag_path.split('.').collect();
    let mut ancestors = Vec::new();
    
    for i in 1..parts.len() {
        ancestors.push(parts[..i].join("."));
    }
    
    ancestors
}
```

## Indexing Strategy

### Tag Queries
- GIN index on `tags.tag_path` for ltree operations
- Composite index on `(picture_id, tag_path)` for lookups

### Picture Queries
- Index on `owner_id` for user's pictures
- Partial index on `deleted_at` for trash queries
- GIN indexes on `exif_data` and `metadata` JSONB columns
- Partial index on `(owner_username, owner_instance_domain)` for received pictures

### Share Queries
- Index on `(recipient_username, recipient_instance)` for incoming shares
- Index on `(owner_id, tag_path)` for outgoing shares

### Job Queries
- Index on `status` for queue processing
- Index on `created_at` for job ordering

### Hierarchy Queries
- GIN index on `config` JSONB for efficient queries

## Migration Strategy

1. **Initial Schema** (`001_initial_schema.sql`):
   - All core tables
   - ENUM types for status fields
   - Indexes and constraints
   - Helper functions
   - Triggers for `updated_at`

2. **Future Migrations**:
   - Add new tagging service types
   - Add new job types
   - Schema optimizations based on query patterns

## Performance Considerations

### Tag Hierarchy Queries
- Use `ltree` operators (`<@`, `@>`) instead of recursive CTEs
- GIN index on `tag_path` provides O(log n) lookups
- Virtual ancestors avoid storing redundant tag records

### Picture Lookups
- Composite index on `(owner_id, picture_id)` for federation
- Partial index on `deleted_at` for active pictures only
- JSONB GIN indexes for EXIF/metadata queries

### Share Queries
- Index on recipient identity for incoming share lookups
- Index on tag path for outgoing share filtering

### Job Processing
- Index on `status` for queue polling
- Composite index on `(owner_id, idempotency_key)` for deduplication

### Hierarchy Queries
- Single query to fetch entire config (no joins)
- GIN index on JSONB for efficient filtering

## Security Considerations

1. **Row-Level Security (RLS)**:
   - Consider enabling RLS for multi-tenant isolation
   - Users can only access their own pictures and shares

2. **Federation Authentication**:
   - Verify sender identity on incoming shares
   - Use ephemeral tokens for cross-instance access

3. **Job Security**:
   - Workers never write directly to backend database
   - Results published via NATS JetStream
   - Backend persists results idempotently

## Future Enhancements

1. **Partitioning**:
   - Partition `jobs` table by `created_at` for time-based cleanup
   - Partition `tags` table by `picture_id` for large instances

2. **Materialized Views**:
   - Pre-compute tag hierarchies for frequently accessed paths
   - Cache picture counts per tag

3. **Full-Text Search**:
   - Add `tsvector` columns for filename/metadata search
   - GIN index for efficient text queries

4. **Audit Logging**:
   - Track tag assignments and removals
   - Log share creation and revocation
   - Monitor federation message delivery
