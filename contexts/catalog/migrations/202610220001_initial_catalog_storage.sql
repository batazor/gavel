-- Catalog owns this schema. This migration is additive and forward-only.

CREATE TABLE catalog_lots (
    lot_id TEXT PRIMARY KEY CHECK (lot_id <> ''),
    seller_id TEXT NOT NULL CHECK (seller_id <> ''),
    title TEXT NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
    description TEXT CHECK (description IS NULL OR char_length(description) <= 5000),
    start_price_minor NUMERIC(20, 0) NOT NULL CHECK (start_price_minor > 0),
    currency_code TEXT NOT NULL CHECK (currency_code ~ '^[A-Z]{3}$'),
    bid_step_minor NUMERIC(20, 0) NOT NULL CHECK (bid_step_minor > 0),
    scheduled_end_millis BIGINT NOT NULL CHECK (scheduled_end_millis > 0),
    inactivity_millis BIGINT CHECK (inactivity_millis IS NULL OR inactivity_millis > 0),
    hard_deadline_millis BIGINT CHECK (
        hard_deadline_millis IS NULL OR hard_deadline_millis >= scheduled_end_millis
    ),
    state TEXT NOT NULL CHECK (state IN ('LISTED', 'WITHDRAWN')),
    version BIGINT NOT NULL CHECK (version > 0),
    authz_revision TEXT NOT NULL CHECK (authz_revision <> ''),
    description_frozen BOOLEAN NOT NULL DEFAULT FALSE,
    auction_source_version BIGINT NOT NULL DEFAULT 0 CHECK (auction_source_version >= 0),
    listed_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

CREATE INDEX catalog_lots_public_page_idx
    ON catalog_lots (listed_at ASC, lot_id ASC)
    WHERE state = 'LISTED';

CREATE TABLE catalog_lot_images (
    image_id TEXT PRIMARY KEY CHECK (image_id <> ''),
    owner_id TEXT NOT NULL CHECK (owner_id <> ''),
    object_key TEXT NOT NULL UNIQUE CHECK (object_key <> ''),
    expected_content_type TEXT NOT NULL CHECK (
        expected_content_type IN ('image/jpeg', 'image/png', 'image/webp')
    ),
    expected_content_length BIGINT NOT NULL CHECK (
        expected_content_length BETWEEN 1 AND 10485760
    ),
    expected_checksum_sha256 TEXT NOT NULL CHECK (expected_checksum_sha256 <> ''),
    expires_at TIMESTAMPTZ NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('PENDING', 'READY', 'ATTACHED', 'INVALID')),
    ready_at TIMESTAMPTZ,
    lot_id TEXT REFERENCES catalog_lots(lot_id) ON DELETE RESTRICT,
    position SMALLINT,
    alt_text TEXT CHECK (alt_text IS NULL OR char_length(alt_text) <= 500),
    cleanup_lease_owner TEXT,
    cleanup_lease_token UUID,
    cleanup_lease_until TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    CHECK (
        (state = 'PENDING' AND ready_at IS NULL AND lot_id IS NULL AND position IS NULL AND alt_text IS NULL)
        OR (state = 'READY' AND ready_at IS NOT NULL AND lot_id IS NULL AND position IS NULL AND alt_text IS NULL)
        OR (state = 'INVALID' AND lot_id IS NULL AND position IS NULL AND alt_text IS NULL)
        OR (state = 'ATTACHED' AND ready_at IS NOT NULL AND lot_id IS NOT NULL AND position BETWEEN 0 AND 19 AND alt_text IS NOT NULL)
    ),
    CHECK (
        (cleanup_lease_owner IS NULL AND cleanup_lease_token IS NULL AND cleanup_lease_until IS NULL)
        OR (cleanup_lease_owner IS NOT NULL AND cleanup_lease_token IS NOT NULL AND cleanup_lease_until IS NOT NULL)
    )
);

CREATE UNIQUE INDEX catalog_lot_images_position_idx
    ON catalog_lot_images (lot_id, position)
    WHERE state = 'ATTACHED';

