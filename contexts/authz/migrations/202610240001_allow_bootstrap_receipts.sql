-- Bootstrap is an authenticated Authz caller and uses the same durable command
-- receipt path as the runtime workloads.

ALTER TABLE authz_command_receipts
    DROP CONSTRAINT authz_command_receipts_caller_scope_check;

ALTER TABLE authz_command_receipts
    ADD CONSTRAINT authz_command_receipts_caller_scope_check CHECK (
        caller_scope IN (
            'bootstrap', 'bff', 'auction', 'catalog', 'payments', 'notify', 'processes'
        )
    );
