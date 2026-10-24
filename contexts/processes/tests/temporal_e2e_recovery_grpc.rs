use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use gavel_auction::rpc as auction;
use gavel_notify::rpc as notify;
use gavel_payments_prost::gavel::payments::v1 as payments;
use gavel_processes::{
    activities::SettlementTemporalActivities,
    adapters::owner_rpc::OwnerRpcActivities,
    workflows::{
        AuctionId, CandidateId, CorrelationContext, Money, PaymentAttemptId, SettleAuctionInput,
        SettleAuctionRun, SettleAuctionWorkflow, SettlementCandidate, SettlementOwnerSignal,
        SettlementPhase, SettlementRound, SettlementSignal, SettlementTerminal,
    },
};
use temporalio_client::{
    Client, ClientOptions, ConnectionOptions, WorkflowFetchHistoryOptions,
    WorkflowGetResultOptions, WorkflowHandle, WorkflowHistory, WorkflowQueryOptions,
    WorkflowSignalOptions, WorkflowStartOptions,
};
use temporalio_common::protos::temporal::api::testservice::v1::SleepRequest;
use temporalio_sdk::{
    Runtime, Worker, WorkerOptions,
    workflow_replayer::{WorkflowReplayer, WorkflowReplayerOptions},
};
use temporalio_sdk_core::ephemeral_server::{EphemeralExe, EphemeralExeVersion, TestServerConfig};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::sync::CancellationToken;
use tonic_temporal::IntoRequest;
use url::Url;

use auction::auction_service_server::{AuctionService, AuctionServiceServer};
use gavel_payments_tonic::gavel::payments::v1::tonic::payments_service_server::{
    PaymentsService, PaymentsServiceServer,
};
use notify::notify_service_server::{NotifyService, NotifyServiceServer};

const QUEUE: &str = "gavel-processes-recovery-grpc-e2e-v1";
const RECOVERY_URL: &str = "https://payments.example/recovery/private-secret-token";

#[derive(Clone, Debug)]
enum PaymentState {
    Processing,
    RecoveryRequired,
    Succeeded { payment_id: String },
    Expired,
}

#[derive(Clone, Debug)]
struct PaymentRecord {
    auction_id: String,
    candidate_id: String,
    round: u32,
    attempt_id: String,
    version: u64,
    state: PaymentState,
    checked_at_token: String,
}

#[derive(Default)]
struct OwnerData {
    payments: Mutex<HashMap<String, PaymentRecord>>,
    attempt_requests: Mutex<Vec<payments::AttemptPaymentRequest>>,
    read_requests: Mutex<Vec<payments::GetSettlementAttemptRequest>>,
    expire_requests: Mutex<Vec<payments::ExpirePaymentRequest>>,
    forfeit_requests: Mutex<Vec<auction::ForfeitSettlementCandidateRequest>>,
    confirm_requests: Mutex<Vec<auction::ConfirmWinnerRequest>>,
    notification_requests: Mutex<Vec<notify::SendNotificationRequest>>,
}

#[derive(Clone, Default)]
struct OwnerState(Arc<OwnerData>);

impl OwnerState {
    fn register_round(&self, auction_id: &str, round: u32) {
        let attempt_id = format!("attempt-{auction_id}-{round}");
        self.0.payments.lock().unwrap().insert(
            attempt_id.clone(),
            PaymentRecord {
                auction_id: auction_id.to_owned(),
                candidate_id: format!("candidate-{auction_id}-{round}"),
                round,
                attempt_id,
                version: 1,
                state: PaymentState::Processing,
                checked_at_token: format!("checked-{auction_id}-{round}-1"),
            },
        );
    }

    fn transition_recovery(&self, auction_id: &str, round: u32) -> (PaymentAttemptId, u64) {
        self.transition(auction_id, round, PaymentState::RecoveryRequired)
    }

