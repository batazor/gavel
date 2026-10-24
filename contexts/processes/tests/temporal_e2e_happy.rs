use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use gavel_auction::rpc as auction;
use gavel_notify::rpc as notify;
use gavel_payments_prost::gavel::payments::v1 as payments;
use gavel_processes::{
    activities::{OwnerTemporalActivities, SettlementTemporalActivities},
    adapters::owner_rpc::OwnerRpcActivities,
    workflows::{
        AuctionId, AuctionLifecycleInput, AuctionLifecycleWorkflow, CandidateId,
        CorrelationContext, LifecycleEventKind, LifecycleSignal, LifecycleTerminal, Money,
        PaymentAttemptId, ProcessId, SettleAuctionInput, SettleAuctionWorkflow,
        SettlementCandidate, SettlementOwnerSignal, SettlementRound, SettlementSignal,
        SettlementTerminal,
    },
};
use temporalio_client::{
    WorkflowCountOptions, WorkflowGetResultOptions, WorkflowSignalOptions, WorkflowStartOptions,
};
use temporalio_sdk::{
    Runtime, Worker, WorkerOptions,
    testing::{
        EphemeralExe, EphemeralExeVersion, LocalWorkflowEnvironmentOptions, WorkflowEnvironment,
    },
};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::sync::CancellationToken;

use auction::auction_service_server::{AuctionService, AuctionServiceServer};
use gavel_payments_tonic::gavel::payments::v1::tonic::payments_service_server::{
    PaymentsService, PaymentsServiceServer,
};
use notify::notify_service_server::{NotifyService, NotifyServiceServer};

#[derive(Default)]
struct OwnerEffects {
    close_requests: Mutex<Vec<auction::CloseAuctionRequest>>,
    confirm_requests: Mutex<Vec<auction::ConfirmWinnerRequest>>,
    forfeit_calls: AtomicUsize,
    payment_attempt_requests: Mutex<Vec<payments::AttemptPaymentRequest>>,
    payment_read_requests: Mutex<Vec<payments::GetSettlementAttemptRequest>>,
    expire_calls: AtomicUsize,
    notification_requests: Mutex<Vec<notify::SendNotificationRequest>>,
}

#[derive(Clone)]
struct AuctionOwner {
    effects: Arc<OwnerEffects>,
}

#[tonic::async_trait]
impl AuctionService for AuctionOwner {
    async fn close_auction(
        &self,
        request: tonic::Request<auction::CloseAuctionRequest>,
    ) -> Result<tonic::Response<auction::CloseAuctionResponse>, tonic::Status> {
        assert_eq!(
            request.metadata().get("x-gavel-principal-type").unwrap(),
            "service_account"
        );
        assert_eq!(
            request.metadata().get("x-gavel-principal-id").unwrap(),
            "processes-e2e"
        );
        assert!(request.metadata().get("grpc-timeout").is_some());
        let request = request.into_inner();
        self.effects.close_requests.lock().unwrap().push(request);
        Ok(tonic::Response::new(auction::CloseAuctionResponse {
            outcome: Some(auction::close_auction_response::Outcome::Closed(
                auction::AuctionClosed {
                    status: auction::AuctionStatus::Settling as i32,
                    closing_price: Some(auction::Money {
                        minor_units: 500,
                        currency_code: "USD".to_owned(),
                    }),
                    version: 3,
                },
            )),
        }))
    }

