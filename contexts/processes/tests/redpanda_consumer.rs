use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use gavel_processes::{
    ingress::{
        Backoff, BrokerDelivery, BrokerRecord, ConsumedRecord, ConsumerLoop, DeliveryError,
        IngressDisposition, QuarantineCategory, QuarantineRecord, QuarantineReplayService,
        ReplayArchive, ReplayQuarantineCommand, RetryPolicy, SignalIngress,
    },
    workflows::ProcessError,
};

#[derive(Default)]
struct MockBrokerState {
    records: VecDeque<ConsumedRecord>,
    operations: Vec<&'static str>,
    quarantine: Vec<QuarantineRecord>,
    fail_quarantine: bool,
}

#[derive(Default)]
struct MockBroker(Mutex<MockBrokerState>);

#[async_trait]
impl BrokerDelivery for MockBroker {
    async fn receive(&self) -> Result<ConsumedRecord, DeliveryError> {
        self.0
            .lock()
            .unwrap()
            .records
            .front()
            .cloned()
            .ok_or(DeliveryError::Broker)
    }

    async fn commit(&self, _record: &ConsumedRecord) -> Result<(), DeliveryError> {
        let mut state = self.0.lock().unwrap();
        state.operations.push("commit");
        state.records.pop_front();
        Ok(())
    }

    async fn quarantine(&self, record: &QuarantineRecord) -> Result<(), DeliveryError> {
        let mut state = self.0.lock().unwrap();
        state.operations.push("quarantine");
        if state.fail_quarantine {
            return Err(DeliveryError::QuarantineUnavailable);
        }
        state.quarantine.push(record.clone());
        Ok(())
    }
}

struct ScriptedIngress {
    results: Mutex<VecDeque<Result<IngressDisposition, ProcessError>>>,
    operations: Arc<Mutex<Vec<&'static str>>>,
    calls: Mutex<u32>,
}

#[async_trait]
impl SignalIngress for ScriptedIngress {
    async fn ingest(&self, _record: BrokerRecord) -> Result<IngressDisposition, ProcessError> {
        *self.calls.lock().unwrap() += 1;
        self.operations.lock().unwrap().push("signal");
        self.results.lock().unwrap().pop_front().unwrap()
    }
}

#[derive(Default)]
struct ImmediateBackoff(Mutex<Vec<Duration>>);

#[async_trait]
impl Backoff for ImmediateBackoff {
    async fn wait(&self, duration: Duration) {
        self.0.lock().unwrap().push(duration);
    }
}

fn valid_record(offset: i64) -> ConsumedRecord {
    ConsumedRecord {
        topic: "gavel.payments.attempts.v1".to_owned(),
        partition: 2,
        offset,
        key: Some(b"attempt-1".to_vec()),
        payload: include_bytes!("fixtures/payment_succeeded_v1.json").to_vec(),
    }
}

fn retry() -> RetryPolicy {
    RetryPolicy {
        max_attempts: 3,
        initial_backoff: Duration::from_millis(10),
        max_backoff: Duration::from_millis(20),
    }
}

fn ingress(
    results: impl IntoIterator<Item = Result<IngressDisposition, ProcessError>>,
    operations: Arc<Mutex<Vec<&'static str>>>,
) -> Arc<ScriptedIngress> {
    Arc::new(ScriptedIngress {
        results: Mutex::new(results.into_iter().collect()),
        operations,
        calls: Mutex::new(0),
    })
}

#[tokio::test]
async fn commits_only_after_durable_signal_and_retries_transient_failure() {
    let broker = Arc::new(MockBroker::default());
    broker.0.lock().unwrap().records.push_back(valid_record(7));
    let operations = Arc::new(Mutex::new(Vec::new()));
    let ingress = ingress(
        [
            Err(ProcessError::DependencyUnavailable),
            Ok(IngressDisposition::Signalled {
                workflow_id: "settle-auction/auction-1".to_owned(),
            }),
        ],
        operations.clone(),
    );
    let backoff = ImmediateBackoff::default();
    let consumer = ConsumerLoop::with_backoff(broker.clone(), ingress, retry(), backoff).unwrap();

    assert!(matches!(
        consumer.run_once().await.unwrap(),
        IngressDisposition::Signalled { .. }
    ));
    assert_eq!(&*operations.lock().unwrap(), &["signal", "signal"]);
    assert_eq!(&broker.0.lock().unwrap().operations, &["commit"]);
}

