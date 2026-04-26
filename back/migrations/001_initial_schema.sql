-- Archypix Backend PostgreSQL Schema
-- Initial migration: Core tables for decentralized picture management

-- Enable required extensions
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "ltree";  -- For hierarchical tag paths

-- ============================================================================
-- ENUM TYPES
-- ============================================================================
CREATE TYPE share_status AS ENUM ('active', 'revoked', 'tombstoned');
CREATE TYPE tag_source AS ENUM ('manual', 'rule', 'segment', 'share-mapping', 'incoming-share');
CREATE TYPE job_status AS ENUM ('pending', 'processing', 'completed', 'failed');
CREATE TYPE job_type AS ENUM ('gen_thumbnail', 'ml_style', 'ml_people', 'ml_group_location');
CREATE TYPE federation_message_type AS ENUM ('share-announcement', 'share-revocation', 'picture-update');
CREATE TYPE federation_direction AS ENUM ('inbound', 'outbound');
CREATE TYPE federation_status AS ENUM ('pending', 'sent', 'delivered', 'failed');
CREATE TYPE safe_delete_mode AS ENUM ('singleBranch', 'fullDelete');
CREATE TYPE service_type AS ENUM ('shared-tag-mapping', 'rule', 'segmentation');

-- ============================================================================
-- USERS
-- ============================================================================
CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    username VARCHAR(255) NOT NULL,
    instance_domain VARCHAR(255) NOT NULL,
    email VARCHAR(255),
    display_name VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Composite unique constraint for federation identity
    CONSTRAINT uq_user_identity UNIQUE (username, instance_domain)
);

CREATE INDEX idx_users_instance ON users(instance_domain);
CREATE INDEX idx_users_username ON users(username);

-- ============================================================================
-- PICTURES
-- ============================================================================
CREATE TABLE pictures (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),

    -- Owner reference (local user)
    owner_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    picture_id VARCHAR(255) NOT NULL,  -- Unique within owner's instance

    -- Cross-instance support: original owner info (for received pictures)
    owner_username VARCHAR(255),  -- NULL for owned pictures
    owner_instance_domain VARCHAR(255),  -- NULL for owned pictures

    -- Storage
    s3_key VARCHAR(1024) NOT NULL,  -- Original file location in S3/MinIO
    s3_bucket VARCHAR(255) NOT NULL,

    -- Metadata
    filename VARCHAR(1024),
    mime_type VARCHAR(100),
    file_size BIGINT,
    width INTEGER,
    height INTEGER,

    -- EXIF and other metadata (flexible JSONB)
    exif_data JSONB DEFAULT '{}',

    -- ML/processing results
    metadata JSONB DEFAULT '{}',

    -- Soft deletion (local only - received pictures never physically deleted)
    deleted_at TIMESTAMPTZ,

    -- Timestamps
    captured_at TIMESTAMPTZ,  -- From EXIF
    ingested_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Composite unique constraint: picture_id unique per owner
    CONSTRAINT uq_picture_per_owner UNIQUE (owner_id, picture_id)
);

CREATE INDEX idx_pictures_owner ON pictures(owner_id);
CREATE INDEX idx_pictures_deleted ON pictures(deleted_at) WHERE deleted_at IS NOT NULL;
CREATE INDEX idx_pictures_captured ON pictures(captured_at);
CREATE INDEX idx_pictures_exif ON pictures USING GIN(exif_data);
CREATE INDEX idx_pictures_metadata ON pictures USING GIN(metadata);
CREATE INDEX idx_pictures_owner_identity ON pictures(owner_username, owner_instance_domain)
    WHERE owner_username IS NOT NULL;

