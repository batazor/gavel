ALTER TABLE payment_outbox
    ADD COLUMN aggregate_id TEXT;

UPDATE payment_outbox
SET aggregate_id = COALESCE(payload->>'aggregateId', event_id)
WHERE aggregate_id IS NULL;

ALTER TABLE payment_outbox
    ALTER COLUMN aggregate_id SET NOT NULL,
    ALTER COLUMN topic SET DEFAULT 'gavel.payments.instruments.v1';

UPDATE payment_outbox
SET topic = 'gavel.payments.attempts.v1'
WHERE event_type IN (
    'PaymentAttemptStarted',
    'PaymentRecoveryRequired',
    'PaymentSucceeded',
    'PaymentExpired'
);

UPDATE payment_outbox
SET topic = 'gavel.payments.instruments.v1'
WHERE event_type IN ('PaymentInstrumentReady', 'PaymentInstrumentRevoked');