#[tokio::test]
async fn uncommitted_record_is_redelivered_after_rebalance_or_restart() {
    let broker = Arc::new(MockBroker::default());
    broker.0.lock().unwrap().records.push_back(valid_record(9));
    let first = ingress(
        [
            Err(ProcessError::DependencyUnavailable),
            Err(ProcessError::DependencyUnavailable),
            Err(ProcessError::DependencyUnavailable),
        ],
        Arc::new(Mutex::new(Vec::new())),
    );
    let failed =
        ConsumerLoop::with_backoff(broker.clone(), first, retry(), ImmediateBackoff::default())
            .unwrap();
    assert_eq!(failed.run_once().await, Err(DeliveryError::RetryExhausted));
    assert_eq!(broker.0.lock().unwrap().records.front().unwrap().offset, 9);

    let restarted = ConsumerLoop::with_backoff(
        broker.clone(),
        ingress(
            [Ok(IngressDisposition::Duplicate)],
            Arc::new(Mutex::new(Vec::new())),
        ),
        retry(),
        ImmediateBackoff::default(),
    )
    .unwrap();
    assert_eq!(
        restarted.run_once().await.unwrap(),
        IngressDisposition::Duplicate
    );
    assert!(broker.0.lock().unwrap().records.is_empty());
}

#[tokio::test]
async fn invalid_payload_never_reaches_ingress_and_commits_after_sanitized_quarantine() {
    let secret = "https://provider.example/recovery?token=top-secret";
    let broker = Arc::new(MockBroker::default());
    broker.0.lock().unwrap().records.push_back(ConsumedRecord {
        topic: "gavel.payments.attempts.v1".to_owned(),
        partition: 0,
        offset: 3,
        key: Some(b"candidate-private".to_vec()),
        payload: include_bytes!("fixtures/payment_secret_injection.json").to_vec(),
    });
    let scripted = ingress([], Arc::new(Mutex::new(Vec::new())));
    let consumer = ConsumerLoop::with_backoff(
        broker.clone(),
        scripted.clone(),
        retry(),
        ImmediateBackoff::default(),
    )
    .unwrap();

    assert_eq!(
        consumer.run_once().await.unwrap(),
        IngressDisposition::Quarantined
    );
    assert_eq!(*scripted.calls.lock().unwrap(), 0);
    let state = broker.0.lock().unwrap();
    assert_eq!(state.operations, ["quarantine", "commit"]);
    let encoded = serde_json::to_string(&state.quarantine[0]).unwrap();
    assert!(!encoded.contains(secret));
    assert!(!encoded.contains("candidate-private"));
    assert!(!encoded.contains("12500"));
    assert!(!encoded.contains("recoveryUrl"));
    assert!(!encoded.contains("payload"));
    assert!(!format!("{:?}", state.quarantine[0]).contains("top-secret"));
}

#[tokio::test]
async fn quarantine_failure_never_commits_poison_record() {
    let broker = Arc::new(MockBroker::default());
    {
        let mut state = broker.0.lock().unwrap();
        state.fail_quarantine = true;
        state.records.push_back(ConsumedRecord {
            topic: "unknown.topic".to_owned(),
            partition: 0,
            offset: 1,
            key: None,
            payload: b"private".to_vec(),
        });
    }
    let consumer = ConsumerLoop::with_backoff(
        broker.clone(),
        ingress([], Arc::new(Mutex::new(Vec::new()))),
        retry(),
        ImmediateBackoff::default(),
    )
    .unwrap();

    assert_eq!(
        consumer.run_once().await,
        Err(DeliveryError::QuarantineUnavailable)
    );
    let state = broker.0.lock().unwrap();
    assert_eq!(state.operations, ["quarantine"]);
    assert_eq!(state.records.len(), 1);
}