-- ============================================================================
-- TAGS
-- ============================================================================
CREATE TABLE tags (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    picture_id UUID NOT NULL REFERENCES pictures(id) ON DELETE CASCADE,

    -- Tag path using ltree for hierarchy (e.g., 'Photos.Travel.Alps')
    -- Stored without leading slash, case-insensitive
    tag_path LTREE NOT NULL,

    -- Whether this tag was explicitly assigned or derived (virtual ancestor)
    is_virtual BOOLEAN NOT NULL DEFAULT FALSE,

    -- Source of the tag assignment
    source tag_source NOT NULL DEFAULT 'manual',
    source_id UUID,  -- Reference to rule/segment/share that created this

    -- Timestamps
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Prevent duplicate tags on same picture
    CONSTRAINT uq_picture_tag UNIQUE (picture_id, tag_path)
);

-- GIN index for efficient ltree operations (@>, <@, ~, @)
CREATE INDEX idx_tags_path ON tags USING GIN(tag_path);
CREATE INDEX idx_tags_picture ON tags(picture_id);
CREATE INDEX idx_tags_source ON tags(source, source_id);

-- ============================================================================
-- OUTGOING SHARES
-- ============================================================================
CREATE TABLE outgoing_shares (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    owner_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,

    -- What is shared
    tag_path LTREE NOT NULL,  -- Tag being shared

    -- Who receives it
    recipient_username VARCHAR(255) NOT NULL,
    recipient_instance VARCHAR(255) NOT NULL,

    -- Share configuration
    allow_share_back BOOLEAN NOT NULL DEFAULT TRUE,
    future BOOLEAN NOT NULL DEFAULT TRUE,  -- Auto-announce new pictures

    -- Status
    status share_status NOT NULL DEFAULT 'active',

    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked_at TIMESTAMPTZ,

    -- Composite unique: one share per recipient per tag
    CONSTRAINT uq_outgoing_share UNIQUE (owner_id, tag_path, recipient_username, recipient_instance)
);

CREATE INDEX idx_outgoing_shares_owner ON outgoing_shares(owner_id);
CREATE INDEX idx_outgoing_shares_recipient ON outgoing_shares(recipient_username, recipient_instance);
CREATE INDEX idx_outgoing_shares_tag ON outgoing_shares USING GIN(tag_path);
CREATE INDEX idx_outgoing_shares_status ON outgoing_shares(status);

-- ============================================================================
-- INCOMING SHARES
-- ============================================================================
CREATE TABLE incoming_shares (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    recipient_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,

    -- Who sent it
    sender_username VARCHAR(255) NOT NULL,
    sender_instance VARCHAR(255) NOT NULL,

    -- Reference to sender's OutgoingShare
    outgoing_share_id UUID NOT NULL,  -- No FK (cross-instance)

    -- Local mapping service (optional)
    local_mapping_service_id UUID,  -- FK added after tagging_services table

    -- Status
    status share_status NOT NULL DEFAULT 'active',

    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked_at TIMESTAMPTZ,

    -- Composite unique: one incoming share per sender per outgoing share
    CONSTRAINT uq_incoming_share UNIQUE (recipient_id, sender_username, sender_instance, outgoing_share_id)
);

CREATE INDEX idx_incoming_shares_recipient ON incoming_shares(recipient_id);
CREATE INDEX idx_incoming_shares_sender ON incoming_shares(sender_username, sender_instance);
CREATE INDEX idx_incoming_shares_status ON incoming_shares(status);