    fn transition_success(&self, auction_id: &str, round: u32) -> (PaymentAttemptId, u64) {
        self.transition(
            auction_id,
            round,
            PaymentState::Succeeded {
                payment_id: format!("payment-{auction_id}-{round}"),
            },
        )
    }

    fn transition(
        &self,
        auction_id: &str,
        round: u32,
        state: PaymentState,
    ) -> (PaymentAttemptId, u64) {
        let mut records = self.0.payments.lock().unwrap();
        let record = records
            .values_mut()
            .find(|record| record.auction_id == auction_id && record.round == round)
            .unwrap();
        record.version += 1;
        record.checked_at_token = format!("checked-{auction_id}-{round}-{}", record.version);
        record.state = state;
        (
            PaymentAttemptId::parse(record.attempt_id.clone()).unwrap(),
            record.version,
        )
    }

    fn record_by_attempt(&self, attempt_id: &str) -> PaymentRecord {
        self.0
            .payments
            .lock()
            .unwrap()
            .get(attempt_id)
            .unwrap()
            .clone()
    }

    fn count_for<T>(values: &Mutex<Vec<T>>, predicate: impl Fn(&T) -> bool) -> usize {
        values
            .lock()
            .unwrap()
            .iter()
            .filter(|value| predicate(value))
            .count()
    }
}

#[derive(Clone)]
struct AuctionOwner {
    state: OwnerState,
}

#[tonic::async_trait]
impl AuctionService for AuctionOwner {
    async fn close_auction(
        &self,
        _: tonic::Request<auction::CloseAuctionRequest>,
    ) -> Result<tonic::Response<auction::CloseAuctionResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented(
            "not used by settlement recovery",
        ))
    }

    async fn cancel_auction(
        &self,
        _: tonic::Request<auction::CancelAuctionRequest>,
    ) -> Result<tonic::Response<auction::CancelAuctionResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented(
            "not used by settlement recovery",
        ))
    }

    async fn forfeit_settlement_candidate(
        &self,
        request: tonic::Request<auction::ForfeitSettlementCandidateRequest>,
    ) -> Result<tonic::Response<auction::ForfeitSettlementCandidateResponse>, tonic::Status> {
        assert_eq!(
            request.metadata().get("x-gavel-principal-type").unwrap(),
            "service_account"
        );
        assert_eq!(
            request.metadata().get("x-gavel-principal-id").unwrap(),
            "processes-recovery-e2e"
        );
        assert!(request.metadata().get("grpc-timeout").is_some());
        let request = request.into_inner();
        self.state.0.forfeit_requests.lock().unwrap().push(request);
        Ok(tonic::Response::new(
            auction::ForfeitSettlementCandidateResponse {
                outcome: Some(
                    auction::forfeit_settlement_candidate_response::Outcome::Advanced(
                        auction::SettlementAdvanced {
                            status: auction::AuctionStatus::Settling as i32,
                            settlement_round: 2,
                            version: 2,
                        },
                    ),
                ),
            },
        ))
    }

    async fn confirm_winner(
        &self,
        request: tonic::Request<auction::ConfirmWinnerRequest>,
    ) -> Result<tonic::Response<auction::ConfirmWinnerResponse>, tonic::Status> {
        assert!(request.metadata().get("grpc-timeout").is_some());
        let request = request.into_inner();
        self.state.0.confirm_requests.lock().unwrap().push(request);
        Ok(tonic::Response::new(auction::ConfirmWinnerResponse {
            outcome: Some(auction::confirm_winner_response::Outcome::Confirmed(
                auction::WinnerConfirmed { version: 3 },
            )),
        }))
    }
}

#[derive(Clone)]
struct PaymentsOwner {
    state: OwnerState,
}

