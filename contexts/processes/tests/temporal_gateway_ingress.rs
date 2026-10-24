use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use gavel_auction::rpc::{
    self as auction_rpc, AuctionProcessEvent, AuctionPublicEvent, auction_process_event,
    auction_public_event,
};
use gavel_processes::ingress::{
    BrokerRecord, DecodedOwnerEvent, IngressDisposition, OwnerEvent, SignalReceipt,
    SignalWithStartClient, SignalWithStartRequest, TemporalGateway, TemporalGatewayError,
    TestBrokerAdapter, VersionedSignalIngress, WorkflowConflictPolicy,
};
use gavel_processes::workflows::AuctionId;
use prost::Message;

#[derive(Default)]
struct FakeTemporalState {
    workflows: HashSet<String>,
    requests: HashSet<String>,
    logical_signals: Vec<SignalWithStartRequest>,
}

#[derive(Clone, Default)]
struct FakeTemporalClient(Arc<Mutex<FakeTemporalState>>);

impl FakeTemporalClient {
    fn snapshot(&self) -> (usize, usize, Vec<SignalWithStartRequest>) {
        let state = self.0.lock().unwrap();
        (
            state.workflows.len(),
            state.logical_signals.len(),
            state.logical_signals.clone(),
        )
    }
}

#[async_trait]
impl SignalWithStartClient for FakeTemporalClient {
    async fn signal_with_start(
        &self,
        request: SignalWithStartRequest,
    ) -> Result<SignalReceipt, TemporalGatewayError> {
        let mut state = self.0.lock().unwrap();
        state.workflows.insert(request.workflow_id.clone());
        let accepted = state.requests.insert(request.request_id.clone());
        if accepted {
            state.logical_signals.push(request.clone());
        }
        Ok(SignalReceipt {
            workflow_id: request.workflow_id,
            signal_accepted: accepted,
        })
    }
}

fn opened_record() -> BrokerRecord {
    let event = AuctionPublicEvent {
        event_id: "auction-event-1".to_owned(),
        aggregate_id: "auction-1".to_owned(),
        aggregate_version: 1,
        schema_version: 1,
        occurred_at: Some(prost_types::Timestamp {
            seconds: 1_700_000_000,
            nanos: 0,
        }),
        correlation_id: "correlation-1".to_owned(),
        causation_id: "command-1".to_owned(),
        payload: Some(auction_public_event::Payload::AuctionOpened(
            auction_rpc::AuctionOpenedEvent {
                ends_at: Some(prost_types::Timestamp {
                    seconds: 1_700_000_600,
                    nanos: 0,
                }),
            },
        )),
    };
    BrokerRecord {
        source: gavel_processes::ingress::EventSource::AuctionPublic,
        payload: event.encode_to_vec(),
    }
}

fn restricted_candidate_record() -> BrokerRecord {
    BrokerRecord {
        source: gavel_processes::ingress::EventSource::AuctionRestricted,
        payload: AuctionProcessEvent {
            event_id: "restricted-candidate-1".to_owned(),
            aggregate_id: "auction-1".to_owned(),
            aggregate_version: 3,
            schema_version: 1,
            occurred_at: Some(prost_types::Timestamp {
                seconds: 1_700_000_100,
                nanos: 0,
            }),
            correlation_id: "correlation-1".to_owned(),
            causation_id: "auction-close-1".to_owned(),
            payload: Some(auction_process_event::Payload::SettlementCandidateSelected(
                auction_rpc::ProcessSettlementCandidateSelected {
                    candidate_id: "candidate-private-1".to_owned(),
                    settlement_round: 1,
                    amount: Some(auction_rpc::Money {
                        minor_units: 12_500,
                        currency_code: "USD".to_owned(),
                    }),
                },
            )),
        }
        .encode_to_vec(),
    }
}

fn ingress(client: FakeTemporalClient) -> Arc<VersionedSignalIngress<FakeTemporalClient>> {
    Arc::new(VersionedSignalIngress::new(TemporalGateway::new(
        client,
        "auction-lifecycle-v1",
        "settle-auction-v1",
    )))
}