-- ============================================================================
-- TAGGING SERVICES (Base table)
-- ============================================================================
CREATE TABLE tagging_services (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    owner_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,

    -- Service type discriminator (determines pipeline order and trigger labels)
    service_type service_type NOT NULL,

    -- Gate conditions
    requires TEXT[] NOT NULL DEFAULT '{}',  -- Tags required for service to fire
    excludes TEXT[] NOT NULL DEFAULT '{}',  -- Tags that prevent service from firing

    -- Status
    enabled BOOLEAN NOT NULL DEFAULT TRUE,

    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Pipeline order and trigger labels are hardcoded based on service_type:
-- shared-tag-mapping: order=1, triggers=[incoming-share]
-- rule: order=2, triggers=[incoming-share, ingest, metadata, manual-tag, rule-edit]
-- segmentation: order=3, triggers=[incoming-share, ingest, metadata, manual-tag, rule-edit, segmentation-edit]

CREATE INDEX idx_tagging_services_owner ON tagging_services(owner_id);
CREATE INDEX idx_tagging_services_type ON tagging_services(service_type);
CREATE INDEX idx_tagging_services_enabled ON tagging_services(enabled);

-- ============================================================================
-- SHARED TAG MAPPING SERVICE
-- ============================================================================
CREATE TABLE shared_tag_mapping_services (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    service_id UUID NOT NULL REFERENCES tagging_services(id) ON DELETE CASCADE,

    -- Which incoming share to map
    incoming_share_id UUID NOT NULL REFERENCES incoming_shares(id) ON DELETE CASCADE,

    -- Tag to assign
    assign_tag LTREE NOT NULL,

    -- Status (flagged if incoming share is revoked)
    is_broken BOOLEAN NOT NULL DEFAULT FALSE,

    -- Unique mapping per service per incoming share
    CONSTRAINT uq_stms_mapping UNIQUE (service_id, incoming_share_id)
);

CREATE INDEX idx_stms_service ON shared_tag_mapping_services(service_id);
CREATE INDEX idx_stms_incoming_share ON shared_tag_mapping_services(incoming_share_id);

-- Add FK from incoming_shares to shared_tag_mapping_services
ALTER TABLE incoming_shares
ADD CONSTRAINT fk_incoming_shares_mapping
FOREIGN KEY (local_mapping_service_id)
REFERENCES shared_tag_mapping_services(id)
ON DELETE SET NULL;

-- ============================================================================
-- RULE TAGGING SERVICE
-- ============================================================================
CREATE TABLE rule_tagging_services (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    service_id UUID NOT NULL REFERENCES tagging_services(id) ON DELETE CASCADE,

    -- Predicate expression (e.g., "exif.gps within bbox(45.8, 6.8, 46.1, 7.1)")
    predicate TEXT NOT NULL,

    -- Tag to assign
    assign_tag LTREE NOT NULL
);

CREATE INDEX idx_rts_service ON rule_tagging_services(service_id);

-- ============================================================================
-- SEGMENTATION TAGGING SERVICE
-- ============================================================================
CREATE TABLE segmentation_tagging_services (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    service_id UUID NOT NULL REFERENCES tagging_services(id) ON DELETE CASCADE,

    -- Segment definition
    name VARCHAR(255) NOT NULL,

    -- Date range (stored as tsrange for efficient overlap queries)
    date_range TSTZRANGE NOT NULL,

    -- Tag to assign
    assign_tag LTREE NOT NULL,

    -- Parent segment for subsegments
    parent_segment_id UUID REFERENCES segmentation_tagging_services(id) ON DELETE CASCADE
);

CREATE INDEX idx_sts_service ON segmentation_tagging_services(service_id);
CREATE INDEX idx_sts_date_range ON segmentation_tagging_services USING GIST(date_range);
CREATE INDEX idx_sts_parent ON segmentation_tagging_services(parent_segment_id);

-- ============================================================================
-- HIERARCHIES (WebDAV filesystem mappings)
-- ============================================================================
CREATE TABLE hierarchies (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    owner_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,

    -- Hierarchy name
    name VARCHAR(255) NOT NULL,

    -- Configuration as JSONB (simpler than multiple tables)
    config JSONB NOT NULL DEFAULT '{
        "roots": [],
        "collapsedTags": [],
        "disabledTags": [],
        "safeDeleteMode": "singleBranch"
    }',

    -- Status
    enabled BOOLEAN NOT NULL DEFAULT TRUE,

    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Unique name per owner
    CONSTRAINT uq_hierarchy_name UNIQUE (owner_id, name)
);

