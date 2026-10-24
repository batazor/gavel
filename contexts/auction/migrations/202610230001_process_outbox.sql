-- Restricted integration events for the Processes workload. Debezium must
-- publish this table to an ACL-protected topic, never to the public topic.
CREATE TABLE auction_process_outbox (
    event_id UUID PRIMARY KEY,
    aggregate_type TEXT NOT NULL DEFAULT 'auction' CHECK (aggregate_type = 'auction'),
    aggregate_id TEXT NOT NULL REFERENCES auction_streams(auction_id) ON DELETE RESTRICT,
    aggregate_version BIGINT NOT NULL CHECK (aggregate_version > 0),
    event_type TEXT NOT NULL CHECK (
        event_type IN (
            'SettlementCandidateSelected',
            'AuctionUnsold',
            'WinnerConfirmed',
            'AuctionCancelled'
        )
    ),
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    occurred_at TIMESTAMPTZ NOT NULL,
    payload BYTEA NOT NULL CHECK (octet_length(payload) > 0),
    correlation_id TEXT NOT NULL CHECK (correlation_id <> ''),
    causation_id TEXT NOT NULL CHECK (causation_id <> ''),
    UNIQUE (aggregate_id, aggregate_version, event_type)
);

CREATE INDEX auction_process_outbox_aggregate_order_idx
    ON auction_process_outbox (aggregate_id, aggregate_version, occurred_at, event_id);