#[tokio::test]
async fn concurrent_duplicate_creates_one_workflow_and_one_logical_signal() {
    let client = FakeTemporalClient::default();
    let ingress = ingress(client.clone());
    let adapter = TestBrokerAdapter::new(ingress);
    let record = opened_record();
    let (first, second) = tokio::join!(
        adapter.deliver(record.clone()),
        adapter.deliver(record.clone())
    );
    let dispositions = [first.unwrap(), second.unwrap()];
    assert_eq!(
        dispositions
            .iter()
            .filter(|value| matches!(value, IngressDisposition::Signalled { .. }))
            .count(),
        1
    );
    assert_eq!(
        dispositions
            .iter()
            .filter(|value| matches!(value, IngressDisposition::Duplicate))
            .count(),
        1
    );

    let (workflow_count, signal_count, requests) = client.snapshot();
    assert_eq!(workflow_count, 1);
    assert_eq!(signal_count, 1);
    let request = &requests[0];
    assert_eq!(request.workflow_id, "auction-lifecycle/auction-1");
    assert_eq!(request.request_id, "owner-event/auction-event-1");
    assert_eq!(request.task_queue, "auction-lifecycle-v1");
    assert_eq!(request.conflict_policy, WorkflowConflictPolicy::UseExisting);
    assert_eq!(
        temporalio_client::WorkflowIdConflictPolicy::from(request.conflict_policy),
        temporalio_client::WorkflowIdConflictPolicy::UseExisting
    );
}

#[tokio::test]
async fn test_broker_fixture_reaches_settlement_workflow_state() {
    let client = FakeTemporalClient::default();
    let adapter = TestBrokerAdapter::new(ingress(client.clone()));
    let payment = include_bytes!("../../payments/tests/fixtures/payment_succeeded_v1.json");
    let disposition = adapter
        .deliver(BrokerRecord {
            source: gavel_processes::ingress::EventSource::PaymentsAttempt,
            payload: payment.to_vec(),
        })
        .await
        .unwrap();
    assert!(matches!(
        disposition,
        IngressDisposition::SignalledAfterGap {
            ref workflow_id,
            expected_version: 1,
            received_version: 4,
        } if workflow_id == "settle-auction/auction-1"
    ));

    let (_, signal_count, requests) = client.snapshot();
    assert_eq!(signal_count, 1);
    assert_eq!(requests[0].workflow_id, "settle-auction/auction-1");
    assert_eq!(
        requests[0].request_id,
        "owner-event/payments-event-success-1"
    );
}

#[tokio::test]
async fn restricted_candidate_routes_only_to_settlement_workflow() {
    let client = FakeTemporalClient::default();
    let adapter = TestBrokerAdapter::new(ingress(client.clone()));

    assert!(matches!(
        adapter.deliver(restricted_candidate_record()).await.unwrap(),
        IngressDisposition::SignalledAfterGap {
            ref workflow_id,
            expected_version: 1,
            received_version: 3,
        } if workflow_id == "settle-auction/auction-1"
    ));
    let (workflow_count, signal_count, requests) = client.snapshot();
    assert_eq!((workflow_count, signal_count), (1, 1));
    assert_eq!(requests[0].workflow_type, "SettleAuction");
    assert_eq!(requests[0].request_id, "owner-event/restricted-candidate-1");
}

#[tokio::test]
async fn terminal_owner_event_is_fanned_out_to_both_processes_once() {
    let client = FakeTemporalClient::default();
    let gateway = TemporalGateway::new(client.clone(), "auction-lifecycle-v1", "settle-auction-v1");
    let event = DecodedOwnerEvent {
        event_id: "auction-terminal-1".to_owned(),
        aggregate_version: 7,
        occurred_at_millis: 1_700_000_700_000,
        correlation_id: "correlation-1".to_owned(),
        causation_id: Some("command-7".to_owned()),
        event: OwnerEvent::AuctionUnsold {
            auction_id: AuctionId::parse("auction-1").unwrap(),
        },
    };

    let receipt = gateway.signal(event.clone()).await.unwrap();
    assert_eq!(receipt.workflow_id, "settle-auction/auction-1");
    gateway.signal(event).await.unwrap();

    let (workflow_count, signal_count, requests) = client.snapshot();
    assert_eq!(workflow_count, 2);
    assert_eq!(signal_count, 2);
    assert_eq!(
        requests
            .iter()
            .map(|request| (
                request.workflow_id.as_str(),
                request.request_id.as_str(),
                request.workflow_type,
            ))
            .collect::<HashSet<_>>(),
        HashSet::from([
            (
                "auction-lifecycle/auction-1",
                "owner-event/auction-terminal-1/lifecycle",
                "AuctionLifecycle",
            ),
            (
                "settle-auction/auction-1",
                "owner-event/auction-terminal-1/settlement",
                "SettleAuction",
            ),
        ])
    );
}

#[test]
fn no_production_manual_event_backdoor_is_exposed() {
    let sources = [
        include_str!("../src/main.rs"),
        include_str!("../src/lib.rs"),
        include_str!("../src/ingress/broker.rs"),
    ]
    .join("\n");
    for forbidden in ["/signals", "/ingress", "/events/push", "manual_signal"] {
        assert!(!sources.contains(forbidden), "manual backdoor: {forbidden}");
    }
}