CREATE INDEX catalog_lot_images_cleanup_idx
    ON catalog_lot_images (state, expires_at, ready_at)
    WHERE state <> 'ATTACHED';

CREATE TABLE catalog_command_receipts (
    caller_scope TEXT NOT NULL CHECK (caller_scope <> ''),
    idempotency_key TEXT NOT NULL CHECK (idempotency_key <> ''),
    command TEXT NOT NULL CHECK (command <> ''),
    request_fingerprint BYTEA NOT NULL CHECK (octet_length(request_fingerprint) = 32),
    status TEXT NOT NULL CHECK (status IN ('CLAIMED', 'COMPLETED')),
    claim_token UUID NOT NULL UNIQUE,
    result_payload JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (caller_scope, idempotency_key),
    CHECK (
        (status = 'CLAIMED' AND result_payload IS NULL)
        OR (status = 'COMPLETED' AND result_payload IS NOT NULL)
    )
);

CREATE TABLE catalog_inbox (
    consumer TEXT NOT NULL CHECK (consumer <> ''),
    event_id TEXT NOT NULL CHECK (event_id <> ''),
    payload_fingerprint BYTEA NOT NULL CHECK (octet_length(payload_fingerprint) = 32),
    aggregate_id TEXT NOT NULL CHECK (aggregate_id <> ''),
    source_version BIGINT NOT NULL CHECK (source_version > 0),
    status TEXT NOT NULL CHECK (status IN ('PROCESSED', 'STALE', 'QUARANTINED')),
    processed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (consumer, event_id)
);

CREATE INDEX catalog_inbox_monotonic_idx
    ON catalog_inbox (consumer, aggregate_id, source_version DESC);

CREATE TABLE catalog_outbox (
    event_id UUID PRIMARY KEY,
    aggregate_type TEXT NOT NULL DEFAULT 'lot' CHECK (aggregate_type = 'lot'),
    aggregate_id TEXT NOT NULL REFERENCES catalog_lots(lot_id) ON DELETE RESTRICT,
    aggregate_version BIGINT NOT NULL CHECK (aggregate_version > 0),
    event_type TEXT NOT NULL CHECK (event_type IN ('LotListed', 'LotUpdated', 'LotWithdrawn')),
    schema_version INTEGER NOT NULL DEFAULT 1 CHECK (schema_version > 0),
    occurred_at TIMESTAMPTZ NOT NULL,
    payload BYTEA NOT NULL CHECK (octet_length(payload) > 0),
    correlation_id TEXT NOT NULL CHECK (correlation_id <> ''),
    published_at TIMESTAMPTZ,
    lease_owner TEXT,
    lease_until TIMESTAMPTZ,
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    UNIQUE (aggregate_id, aggregate_version, event_type),
    CHECK ((lease_owner IS NULL) = (lease_until IS NULL))
);

CREATE INDEX catalog_outbox_pending_idx
    ON catalog_outbox (created_at, event_id)
    WHERE published_at IS NULL;

CREATE TABLE catalog_reconciliation_queue (
    operation_id TEXT PRIMARY KEY CHECK (operation_id <> ''),
    operation_type TEXT NOT NULL CHECK (operation_type IN ('MEDIA_DELETE', 'AUTHZ_GRANT_DELETE')),
    resource_id TEXT NOT NULL CHECK (resource_id <> ''),
    owner_id TEXT,
    idempotency_key TEXT NOT NULL CHECK (idempotency_key <> ''),
    not_before TIMESTAMPTZ NOT NULL,
    lease_owner TEXT,
    lease_token UUID,
    lease_until TIMESTAMPTZ,
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    last_error_code TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    completed_at TIMESTAMPTZ,
    UNIQUE (operation_type, resource_id),
    CHECK (
        (lease_owner IS NULL AND lease_token IS NULL AND lease_until IS NULL)
        OR (lease_owner IS NOT NULL AND lease_token IS NOT NULL AND lease_until IS NOT NULL)
    )
);

CREATE INDEX catalog_reconciliation_due_idx
    ON catalog_reconciliation_queue (not_before, created_at)
    WHERE completed_at IS NULL;