CREATE INDEX idx_hierarchies_owner ON hierarchies(owner_id);
CREATE INDEX idx_hierarchies_config ON hierarchies USING GIN(config);

-- Config JSONB structure:
-- {
--   "roots": [
--     {"path": "Photos", "keepDir": false},
--     {"path": "Images", "keepDir": true}
--   ],
--   "collapsedTags": ["Photos.Travel.Alps.Hiking"],
--   "disabledTags": ["Photos.Outdoor"],
--   "safeDeleteMode": "singleBranch" | "fullDelete"
-- }

-- ============================================================================
-- JOBS (Async processing queue)
-- ============================================================================
CREATE TABLE jobs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    owner_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,

    -- Job type
    job_type job_type NOT NULL,

    -- Job status
    status job_status NOT NULL DEFAULT 'pending',

    -- Configuration (job-specific params, may include picture IDs)
    config JSONB NOT NULL DEFAULT '{}',

    -- Result (populated when completed)
    result JSONB DEFAULT '{}',
    result_s3_keys TEXT[],  -- S3 keys of generated artifacts

    -- Error handling
    error_message TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0,
    max_retries INTEGER NOT NULL DEFAULT 3,

    -- Idempotency
    idempotency_key VARCHAR(255) UNIQUE,

    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,

    -- Ensure idempotency
    CONSTRAINT uq_job_idempotency UNIQUE (owner_id, idempotency_key)
);

CREATE INDEX idx_jobs_owner ON jobs(owner_id);
CREATE INDEX idx_jobs_status ON jobs(status);
CREATE INDEX idx_jobs_type ON jobs(job_type);
CREATE INDEX idx_jobs_created ON jobs(created_at);

-- Config JSONB structure examples:
-- gen_thumbnail: {"picture_ids": ["uuid1", "uuid2"], "sizes": ["thumb", "medium"]}
-- ml_people: {"picture_ids": ["uuid1"], "snapshot_version": "v1.2.3"}
-- ml_group_location: {"picture_ids": ["uuid1", "uuid2", "uuid3"]}

-- ============================================================================
-- FEDERATION MESSAGES (Outbound/inbound federation log)
-- ============================================================================
CREATE TABLE federation_messages (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),

    -- Message type
    message_type federation_message_type NOT NULL,

    -- Direction
    direction federation_direction NOT NULL,

    -- Source/destination
    sender_username VARCHAR(255),
    sender_instance VARCHAR(255),
    recipient_username VARCHAR(255),
    recipient_instance VARCHAR(255),

    -- Related entities (optional, for correlation)
    outgoing_share_id UUID REFERENCES outgoing_shares(id) ON DELETE SET NULL,
    incoming_share_id UUID REFERENCES incoming_shares(id) ON DELETE SET NULL,

    -- Payload (may include picture IDs and other data)
    payload JSONB NOT NULL DEFAULT '{}',

    -- Status
    status federation_status NOT NULL DEFAULT 'pending',

    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    sent_at TIMESTAMPTZ,
    delivered_at TIMESTAMPTZ,

    -- Error handling
    error_message TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_federation_messages_type ON federation_messages(message_type);
CREATE INDEX idx_federation_messages_direction ON federation_messages(direction);
CREATE INDEX idx_federation_messages_status ON federation_messages(status);
CREATE INDEX idx_federation_messages_sender ON federation_messages(sender_username, sender_instance);
CREATE INDEX idx_federation_messages_recipient ON federation_messages(recipient_username, recipient_instance);

-- Payload JSONB structure examples:
-- share-announcement: {"picture_ids": ["uuid1", "uuid2"], "tag_path": "Photos.Travel.Alps"}
-- share-revocation: {"reason": "user_request"}
-- picture-update: {"picture_ids": ["uuid1"], "update_type": "metadata"}