    async fn cancel_auction(
        &self,
        _: tonic::Request<auction::CancelAuctionRequest>,
    ) -> Result<tonic::Response<auction::CancelAuctionResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("not used by happy path"))
    }

    async fn forfeit_settlement_candidate(
        &self,
        _: tonic::Request<auction::ForfeitSettlementCandidateRequest>,
    ) -> Result<tonic::Response<auction::ForfeitSettlementCandidateResponse>, tonic::Status> {
        self.effects.forfeit_calls.fetch_add(1, Ordering::SeqCst);
        Err(tonic::Status::failed_precondition(
            "forfeit is not valid on happy path",
        ))
    }

    async fn confirm_winner(
        &self,
        request: tonic::Request<auction::ConfirmWinnerRequest>,
    ) -> Result<tonic::Response<auction::ConfirmWinnerResponse>, tonic::Status> {
        assert!(request.metadata().get("grpc-timeout").is_some());
        let request = request.into_inner();
        self.effects.confirm_requests.lock().unwrap().push(request);
        Ok(tonic::Response::new(auction::ConfirmWinnerResponse {
            outcome: Some(auction::confirm_winner_response::Outcome::Confirmed(
                auction::WinnerConfirmed { version: 4 },
            )),
        }))
    }
}

#[derive(Clone)]
struct PaymentsOwner {
    effects: Arc<OwnerEffects>,
}