#[tonic_bsr::async_trait]
impl PaymentsService for PaymentsOwner {
    async fn attempt_payment(
        &self,
        request: tonic_bsr::Request<payments::AttemptPaymentRequest>,
    ) -> Result<tonic_bsr::Response<payments::AttemptPaymentResponse>, tonic_bsr::Status> {
        assert!(request.metadata().get("grpc-timeout").is_some());
        let request = request.into_inner();
        let record = self
            .state
            .0
            .payments
            .lock()
            .unwrap()
            .values()
            .find(|record| {
                record.auction_id == request.auction_id && record.round == request.settlement_round
            })
            .unwrap()
            .clone();
        assert_eq!(record.candidate_id, request.candidate_id);
        self.state.0.attempt_requests.lock().unwrap().push(request);
        Ok(tonic_bsr::Response::new(payments::AttemptPaymentResponse {
            outcome: Some(payments::attempt_payment_response::Outcome::Result(
                payments::PaymentAttemptResult {
                    payment_attempt_id: record.attempt_id,
                    status: payments::PaymentAttemptStatus::Processing as i32,
                    recovery_url: String::new(),
                    version: record.version,
                    checked_at_token: record.checked_at_token,
                },
            )),
        }))
    }

    async fn get_settlement_attempt(
        &self,
        request: tonic_bsr::Request<payments::GetSettlementAttemptRequest>,
    ) -> Result<tonic_bsr::Response<payments::GetSettlementAttemptResponse>, tonic_bsr::Status>
    {
        assert!(request.metadata().get("grpc-timeout").is_some());
        let request = request.into_inner();
        let record = self.state.record_by_attempt(&request.payment_attempt_id);
        assert_eq!(record.auction_id, request.expected_auction_id);
        assert_eq!(record.round, request.expected_settlement_round);
        self.state.0.read_requests.lock().unwrap().push(request);
        let (status, state_data) = match record.state {
            PaymentState::Processing => (payments::PaymentAttemptStatus::Processing, None),
            PaymentState::RecoveryRequired => (
                payments::PaymentAttemptStatus::RecoveryRequired,
                Some(
                    payments::settlement_attempt_result::StateData::RecoveryRequired(
                        payments::SettlementRecoveryData {
                            recovery_url: RECOVERY_URL.to_owned(),
                        },
                    ),
                ),
            ),
            PaymentState::Succeeded { payment_id } => (
                payments::PaymentAttemptStatus::Succeeded,
                Some(payments::settlement_attempt_result::StateData::Succeeded(
                    payments::SettlementSuccessData { payment_id },
                )),
            ),
            PaymentState::Expired => (payments::PaymentAttemptStatus::Expired, None),
        };
        Ok(tonic_bsr::Response::new(
            payments::GetSettlementAttemptResponse {
                outcome: Some(payments::get_settlement_attempt_response::Outcome::Result(
                    payments::SettlementAttemptResult {
                        payment_attempt_id: record.attempt_id,
                        auction_id: record.auction_id,
                        settlement_round: record.round,
                        candidate_id: record.candidate_id,
                        status: status as i32,
                        version: record.version,
                        checked_at_token: record.checked_at_token,
                        state_data,
                    },
                )),
            },
        ))
    }

    async fn expire_payment(
        &self,
        request: tonic_bsr::Request<payments::ExpirePaymentRequest>,
    ) -> Result<tonic_bsr::Response<payments::ExpirePaymentResponse>, tonic_bsr::Status> {
        assert!(request.metadata().get("grpc-timeout").is_some());
        let request = request.into_inner();
        self.state
            .0
            .expire_requests
            .lock()
            .unwrap()
            .push(request.clone());
        let mut records = self.state.0.payments.lock().unwrap();
        let record = records.get_mut(&request.payment_attempt_id).unwrap();
        record.version += 1;
        record.checked_at_token = format!(
            "checked-{}-{}-{}",
            record.auction_id, record.round, record.version
        );
        if record.auction_id.contains("already-succeeded-race") {
            record.state = PaymentState::Succeeded {
                payment_id: format!("payment-{}-{}", record.auction_id, record.round),
            };
            return Ok(tonic_bsr::Response::new(payments::ExpirePaymentResponse {
                outcome: Some(payments::expire_payment_response::Outcome::Rejection(
                    payments::DomainRejection {
                        reason: "already_succeeded".to_owned(),
                        safe_message: "payment already succeeded".to_owned(),
                        code: payments::PaymentRejectionCode::AlreadySucceeded as i32,
                    },
                )),
            }));
        }
        record.state = PaymentState::Expired;
        Ok(tonic_bsr::Response::new(payments::ExpirePaymentResponse {
            outcome: Some(payments::expire_payment_response::Outcome::Expired(
                payments::PaymentExpired {
                    payment_attempt_id: record.attempt_id.clone(),
                    version: record.version,
                    checked_at_token: record.checked_at_token.clone(),
                },
            )),
        }))
    }
}