-- ============================================================================
-- HELPER FUNCTIONS
-- ============================================================================

-- Function to check if a picture has a tag (including virtual ancestors)
-- Note: get_tag_ancestors is implemented in Rust using ltree operators
CREATE OR REPLACE FUNCTION picture_has_tag(picture_uuid UUID, target_tag LTREE)
RETURNS BOOLEAN AS $$
BEGIN
    RETURN EXISTS (
        SELECT 1 FROM tags
        WHERE picture_id = picture_uuid
        AND (
            tag_path = target_tag
            OR tag_path <@ target_tag  -- tag_path is descendant of target_tag
            OR target_tag <@ tag_path  -- target_tag is descendant of tag_path (ancestor check)
        )
    );
END;
$$ LANGUAGE plpgsql STABLE;

-- Function to get all pictures under a tag (including descendants)
CREATE OR REPLACE FUNCTION get_pictures_under_tag(tag_prefix LTREE)
RETURNS TABLE(picture_id UUID) AS $$
BEGIN
    RETURN QUERY
    SELECT DISTINCT t.picture_id
    FROM tags t
    WHERE t.tag_path <@ tag_prefix  -- tag_path is descendant of or equal to tag_prefix
    AND NOT EXISTS (
        SELECT 1 FROM pictures p
        WHERE p.id = t.picture_id
        AND p.deleted_at IS NOT NULL
    );
END;
$$ LANGUAGE plpgsql STABLE;

-- ============================================================================
-- TRIGGERS
-- ============================================================================

-- Auto-update updated_at timestamp
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER update_users_updated_at BEFORE UPDATE ON users
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_pictures_updated_at BEFORE UPDATE ON pictures
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_tagging_services_updated_at BEFORE UPDATE ON tagging_services
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_hierarchies_updated_at BEFORE UPDATE ON hierarchies
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- ============================================================================
-- COMMENTS (Documentation)
-- ============================================================================

COMMENT ON TABLE users IS 'User accounts with federation identity (@username:instance.com)';
COMMENT ON TABLE pictures IS 'Picture metadata with composite key (owner_id, picture_id); owner_username/owner_instance_domain for received pictures';
COMMENT ON TABLE tags IS 'Tag assignments using ltree for hierarchical paths; is_virtual marks derived ancestors';
COMMENT ON TABLE outgoing_shares IS 'Shares created by users to share tags with other users';
COMMENT ON TABLE incoming_shares IS 'Shares received from other users';
COMMENT ON TABLE tagging_services IS 'Base table for tagging service pipeline; service_type determines order and triggers';
COMMENT ON TABLE shared_tag_mapping_services IS 'Maps incoming shares to local tags';
COMMENT ON TABLE rule_tagging_services IS 'Assigns tags based on EXIF/metadata predicates';
COMMENT ON TABLE segmentation_tagging_services IS 'Assigns tags based on date ranges with subsegment support';
COMMENT ON TABLE hierarchies IS 'WebDAV filesystem mappings; config JSONB stores roots, collapsedTags, disabledTags, safeDeleteMode';
COMMENT ON TABLE jobs IS 'Async processing queue; config JSONB holds job-specific params (may include picture IDs)';
COMMENT ON TABLE federation_messages IS 'Federation message log; payload JSONB holds message data (may include picture IDs)';

COMMENT ON FUNCTION picture_has_tag IS 'Checks if a picture has a tag including virtual ancestors';
COMMENT ON FUNCTION get_pictures_under_tag IS 'Returns all non-deleted pictures under a tag prefix';

-- Pipeline order and trigger labels (hardcoded, not stored):
-- shared-tag-mapping: order=1, triggers=[incoming-share]
-- rule: order=2, triggers=[incoming-share, ingest, metadata, manual-tag, rule-edit]
-- segmentation: order=3, triggers=[incoming-share, ingest, metadata, manual-tag, rule-edit, segmentation-edit]
