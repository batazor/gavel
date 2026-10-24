CREATE TABLE notifications (
    notification_id text PRIMARY KEY,
    recipient_id text NOT NULL,
    template_ref text NOT NULL,
    fingerprint text NOT NULL,
    status text NOT NULL CHECK (status IN ('pending', 'sent', 'failed')),
    attempt_count integer NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    version bigint NOT NULL CHECK (version > 0),
    aggregate jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX notifications_status_updated_idx ON notifications(status, updated_at, notification_id);

CREATE TABLE notification_delivery_attempts (
    notification_id text NOT NULL REFERENCES notifications(notification_id),
    attempt_number integer NOT NULL CHECK (attempt_number > 0),
    outcome text NOT NULL CHECK (outcome IN ('sent', 'transient_failure', 'permanent_failure', 'retry_exhausted')),
    safe_detail text,
    occurred_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (notification_id, attempt_number)
);

CREATE TABLE notification_outbox (
    event_id text PRIMARY KEY,
    event_type text NOT NULL CHECK (event_type IN ('NotificationSent', 'NotificationFailed')),
    payload jsonb NOT NULL,
    occurred_at timestamptz NOT NULL DEFAULT now(),
    published_at timestamptz
);

CREATE INDEX notification_outbox_pending_idx ON notification_outbox(occurred_at, event_id) WHERE published_at IS NULL;
