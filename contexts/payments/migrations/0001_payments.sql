CREATE TABLE payment_instruments (
    instrument_id text PRIMARY KEY,
    owner_id text NOT NULL UNIQUE,
    provider_session_ref text NOT NULL UNIQUE,
    state text NOT NULL CHECK (state IN ('pending', 'ready', 'revoked')),
    version bigint NOT NULL CHECK (version > 0),
    aggregate jsonb NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX payment_instruments_state_updated_idx
    ON payment_instruments (state, updated_at, instrument_id);

CREATE TABLE payment_attempts (
    attempt_id text PRIMARY KEY,
    auction_id text NOT NULL,
    settlement_round integer NOT NULL CHECK (settlement_round > 0),
    candidate_id text NOT NULL,
    provider_operation_ref text,
    state text NOT NULL CHECK (state IN ('pending', 'processing', 'recovery_required', 'succeeded', 'expired')),
    version bigint NOT NULL CHECK (version > 0),
    aggregate jsonb NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (auction_id, settlement_round)
);

CREATE UNIQUE INDEX payment_attempts_one_success_per_auction_idx
    ON payment_attempts (auction_id) WHERE state = 'succeeded';
CREATE INDEX payment_attempts_reconciliation_idx
    ON payment_attempts (state, updated_at, attempt_id)
    WHERE state IN ('processing', 'recovery_required');
CREATE INDEX payment_attempts_provider_operation_idx
    ON payment_attempts (provider_operation_ref) WHERE provider_operation_ref IS NOT NULL;

CREATE TABLE payment_command_receipts (
    scope text NOT NULL,
    command text NOT NULL,
    key_digest text NOT NULL,
    fingerprint text NOT NULL,
    claim_token uuid NOT NULL,
    lease_until timestamptz NOT NULL,
    result_payload bytea,
    completed_at timestamptz,
    PRIMARY KEY (scope, command, key_digest)
);
CREATE INDEX payment_command_receipts_lease_idx
    ON payment_command_receipts (lease_until) WHERE completed_at IS NULL;

CREATE TABLE payment_callback_inbox (
    provider text NOT NULL,
    event_ref_digest text NOT NULL,
    fingerprint text NOT NULL,
    received_at timestamptz NOT NULL DEFAULT now(),
    lease_until timestamptz NOT NULL,
    completed_at timestamptz,
    PRIMARY KEY (provider, event_ref_digest)
);
CREATE INDEX payment_callback_inbox_lease_idx
    ON payment_callback_inbox (lease_until) WHERE completed_at IS NULL;

CREATE TABLE payment_outbox (
    event_id text PRIMARY KEY,
    topic text NOT NULL DEFAULT 'payments.events',
    event_type text NOT NULL,
    payload jsonb NOT NULL,
    occurred_at timestamptz NOT NULL DEFAULT now(),
    published_at timestamptz
);
CREATE INDEX payment_outbox_pending_idx ON payment_outbox (occurred_at, event_id) WHERE published_at IS NULL;