#[derive(Clone)]
struct NotifyOwner {
    state: OwnerState,
}

#[tonic::async_trait]
impl NotifyService for NotifyOwner {
    async fn send_notification(
        &self,
        request: tonic::Request<notify::SendNotificationRequest>,
    ) -> Result<tonic::Response<notify::SendNotificationResponse>, tonic::Status> {
        assert!(request.metadata().get("grpc-timeout").is_some());
        self.state
            .0
            .notification_requests
            .lock()
            .unwrap()
            .push(request.into_inner());
        Ok(tonic::Response::new(notify::SendNotificationResponse {
            outcome: Some(notify::send_notification_response::Outcome::Accepted(
                notify::NotificationAccepted {
                    status: notify::NotificationStatus::Pending as i32,
                    notification_id: "notification-recovery".to_owned(),
                    template_version: 1,
                    attempt_count: 0,
                    version: 1,
                },
            )),
        }))
    }
}

struct OwnerServers {
    cancellation: CancellationToken,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl OwnerServers {
    async fn shutdown(self) {
        self.cancellation.cancel();
        for task in self.tasks {
            task.await.unwrap();
        }
    }
}

async fn start_owner_boundaries() -> (OwnerRpcActivities, OwnerState, OwnerServers) {
    let state = OwnerState::default();
    let cancellation = CancellationToken::new();
    let owner_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let owner_address = owner_listener.local_addr().unwrap();
    let owner_cancel = cancellation.clone();
    let auction_owner = AuctionOwner {
        state: state.clone(),
    };
    let notify_owner = NotifyOwner {
        state: state.clone(),
    };
    let owner_task = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(AuctionServiceServer::new(auction_owner))
            .add_service(NotifyServiceServer::new(notify_owner))
            .serve_with_incoming_shutdown(
                TcpListenerStream::new(owner_listener),
                owner_cancel.cancelled_owned(),
            )
            .await
            .unwrap();
    });

    let payments_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let payments_address = payments_listener.local_addr().unwrap();
    let payments_cancel = cancellation.clone();
    let payments_owner = PaymentsOwner {
        state: state.clone(),
    };
    let payments_task = tokio::spawn(async move {
        tonic_bsr::transport::Server::builder()
            .add_service(PaymentsServiceServer::new(payments_owner))
            .serve_with_incoming_shutdown(
                TcpListenerStream::new(payments_listener),
                payments_cancel.cancelled_owned(),
            )
            .await
            .unwrap();
    });

    let auction_client = auction::auction_service_client::AuctionServiceClient::connect(format!(
        "http://{owner_address}"
    ))
    .await
    .unwrap();
    let notify_client = notify::notify_service_client::NotifyServiceClient::connect(format!(
        "http://{owner_address}"
    ))
    .await
    .unwrap();
    let payments_client = gavel_payments_tonic::gavel::payments::v1::tonic::payments_service_client::PaymentsServiceClient::connect(format!(
        "http://{payments_address}"
    ))
    .await
    .unwrap();
    (
        OwnerRpcActivities::new(
            auction_client,
            payments_client,
            notify_client,
            "processes-recovery-e2e",
        ),
        state,
        OwnerServers {
            cancellation,
            tasks: vec![owner_task, payments_task],
        },
    )
}

