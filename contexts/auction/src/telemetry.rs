use std::sync::{
    OnceLock,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Copy, Debug)]
pub enum CommandOperation {
    Open,
    PlaceBid,
    Reschedule,
    Close,
    Cancel,
    Forfeit,
    Confirm,
    Unknown,
}

impl CommandOperation {
    pub fn from_name(value: &str) -> Self {
        match value {
            "open_auction_from_lot_listed" => Self::Open,
            "place_bid" => Self::PlaceBid,
            "reschedule_auction" => Self::Reschedule,
            "close_auction" => Self::Close,
            "cancel_auction" => Self::Cancel,
            "forfeit_settlement_candidate" => Self::Forfeit,
            "confirm_winner" => Self::Confirm,
            _ => Self::Unknown,
        }
    }

    fn index(self) -> usize {
        self as usize
    }
    fn label(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::PlaceBid => "place_bid",
            Self::Reschedule => "reschedule",
            Self::Close => "close",
            Self::Cancel => "cancel",
            Self::Forfeit => "forfeit",
            Self::Confirm => "confirm",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum CommandOutcome {
    Started,
    Succeeded,
    Replayed,
    Rejected,
    Failed,
}
impl CommandOutcome {
    fn index(self) -> usize {
        self as usize
    }
    fn label(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Succeeded => "succeeded",
            Self::Replayed => "replayed",
            Self::Rejected => "rejected",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum IdempotencyOutcome {
    Miss,
    Hit,
    Conflict,
}
impl IdempotencyOutcome {
    fn index(self) -> usize {
        self as usize
    }
    fn label(self) -> &'static str {
        match self {
            Self::Miss => "miss",
            Self::Hit => "hit",
            Self::Conflict => "conflict",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum InboxMetricOutcome {
    Accepted,
    Duplicate,
    Conflict,
}
impl InboxMetricOutcome {
    fn index(self) -> usize {
        self as usize
    }
    fn label(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Duplicate => "duplicate",
            Self::Conflict => "conflict",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ConsumerMetricOutcome {
    Applied,
    Duplicate,
    Stale,
    Quarantined,
    Failed,
}
impl ConsumerMetricOutcome {
    fn index(self) -> usize {
        self as usize
    }
    fn label(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Duplicate => "duplicate",
            Self::Stale => "stale",
            Self::Quarantined => "quarantined",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum CacheTier {
    L1,
    Redis,
    Postgres,
}
impl CacheTier {
    fn index(self) -> usize {
        self as usize
    }
    fn label(self) -> &'static str {
        match self {
            Self::L1 => "l1",
            Self::Redis => "redis",
            Self::Postgres => "postgres",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum CacheOutcome {
    Hit,
    Miss,
    Failure,
    Write,
}
impl CacheOutcome {
    fn index(self) -> usize {
        self as usize
    }
    fn label(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Miss => "miss",
            Self::Failure => "failure",
            Self::Write => "write",
        }
    }
}

struct Telemetry {
    commands: Vec<AtomicU64>,
    retries: Vec<AtomicU64>,
    idempotency: Vec<AtomicU64>,
    replayed_events: AtomicU64,
    outbox_events: AtomicU64,
    inbox: Vec<AtomicU64>,
    consumers: Vec<AtomicU64>,
    cache: Vec<AtomicU64>,
}

impl Telemetry {
    fn new() -> Self {
        let counters = |length| (0..length).map(|_| AtomicU64::new(0)).collect();
        Self {
            commands: counters(8 * 5),
            retries: counters(8),
            idempotency: counters(3),
            replayed_events: AtomicU64::new(0),
            outbox_events: AtomicU64::new(0),
            inbox: counters(3),
            consumers: counters(5),
            cache: counters(3 * 4),
        }
    }
}

fn telemetry() -> &'static Telemetry {
    static VALUE: OnceLock<Telemetry> = OnceLock::new();
    VALUE.get_or_init(Telemetry::new)
}

pub fn record_command(operation: CommandOperation, outcome: CommandOutcome) {
    telemetry().commands[operation.index() * 5 + outcome.index()].fetch_add(1, Ordering::Relaxed);
    tracing::info!(target: "gavel.auction.command", operation = operation.label(), outcome = outcome.label(), "auction command");
}
pub fn record_retry(operation: CommandOperation) {
    telemetry().retries[operation.index()].fetch_add(1, Ordering::Relaxed);
}
pub fn record_idempotency(outcome: IdempotencyOutcome) {
    telemetry().idempotency[outcome.index()].fetch_add(1, Ordering::Relaxed);
}
pub fn record_replay(events: usize) {
    telemetry()
        .replayed_events
        .fetch_add(events as u64, Ordering::Relaxed);
}
pub fn record_outbox(events: usize) {
    telemetry()
        .outbox_events
        .fetch_add(events as u64, Ordering::Relaxed);
}
pub fn record_inbox(outcome: InboxMetricOutcome) {
    telemetry().inbox[outcome.index()].fetch_add(1, Ordering::Relaxed);
}
pub fn record_consumer(outcome: ConsumerMetricOutcome) {
    telemetry().consumers[outcome.index()].fetch_add(1, Ordering::Relaxed);
}
pub fn record_cache(tier: CacheTier, outcome: CacheOutcome) {
    telemetry().cache[tier.index() * 4 + outcome.index()].fetch_add(1, Ordering::Relaxed);
}

pub fn render_prometheus() -> String {
    let telemetry = telemetry();
    let mut output = String::from(
        "# TYPE gavel_service_info gauge\ngavel_service_info{service=\"auction\"} 1\n",
    );
    for operation in [
        CommandOperation::Open,
        CommandOperation::PlaceBid,
        CommandOperation::Reschedule,
        CommandOperation::Close,
        CommandOperation::Cancel,
        CommandOperation::Forfeit,
        CommandOperation::Confirm,
        CommandOperation::Unknown,
    ] {
        for outcome in [
            CommandOutcome::Started,
            CommandOutcome::Succeeded,
            CommandOutcome::Replayed,
            CommandOutcome::Rejected,
            CommandOutcome::Failed,
        ] {
            push(
                &mut output,
                "gavel_auction_commands_total",
                &[
                    ("operation", operation.label()),
                    ("outcome", outcome.label()),
                ],
                telemetry.commands[operation.index() * 5 + outcome.index()].load(Ordering::Relaxed),
            );
        }
        push(
            &mut output,
            "gavel_auction_optimistic_retries_total",
            &[("operation", operation.label())],
            telemetry.retries[operation.index()].load(Ordering::Relaxed),
        );
    }
    for outcome in [
        IdempotencyOutcome::Miss,
        IdempotencyOutcome::Hit,
        IdempotencyOutcome::Conflict,
    ] {
        push(
            &mut output,
            "gavel_auction_idempotency_total",
            &[("outcome", outcome.label())],
            telemetry.idempotency[outcome.index()].load(Ordering::Relaxed),
        );
    }
    push(
        &mut output,
        "gavel_auction_stream_replay_events_total",
        &[],
        telemetry.replayed_events.load(Ordering::Relaxed),
    );
    push(
        &mut output,
        "gavel_auction_outbox_events_total",
        &[],
        telemetry.outbox_events.load(Ordering::Relaxed),
    );
    for outcome in [
        InboxMetricOutcome::Accepted,
        InboxMetricOutcome::Duplicate,
        InboxMetricOutcome::Conflict,
    ] {
        push(
            &mut output,
            "gavel_auction_inbox_total",
            &[("outcome", outcome.label())],
            telemetry.inbox[outcome.index()].load(Ordering::Relaxed),
        );
    }
    for outcome in [
        ConsumerMetricOutcome::Applied,
        ConsumerMetricOutcome::Duplicate,
        ConsumerMetricOutcome::Stale,
        ConsumerMetricOutcome::Quarantined,
        ConsumerMetricOutcome::Failed,
    ] {
        push(
            &mut output,
            "gavel_auction_consumer_events_total",
            &[("outcome", outcome.label())],
            telemetry.consumers[outcome.index()].load(Ordering::Relaxed),
        );
    }
    for tier in [CacheTier::L1, CacheTier::Redis, CacheTier::Postgres] {
        for outcome in [
            CacheOutcome::Hit,
            CacheOutcome::Miss,
            CacheOutcome::Failure,
            CacheOutcome::Write,
        ] {
            push(
                &mut output,
                "gavel_auction_cache_operations_total",
                &[("tier", tier.label()), ("outcome", outcome.label())],
                telemetry.cache[tier.index() * 4 + outcome.index()].load(Ordering::Relaxed),
            );
        }
    }
    output
}

fn push(output: &mut String, name: &str, labels: &[(&str, &str)], value: u64) {
    output.push_str(name);
    if !labels.is_empty() {
        output.push('{');
        for (index, (key, value)) in labels.iter().enumerate() {
            if index > 0 {
                output.push(',');
            }
            output.push_str(key);
            output.push_str("=\"");
            output.push_str(value);
            output.push('"');
        }
        output.push('}');
    }
    output.push(' ');
    output.push_str(&value.to_string());
    output.push('\n');
}

#[cfg(test)]
mod tests {
    use std::{
        io::Write,
        sync::{Arc, Mutex},
    };

    use super::*;
    use tracing::subscriber::with_default;
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Buffer {
        type Writer = Buffer;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn metric_contract_has_bounded_labels_and_no_secret_dimensions() {
        record_command(CommandOperation::PlaceBid, CommandOutcome::Succeeded);
        record_cache(CacheTier::Redis, CacheOutcome::Hit);
        let rendered = render_prometheus();
        for name in [
            "gavel_auction_commands_total",
            "gavel_auction_optimistic_retries_total",
            "gavel_auction_idempotency_total",
            "gavel_auction_stream_replay_events_total",
            "gavel_auction_outbox_events_total",
            "gavel_auction_inbox_total",
            "gavel_auction_consumer_events_total",
            "gavel_auction_cache_operations_total",
        ] {
            assert!(rendered.contains(name));
        }
        for forbidden in [
            "auction_id",
            "user_id",
            "idempotency_key",
            "payment_id",
            "amount",
            "maximum",
        ] {
            assert!(
                !rendered.contains(&format!("{forbidden}=\"")),
                "secret/high-cardinality label leaked: {forbidden}"
            );
        }
    }

    #[test]
    fn structured_command_logs_contain_only_bounded_fields() {
        let buffer = Buffer::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_writer(buffer.clone())
            .finish();
        with_default(subscriber, || {
            record_command(CommandOperation::PlaceBid, CommandOutcome::Rejected);
        });
        let output = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        assert!(output.contains("place_bid"));
        assert!(output.contains("rejected"));
        for forbidden in [
            "auction_id",
            "user_id",
            "idempotency_key",
            "payment_id",
            "amount",
            "maximum",
        ] {
            assert!(
                !output.contains(forbidden),
                "unsafe log field leaked: {forbidden}"
            );
        }
    }
}