#[tonic_bsr::async_trait]
impl PaymentsService for PaymentsOwner {
    async fn attempt_payment(
        &self,
        request: tonic_bsr::Request<payments::AttemptPaymentRequest>,
    ) -> Result<tonic_bsr::Response<payments::AttemptPaymentResponse>, tonic_bsr::Status> {
        assert!(request.metadata().get("grpc-timeout").is_some());
        let request = request.into_inner();
        self.effects
            .payment_attempt_requests
            .lock()
            .unwrap()
            .push(request);
        Ok(tonic_bsr::Response::new(payments::AttemptPaymentResponse {
            outcome: Some(payments::attempt_payment_response::Outcome::Result(
                payments::PaymentAttemptResult {
                    payment_attempt_id: "attempt-happy-1".to_owned(),
                    status: payments::PaymentAttemptStatus::Processing as i32,
                    recovery_url: String::new(),
                    version: 1,
                    checked_at_token: "attempt-v1".to_owned(),
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
        self.effects
            .payment_read_requests
            .lock()
            .unwrap()
            .push(request.clone());
        Ok(tonic_bsr::Response::new(
            payments::GetSettlementAttemptResponse {
                outcome: Some(payments::get_settlement_attempt_response::Outcome::Result(
                    payments::SettlementAttemptResult {
                        payment_attempt_id: request.payment_attempt_id,
                        auction_id: request.expected_auction_id,
                        settlement_round: request.expected_settlement_round,
                        candidate_id: "candidate-happy-1".to_owned(),
                        status: payments::PaymentAttemptStatus::Succeeded as i32,
                        version: 2,
                        checked_at_token: "attempt-v2".to_owned(),
                        state_data: Some(
                            payments::settlement_attempt_result::StateData::Succeeded(
                                payments::SettlementSuccessData {
                                    payment_id: "payment-happy-1".to_owned(),
                                },
                            ),
                        ),
                    },
                )),
            },
        ))
    }

    async fn expire_payment(
        &self,
        _: tonic_bsr::Request<payments::ExpirePaymentRequest>,
    ) -> Result<tonic_bsr::Response<payments::ExpirePaymentResponse>, tonic_bsr::Status> {
        self.effects.expire_calls.fetch_add(1, Ordering::SeqCst);
        Err(tonic_bsr::Status::failed_precondition(
            "expiry is not valid on happy path",
        ))
    }
}

#[derive(Clone)]
struct NotifyOwner {
    effects: Arc<OwnerEffects>,
}

#[tonic::async_trait]
impl NotifyService for NotifyOwner {
    async fn send_notification(
        &self,
        request: tonic::Request<notify::SendNotificationRequest>,
    ) -> Result<tonic::Response<notify::SendNotificationResponse>, tonic::Status> {
        assert!(request.metadata().get("grpc-timeout").is_some());
        self.effects
            .notification_requests
            .lock()
            .unwrap()
            .push(request.into_inner());
        Ok(tonic::Response::new(notify::SendNotificationResponse {
            outcome: Some(notify::send_notification_response::Outcome::Accepted(
                notify::NotificationAccepted {
                    status: notify::NotificationStatus::Pending as i32,
                    notification_id: "notification-happy".to_owned(),
                    template_version: 1,
                    attempt_count: 0,
                    version: 1,
                },
            )),
        }))
    }
}

struct OwnerServers {
    shutdown: CancellationToken,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl OwnerServers {
    async fn shutdown(self) {
        self.shutdown.cancel();
        for task in self.tasks {
            task.await.unwrap();
        }
    }
}

async fn start_owner_boundaries() -> (OwnerRpcActivities, Arc<OwnerEffects>, OwnerServers) {
    let effects = Arc::new(OwnerEffects::default());
    let shutdown = CancellationToken::new();

    let owner_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let owner_address = owner_listener.local_addr().unwrap();
    let owner_shutdown = shutdown.clone();
    let auction_owner = AuctionOwner {
        effects: effects.clone(),
    };
    let notify_owner = NotifyOwner {
        effects: effects.clone(),
    };
    let owner_task = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(AuctionServiceServer::new(auction_owner))
            .add_service(NotifyServiceServer::new(notify_owner))
            .serve_with_incoming_shutdown(
                TcpListenerStream::new(owner_listener),
                owner_shutdown.cancelled_owned(),
            )
            .await
            .unwrap();
    });

    let payments_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let payments_address = payments_listener.local_addr().unwrap();
    let payments_shutdown = shutdown.clone();
    let payments_owner = PaymentsOwner {
        effects: effects.clone(),
    };
    let payments_task = tokio::spawn(async move {
        tonic_bsr::transport::Server::builder()
            .add_service(PaymentsServiceServer::new(payments_owner))
            .serve_with_incoming_shutdown(
                TcpListenerStream::new(payments_listener),
                payments_shutdown.cancelled_owned(),
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
            "processes-e2e",
        ),
        effects,
        OwnerServers {
            shutdown,
            tasks: vec![owner_task, payments_task],
        },
    )
}

async fn wait_for_effect(effect: impl Fn() -> usize, expected: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while effect() != expected {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

async fn assert_single_workflow(client: &temporalio_client::Client, workflow_id: &str) {
    let query = format!("WorkflowId = '{workflow_id}'");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let count = client
                .count_workflows(query.clone(), WorkflowCountOptions::default())
                .await
                .unwrap()
                .count();
            if count == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "run with `make integration-check`; downloads a pinned Temporal CLI dev server"]
async fn open_reschedule_close_payment_success_confirm_through_owner_grpc_boundaries() {
    let version = std::env::var("TEMPORAL_CLI_VERSION").unwrap_or_else(|_| "v1.8.3".to_owned());
    let environment = WorkflowEnvironment::start_local(
        LocalWorkflowEnvironmentOptions::builder()
            .server_executable(EphemeralExe::CachedDownload {
                version: EphemeralExeVersion::Fixed(version),
                dest_dir: Some("target/temporal-cli-cache".to_owned()),
                ttl: None,
            })
            .extra_args(vec![
                "--search-attribute=GavelProcessType=Keyword".to_owned(),
                "--search-attribute=GavelLifecyclePhase=Keyword".to_owned(),
                "--search-attribute=GavelCloseDeadlineMillis=Int".to_owned(),
            ])
            .build(),
    )
    .await
    .unwrap();
    let runtime = Runtime::from_current_tokio(Default::default()).unwrap();
    let client = environment.client().clone();
    let (owners, effects, owner_servers) = start_owner_boundaries().await;
    let owners = Arc::new(owners);
    let queue = format!("gavel-processes-happy-e2e-{}", std::process::id());
    let options = WorkerOptions::new(queue.clone())
        .register_workflow::<AuctionLifecycleWorkflow>()
        .unwrap()
        .register_workflow::<SettleAuctionWorkflow>()
        .unwrap()
        .register_activities(OwnerTemporalActivities::new(owners.clone()))
        .register_activities(SettlementTemporalActivities::new(
            owners.clone(),
            owners.clone(),
            owners,
        ))
        .build();
    let mut worker = Worker::new(&runtime, client.clone(), options).unwrap();
    let worker_shutdown = worker.shutdown_handle();

    let scenario =
        async {
            let suffix = std::process::id();
            let auction_id = AuctionId::parse(format!("happy-e2e-{suffix}")).unwrap();
            let lifecycle_id = format!("auction-lifecycle/{auction_id}");
            let settlement_id = format!("settle-auction/{auction_id}");
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            let correlation = CorrelationContext {
                correlation_id: "happy-e2e".to_owned(),
                causation_id: "auction-opened".to_owned(),
            };

            let lifecycle = client
                .start_workflow(
                    AuctionLifecycleWorkflow::run,
                    AuctionLifecycleInput {
                        process_id: ProcessId::parse(lifecycle_id.clone()).unwrap(),
                        auction_id: auction_id.clone(),
                        opened_version: 1,
                        close_at_millis: now + 250,
                        correlation: correlation.clone(),
                    },
                    WorkflowStartOptions::new(queue.clone(), lifecycle_id.clone()).build(),
                )
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
            let rescheduled = LifecycleSignal {
                event_id: "auction-rescheduled-2".to_owned(),
                fingerprint: "auction-rescheduled-2-fingerprint".to_owned(),
                aggregate_version: 2,
                kind: LifecycleEventKind::AuctionRescheduled {
                    close_at_millis: now + 700,
                },
            };
            lifecycle
                .signal(
                    AuctionLifecycleWorkflow::owner_event,
                    rescheduled.clone(),
                    WorkflowSignalOptions::default(),
                )
                .await
                .unwrap();
            lifecycle
                .signal(
                    AuctionLifecycleWorkflow::owner_event,
                    rescheduled,
                    WorkflowSignalOptions::default(),
                )
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(350)).await;
            assert_eq!(effects.close_requests.lock().unwrap().len(), 0);
            wait_for_effect(|| effects.close_requests.lock().unwrap().len(), 1).await;

            let settlement = client
                .start_workflow(
                    SettleAuctionWorkflow::run,
                    SettleAuctionInput {
                        auction_id: auction_id.clone(),
                        correlation,
                    },
                    WorkflowStartOptions::new(queue.clone(), settlement_id.clone()).build(),
                )
                .await
                .unwrap();
            let candidate = SettlementSignal {
                event_id: "candidate-selected-3".to_owned(),
                fingerprint: "candidate-selected-3-fingerprint".to_owned(),
                owner_version: 3,
                kind: SettlementOwnerSignal::CandidateSelected {
                    candidate: SettlementCandidate {
                        auction_id: auction_id.clone(),
                        candidate_id: CandidateId::parse("candidate-happy-1").unwrap(),
                        round: SettlementRound::new(1).unwrap(),
                        amount: Money::new(500, "USD").unwrap(),
                        auction_version: 3,
                    },
                },
            };
            for _ in 0..2 {
                settlement
                    .signal(
                        SettleAuctionWorkflow::owner_event,
                        candidate.clone(),
                        WorkflowSignalOptions::default(),
                    )
                    .await
                    .unwrap();
            }
            wait_for_effect(|| effects.payment_attempt_requests.lock().unwrap().len(), 1).await;

            let payment_succeeded = SettlementSignal {
                event_id: "payment-succeeded-2".to_owned(),
                fingerprint: "payment-succeeded-2-fingerprint".to_owned(),
                owner_version: 2,
                kind: SettlementOwnerSignal::PaymentChanged {
                    attempt_id: PaymentAttemptId::parse("attempt-happy-1").unwrap(),
                    round: SettlementRound::new(1).unwrap(),
                    attempt_version: 2,
                },
            };
            settlement
                .signal(
                    SettleAuctionWorkflow::owner_event,
                    payment_succeeded,
                    WorkflowSignalOptions::default(),
                )
                .await
                .unwrap();
            let settlement_result = settlement
                .get_result(WorkflowGetResultOptions::default())
                .await
                .unwrap();
            assert_eq!(settlement_result.terminal, SettlementTerminal::Sold);

            lifecycle
                .signal(
                    AuctionLifecycleWorkflow::owner_event,
                    LifecycleSignal {
                        event_id: "winner-confirmed-4".to_owned(),
                        fingerprint: "winner-confirmed-4-fingerprint".to_owned(),
                        aggregate_version: 4,
                        kind: LifecycleEventKind::Terminal {
                            outcome: LifecycleTerminal::Sold,
                        },
                    },
                    WorkflowSignalOptions::default(),
                )
                .await
                .unwrap();
            let lifecycle_result = lifecycle
                .get_result(WorkflowGetResultOptions::default())
                .await
                .unwrap();
            assert_eq!(lifecycle_result.terminal, LifecycleTerminal::Sold);

            assert_single_workflow(&client, &lifecycle_id).await;
            assert_single_workflow(&client, &settlement_id).await;

            let close_requests = effects.close_requests.lock().unwrap();
            assert_eq!(close_requests.len(), 1);
            assert_eq!(close_requests[0].auction_id, auction_id.as_str());
            assert_eq!(
                close_requests[0].idempotency_key,
                format!("auction-lifecycle/{auction_id}:close")
            );
            drop(close_requests);

            let confirm_requests = effects.confirm_requests.lock().unwrap();
            assert_eq!(confirm_requests.len(), 1);
            assert_eq!(confirm_requests[0].auction_id, auction_id.as_str());
            assert_eq!(confirm_requests[0].candidate_id, "candidate-happy-1");
            assert_eq!(confirm_requests[0].payment_id, "payment-happy-1");
            assert_eq!(
                confirm_requests[0].idempotency_key,
                format!("settle-auction/{auction_id}:confirm:1")
            );
            drop(confirm_requests);
            assert_eq!(effects.forfeit_calls.load(Ordering::SeqCst), 0);

            let attempt_requests = effects.payment_attempt_requests.lock().unwrap();
            assert_eq!(attempt_requests.len(), 1);
            assert_eq!(attempt_requests[0].auction_id, auction_id.as_str());
            assert_eq!(attempt_requests[0].settlement_round, 1);
            assert_eq!(attempt_requests[0].candidate_id, "candidate-happy-1");
            assert_eq!(
                attempt_requests[0].idempotency_key,
                format!("settle-auction/{auction_id}:attempt-payment:1")
            );
            drop(attempt_requests);
            assert_eq!(effects.payment_read_requests.lock().unwrap().len(), 1);
            assert_eq!(effects.expire_calls.load(Ordering::SeqCst), 0);

            let notifications = effects.notification_requests.lock().unwrap();
            assert_eq!(notifications.len(), 2);
            assert!(notifications.iter().all(|request| {
                request.data.get("auction_id") == Some(&auction_id.to_string())
            }));
            assert_eq!(
                notifications
                    .iter()
                    .filter(|request| {
                        request.template == notify::NotificationTemplate::SettlementCandidate as i32
                    })
                    .count(),
                1
            );
            assert_eq!(
                notifications
                    .iter()
                    .filter(|request| {
                        request.template == notify::NotificationTemplate::WinnerConfirmed as i32
                    })
                    .count(),
                1
            );
            drop(notifications);
            worker_shutdown();
        };

    let (worker_result, ()) = tokio::join!(worker.run(), scenario);
    worker_result.unwrap();
    owner_servers.shutdown().await;
    drop(worker);
    drop(runtime);
    environment.shutdown().await.unwrap();
}