type SettlementHandle = WorkflowHandle<Client, SettleAuctionRun>;

fn worker_options(owners: Arc<OwnerRpcActivities>) -> WorkerOptions {
    WorkerOptions::new(QUEUE)
        .max_cached_workflows(0)
        .register_workflow::<SettleAuctionWorkflow>()
        .unwrap()
        .register_activities(SettlementTemporalActivities::new(
            owners.clone(),
            owners.clone(),
            owners,
        ))
        .build()
}

async fn start_settlement(
    client: &Client,
    auction_id: &str,
    round: u32,
    auction_version: u64,
) -> SettlementHandle {
    let handle = client
        .start_workflow(
            SettleAuctionWorkflow::run,
            SettleAuctionInput {
                auction_id: AuctionId::parse(auction_id).unwrap(),
                correlation: CorrelationContext {
                    correlation_id: format!("correlation-{auction_id}"),
                    causation_id: format!("candidate-{auction_id}-{round}"),
                },
            },
            WorkflowStartOptions::new(QUEUE, format!("settle-auction/{auction_id}")).build(),
        )
        .await
        .unwrap();
    signal_candidate(&handle, auction_id, round, auction_version).await;
    handle
}

async fn signal_candidate(
    handle: &SettlementHandle,
    auction_id: &str,
    round: u32,
    auction_version: u64,
) {
    handle
        .signal(
            SettleAuctionWorkflow::owner_event,
            SettlementSignal {
                event_id: format!("candidate-event-{auction_id}-{round}"),
                fingerprint: format!("candidate-fingerprint-{auction_id}-{round}"),
                owner_version: auction_version,
                kind: SettlementOwnerSignal::CandidateSelected {
                    candidate: SettlementCandidate {
                        auction_id: AuctionId::parse(auction_id).unwrap(),
                        candidate_id: CandidateId::parse(format!("candidate-{auction_id}-{round}"))
                            .unwrap(),
                        round: SettlementRound::new(round).unwrap(),
                        amount: Money::new(500, "USD").unwrap(),
                        auction_version,
                    },
                },
            },
            WorkflowSignalOptions::default(),
        )
        .await
        .unwrap();
}

async fn signal_payment(
    handle: &SettlementHandle,
    auction_id: &str,
    attempt_id: PaymentAttemptId,
    round: u32,
    version: u64,
) {
    handle
        .signal(
            SettleAuctionWorkflow::owner_event,
            SettlementSignal {
                event_id: format!("payment-event-{auction_id}-{round}-{version}"),
                fingerprint: format!("payment-fingerprint-{auction_id}-{round}-{version}"),
                owner_version: version,
                kind: SettlementOwnerSignal::PaymentChanged {
                    attempt_id,
                    round: SettlementRound::new(round).unwrap(),
                    attempt_version: version,
                },
            },
            WorkflowSignalOptions::default(),
        )
        .await
        .unwrap();
}

async fn wait_for_phase(handle: &SettlementHandle, expected: SettlementPhase) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let query = tokio::time::timeout(
                Duration::from_secs(1),
                handle.query(
                    SettleAuctionWorkflow::settlement_state,
                    (),
                    WorkflowQueryOptions::default(),
                ),
            )
            .await;
            if let Ok(Ok(state)) = query
                && state.phase == expected
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("workflow did not reach {expected:?}"));
}

async fn wait_for_count(count: impl Fn() -> usize, expected: usize) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while count() != expected {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
}

async fn skip_recovery_window(client: &Client) {
    let mut test_service = client.connection().test_service();
    test_service
        .unlock_time_skipping_with_sleep(
            SleepRequest {
                duration: Some(
                    prost_wkt_types::Duration::try_from(Duration::from_secs(2 * 60 * 60 + 1))
                        .unwrap(),
                ),
            }
            .into_request(),
        )
        .await
        .unwrap();
}

