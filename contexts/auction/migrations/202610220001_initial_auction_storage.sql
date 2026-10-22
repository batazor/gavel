-- Auction owns this schema. It is intentionally additive and forward-only.

CREATE TABLE auction_streams (
    auction_id TEXT PRIMARY KEY,
    current_version BIGINT NOT NULL DEFAULT 0 CHECK (current_version >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

CREATE TABLE auction_events (
    auction_id TEXT NOT NULL REFERENCES auction_streams(auction_id) ON DELETE RESTRICT,
    version BIGINT NOT NULL CHECK (version > 0),
    event_id UUID NOT NULL UNIQUE,
    event_type TEXT NOT NULL CHECK (event_type <> ''),
    event_schema_version INTEGER NOT NULL CHECK (event_schema_version > 0),
    payload JSONB NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (auction_id, version)
);

CREATE INDEX auction_events_replay_idx
    ON auction_events (auction_id, version ASC);

CREATE TABLE auction_snapshots (
    auction_id TEXT PRIMARY KEY REFERENCES auction_streams(auction_id) ON DELETE CASCADE,
    version BIGINT NOT NULL CHECK (version >= 0),
    snapshot_schema_version INTEGER NOT NULL CHECK (snapshot_schema_version > 0),
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

CREATE TABLE auction_command_results (
    principal_scope TEXT NOT NULL CHECK (principal_scope <> ''),
    idempotency_key TEXT NOT NULL CHECK (idempotency_key <> ''),
    request_fingerprint BYTEA NOT NULL CHECK (octet_length(request_fingerprint) = 32),
    operation TEXT NOT NULL CHECK (operation <> ''),
    result_type TEXT NOT NULL CHECK (result_type <> ''),
    result_payload JSONB NOT NULL,
    stream_version BIGINT NOT NULL CHECK (stream_version >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (principal_scope, idempotency_key)
);

CREATE TABLE auction_public_projection (
    auction_id TEXT PRIMARY KEY REFERENCES auction_streams(auction_id) ON DELETE CASCADE,
    lot_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK (
        status IN ('SCHEDULED', 'OPEN', 'SETTLING', 'SOLD', 'UNSOLD', 'CANCELLED')
    ),
    amount_minor NUMERIC(20, 0) NOT NULL CHECK (amount_minor >= 0),
    currency_code TEXT NOT NULL CHECK (currency_code ~ '^[A-Z]{3}$'),
    ends_at TIMESTAMPTZ NOT NULL,
    stream_version BIGINT NOT NULL CHECK (stream_version >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

CREATE UNIQUE INDEX auction_public_projection_lot_idx
    ON auction_public_projection (lot_id);

-- Debezium captures only this table. Private event payloads are never exposed.
CREATE TABLE auction_outbox (
    event_id UUID PRIMARY KEY,
    aggregate_type TEXT NOT NULL DEFAULT 'auction' CHECK (aggregate_type = 'auction'),
    aggregate_id TEXT NOT NULL,
    aggregate_version BIGINT NOT NULL CHECK (aggregate_version > 0),
    event_type TEXT NOT NULL CHECK (event_type <> ''),
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    occurred_at TIMESTAMPTZ NOT NULL,
    payload BYTEA NOT NULL,
    correlation_id TEXT NOT NULL CHECK (correlation_id <> ''),
    causation_id TEXT NOT NULL CHECK (causation_id <> '')
);

CREATE INDEX auction_outbox_aggregate_order_idx
    ON auction_outbox (aggregate_id, aggregate_version, occurred_at);

CREATE TABLE auction_inbox (
    consumer TEXT NOT NULL CHECK (consumer <> ''),
    event_id TEXT NOT NULL CHECK (event_id <> ''),
    payload_fingerprint BYTEA NOT NULL CHECK (octet_length(payload_fingerprint) = 32),
    status TEXT NOT NULL DEFAULT 'PROCESSED' CHECK (status IN ('PROCESSED', 'QUARANTINED')),
    processed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (consumer, event_id)
);

CREATE TABLE payment_readiness (
    user_id TEXT PRIMARY KEY,
    ready BOOLEAN NOT NULL,
    source_version BIGINT NOT NULL CHECK (source_version >= 0),
    source_event_id TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE auction_reconciliation_queue (
    operation_id TEXT PRIMARY KEY,
    resource_type TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    relation TEXT NOT NULL,
    subject_type TEXT NOT NULL,
    subject_id TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    available_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    last_error_code TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    completed_at TIMESTAMPTZ
);

CREATE INDEX auction_reconciliation_pending_idx
    ON auction_reconciliation_queue (available_at, created_at)
    WHERE completed_at IS NULL;
