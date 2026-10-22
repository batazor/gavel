-- Authz owns this forward-only receipt schema.

CREATE TABLE authz_command_receipts (
    caller_scope TEXT NOT NULL CHECK (
        caller_scope IN ('bff', 'auction', 'catalog', 'payments', 'notify', 'processes')
    ),
    idempotency_key TEXT NOT NULL CHECK (
        idempotency_key <> '' AND octet_length(idempotency_key) <= 256
    ),
    request_fingerprint BYTEA NOT NULL CHECK (octet_length(request_fingerprint) = 32),
    operation TEXT NOT NULL CHECK (
        operation IN ('register_principal', 'write_relationships')
    ),
    status TEXT NOT NULL CHECK (status IN ('PENDING', 'COMPLETED')),
    lease_token UUID,
    lease_expires_at TIMESTAMPTZ,
    attempts INTEGER NOT NULL DEFAULT 1 CHECK (attempts > 0),
    result_payload JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (caller_scope, idempotency_key),
    CONSTRAINT authz_command_receipts_state_check CHECK (
        (
            status = 'PENDING'
            AND lease_token IS NOT NULL
            AND lease_expires_at IS NOT NULL
            AND result_payload IS NULL
        )
        OR
        (
            status = 'COMPLETED'
            AND lease_token IS NULL
            AND lease_expires_at IS NULL
            AND result_payload IS NOT NULL
            AND jsonb_typeof(result_payload) = 'object'
        )
    )
);

CREATE INDEX authz_command_receipts_recovery_idx
    ON authz_command_receipts (lease_expires_at ASC)
    WHERE status = 'PENDING';