async fn history(handle: &SettlementHandle) -> Vec<u8> {
    handle
        .fetch_history(WorkflowFetchHistoryOptions::default())
        .to_json()
        .await
        .unwrap()
}

fn assert_history_is_private(history: &[u8]) {
    let text = String::from_utf8(history.to_vec())
        .unwrap()
        .to_ascii_lowercase();
    for forbidden in [
        RECOVERY_URL,
        "private-secret-token",
        "recovery_url",
        "provider_reference",
        "max_bid",
    ] {
        assert!(!text.contains(forbidden));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "runs SDK-pinned Temporal test server and generated in-process owner gRPC services"]
async fn recovery_grpc_expiry_next_round_restart_replay_and_already_succeeded_race_converge() {
    let mut temporal_server = TestServerConfig::builder()
        .exe(EphemeralExe::CachedDownload {
            version: EphemeralExeVersion::SDKDefault {
                sdk_name: "sdk-rust".to_owned(),
                sdk_version: "1.1.0".to_owned(),
            },
            dest_dir: Some("target/temporal-test-server-cache".to_owned()),
            ttl: None,
        })
        .build()
        .start_server()
        .await
        .unwrap();
    let client = Client::connect(
        ConnectionOptions::new(Url::parse(&format!("http://{}", temporal_server.target)).unwrap())
            .identity("gavel-processes-recovery-grpc-test".to_owned())
            .client_name("gavel-processes".to_owned())
            .client_version(env!("CARGO_PKG_VERSION").to_owned())
            .build(),
        ClientOptions::new("default").build(),
    )
    .await
    .unwrap();
    let runtime = Runtime::from_current_tokio(Default::default()).unwrap();
    let (owners, state, owner_servers) = start_owner_boundaries().await;
    let owners = Arc::new(owners);
    let mut first_worker =
        Worker::new(&runtime, client.clone(), worker_options(owners.clone())).unwrap();
    let first_shutdown = first_worker.shutdown_handle();

    let main_auction = format!("recovery-grpc-main-{}", std::process::id());
    state.register_round(&main_auction, 1);
    state.register_round(&main_auction, 2);
    let first_phase = async {
        let main = start_settlement(&client, &main_auction, 1, 1).await;
        wait_for_phase(&main, SettlementPhase::WaitingForPayment).await;
        let (attempt_id, version) = state.transition_recovery(&main_auction, 1);
        signal_payment(&main, &main_auction, attempt_id, 1, version).await;
        wait_for_phase(&main, SettlementPhase::RecoveryRequired).await;
        wait_for_count(
            || {
                OwnerState::count_for(&state.0.notification_requests, |request| {
                    request.template == notify::NotificationTemplate::PaymentRecoveryRequired as i32
                })
            },
            1,
        )
        .await;
        let recovery_request = state
            .0
            .notification_requests
            .lock()
            .unwrap()
            .iter()
            .find(|request| {
                request.template == notify::NotificationTemplate::PaymentRecoveryRequired as i32
            })
            .unwrap()
            .clone();
        assert_eq!(
            recovery_request
                .data
                .get("recovery_url")
                .map(String::as_str),
            Some(RECOVERY_URL)
        );
        first_shutdown();
        main
    };
    let (first_result, main) = tokio::join!(first_worker.run(), first_phase);
    first_result.unwrap();
    drop(first_worker);

    let mut second_worker =
        Worker::new(&runtime, client.clone(), worker_options(owners.clone())).unwrap();
    let second_shutdown = second_worker.shutdown_handle();
    let second_phase = async {
        skip_recovery_window(&client).await;
        wait_for_phase(&main, SettlementPhase::AwaitingNextRound).await;
        assert_eq!(
            OwnerState::count_for(&state.0.expire_requests, |request| {
                request.payment_attempt_id == format!("attempt-{main_auction}-1")
            }),
            1
        );
        assert_eq!(
            OwnerState::count_for(&state.0.forfeit_requests, |request| {
                request.auction_id == main_auction && request.settlement_round == 1
            }),
            1
        );

        signal_candidate(&main, &main_auction, 2, 2).await;
        wait_for_phase(&main, SettlementPhase::WaitingForPayment).await;
        let (attempt_id, version) = state.transition_success(&main_auction, 2);
        signal_payment(&main, &main_auction, attempt_id, 2, version).await;
        let main_result = main
            .get_result(WorkflowGetResultOptions::default())
            .await
            .unwrap();
        assert_eq!(main_result.terminal, SettlementTerminal::Sold);
        assert_eq!(
            OwnerState::count_for(&state.0.confirm_requests, |request| {
                request.auction_id == main_auction
                    && request.candidate_id == format!("candidate-{main_auction}-2")
            }),
            1
        );
        assert_eq!(
            OwnerState::count_for(&state.0.notification_requests, |request| {
                request.recipient_id == format!("candidate-{main_auction}-1")
                    && request.template
                        == notify::NotificationTemplate::PaymentRecoveryRequired as i32
            }),
            1
        );
        assert_eq!(
            OwnerState::count_for(&state.0.notification_requests, |request| {
                request.recipient_id == format!("candidate-{main_auction}-1")
                    && request.template
                        == notify::NotificationTemplate::SettlementCandidateFailed as i32
                    && request.data.get("auction_id") == Some(&main_auction)
            }),
            1
        );
        let main_history = history(&main).await;
        assert_history_is_private(&main_history);

        let race_auction = format!(
            "recovery-grpc-already-succeeded-race-{}",
            std::process::id()
        );
        state.register_round(&race_auction, 1);
        let race = start_settlement(&client, &race_auction, 1, 1).await;
        wait_for_phase(&race, SettlementPhase::WaitingForPayment).await;
        let (attempt_id, version) = state.transition_recovery(&race_auction, 1);
        signal_payment(&race, &race_auction, attempt_id, 1, version).await;
        wait_for_phase(&race, SettlementPhase::RecoveryRequired).await;
        wait_for_count(
            || {
                OwnerState::count_for(&state.0.notification_requests, |request| {
                    request.recipient_id == format!("candidate-{race_auction}-1")
                        && request.template
                            == notify::NotificationTemplate::PaymentRecoveryRequired as i32
                })
            },
            1,
        )
        .await;
        skip_recovery_window(&client).await;
        let race_result = race
            .get_result(WorkflowGetResultOptions::default())
            .await
            .unwrap();
        assert_eq!(race_result.terminal, SettlementTerminal::Sold);
        assert_eq!(
            OwnerState::count_for(&state.0.expire_requests, |request| {
                request.payment_attempt_id == format!("attempt-{race_auction}-1")
            }),
            1
        );
        assert_eq!(
            OwnerState::count_for(&state.0.forfeit_requests, |request| {
                request.auction_id == race_auction
            }),
            0
        );
        assert_eq!(
            OwnerState::count_for(&state.0.confirm_requests, |request| {
                request.auction_id == race_auction
            }),
            1
        );
        let race_history = history(&race).await;
        assert_history_is_private(&race_history);
        second_shutdown();
        (main_history, race_history)
    };
    let (second_result, (main_history, race_history)) =
        tokio::join!(second_worker.run(), second_phase);
    second_result.unwrap();
    drop(second_worker);
    drop(runtime);
    drop(client);
    owner_servers.shutdown().await;
    temporal_server.shutdown().await.unwrap();

    for recorded_history in [main_history, race_history] {
        let replay_options = WorkflowReplayerOptions::new()
            .register_workflow::<SettleAuctionWorkflow>()
            .unwrap()
            .build();
        WorkflowReplayer::new(replay_options)
            .unwrap()
            .replay_workflow(WorkflowHistory::from_json(&recorded_history).unwrap())
            .await
            .unwrap();
    }
}