#[tokio::test]
async fn unsupported_schema_uses_bounded_safe_category_without_owner_command() {
    let broker = Arc::new(MockBroker::default());
    broker.0.lock().unwrap().records.push_back(ConsumedRecord {
        topic: "gavel.payments.attempts.v1".to_owned(),
        partition: 1,
        offset: 12,
        key: Some(b"attempt-1".to_vec()),
        payload: include_bytes!("fixtures/payment_schema_v2.json").to_vec(),
    });
    let scripted = ingress([], Arc::new(Mutex::new(Vec::new())));
    let consumer = ConsumerLoop::with_backoff(
        broker.clone(),
        scripted.clone(),
        retry(),
        ImmediateBackoff::default(),
    )
    .unwrap();

    assert_eq!(
        consumer.run_once().await.unwrap(),
        IngressDisposition::Quarantined
    );
    assert_eq!(*scripted.calls.lock().unwrap(), 0);
    assert_eq!(
        broker.0.lock().unwrap().quarantine[0].failure.category,
        QuarantineCategory::UnsupportedSchemaVersion
    );
}

#[derive(Default)]
struct MemoryArchive {
    originals: Mutex<Vec<(String, ConsumedRecord)>>,
    replayed: Mutex<Vec<String>>,
}

#[async_trait]
impl ReplayArchive for MemoryArchive {
    async fn load_original(&self, record_id: &str) -> Result<ConsumedRecord, DeliveryError> {
        self.originals
            .lock()
            .unwrap()
            .iter()
            .find(|(id, _)| id == record_id)
            .map(|(_, record)| record.clone())
            .ok_or(DeliveryError::ReplayNotFound)
    }

    async fn mark_replayed(&self, record_id: &str) -> Result<(), DeliveryError> {
        self.replayed.lock().unwrap().push(record_id.to_owned());
        Ok(())
    }
}

#[tokio::test]
async fn explicit_replay_returns_original_through_same_ingress_boundary() {
    let archive = Arc::new(MemoryArchive::default());
    archive
        .originals
        .lock()
        .unwrap()
        .push(("quarantine-1".to_owned(), valid_record(20)));
    let scripted = ingress(
        [Ok(IngressDisposition::Signalled {
            workflow_id: "settle-auction/auction-1".to_owned(),
        })],
        Arc::new(Mutex::new(Vec::new())),
    );
    let replay = QuarantineReplayService::new(archive.clone(), scripted.clone());

    assert!(matches!(
        replay
            .replay(ReplayQuarantineCommand {
                record_id: "quarantine-1".to_owned(),
            })
            .await
            .unwrap(),
        IngressDisposition::Signalled { .. }
    ));
    assert_eq!(*scripted.calls.lock().unwrap(), 1);
    assert_eq!(&*archive.replayed.lock().unwrap(), &["quarantine-1"]);
}

#[tokio::test]
async fn replay_of_still_invalid_record_does_not_execute_owner_command() {
    let archive = Arc::new(MemoryArchive::default());
    archive.originals.lock().unwrap().push((
        "quarantine-invalid".to_owned(),
        ConsumedRecord {
            topic: "gavel.payments.attempts.v1".to_owned(),
            partition: 0,
            offset: 21,
            key: None,
            payload: include_bytes!("fixtures/payment_secret_injection.json").to_vec(),
        },
    ));
    let scripted = ingress(
        [Err(ProcessError::InvalidInput)],
        Arc::new(Mutex::new(Vec::new())),
    );
    let replay = QuarantineReplayService::new(archive.clone(), scripted.clone());

    assert_eq!(
        replay
            .replay(ReplayQuarantineCommand {
                record_id: "quarantine-invalid".to_owned(),
            })
            .await,
        Err(DeliveryError::ReplayRejected)
    );
    assert_eq!(*scripted.calls.lock().unwrap(), 0);
    assert!(archive.replayed.lock().unwrap().is_empty());
}
