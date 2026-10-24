use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use gavel_processes::{
    activities::{
        AttemptPaymentIntent, AuctionActivities, AuctionMutationOutcome, CloseAuctionIntent,
        ConfirmWinnerIntent, ExpirePaymentIntent, ForfeitCandidateIntent,
        NotificationAcknowledgement, NotifyActivities, NotifyIntent, OwnerTemporalActivities,
        PaymentsActivities, RecoveryNotificationIntent, SettlementAttemptRead,
        SettlementAttemptSnapshot, SettlementTemporalActivities,
    },
    workflows::{
        AuctionId, AuctionLifecycleInput, AuctionLifecycleWorkflow, CandidateId,
        CorrelationContext, LifecycleEventKind, LifecycleSignal, LifecycleTerminal, Money,
        PaymentAttemptId, ProcessError, ProcessId, SettleAuctionInput, SettleAuctionWorkflow,
        SettlementAttemptStatus, SettlementCandidate, SettlementOwnerSignal, SettlementRound,
        SettlementSignal, SettlementTerminal,
    },
};

use temporalio_client::{
    WorkflowFetchHistoryOptions, WorkflowGetResultOptions, WorkflowHistory, WorkflowQueryOptions,
    WorkflowSignalOptions, WorkflowStartOptions,
};
use temporalio_macros::{activities, workflow, workflow_methods};
use temporalio_sdk::{
    ActivityOptions, Runtime, Worker, WorkerOptions, WorkflowContext, WorkflowResult,
    activities::{ActivityContext, ActivityError},
    testing::{
        EphemeralExe, EphemeralExeVersion, LocalWorkflowEnvironmentOptions, WorkflowEnvironment,
    },
    workflow_replayer::{WorkflowReplayer, WorkflowReplayerOptions},
};

#[workflow]
#[derive(Default)]
struct SyntheticWorkflow;

#[workflow_methods]
impl SyntheticWorkflow {
    #[run]
    async fn run(context: &mut WorkflowContext<Self>, value: String) -> WorkflowResult<String> {
        context.timer(Duration::from_millis(50)).await;
        let result = context
            .execute_activity(
                SyntheticActivities::complete,
                value,
                ActivityOptions::start_to_close_timeout(Duration::from_secs(5)),
            )
            .await?;
        Ok(result)
    }
}

struct SyntheticActivities;

#[activities]
impl SyntheticActivities {
    #[activity]
    async fn complete(_context: ActivityContext, value: String) -> Result<String, ActivityError> {
        Ok(format!("completed:{value}"))
    }
}

struct CountingAuction {
    closes: AtomicUsize,
    keys: Mutex<Vec<String>>,
}

struct SuccessfulSettlementOwner {
    confirmations: AtomicUsize,
    notifications: AtomicUsize,
}

#[async_trait]
impl AuctionActivities for SuccessfulSettlementOwner {
    async fn close_auction(
        &self,
        _: CloseAuctionIntent,
    ) -> Result<AuctionMutationOutcome, ProcessError> {
        unreachable!()
    }

    async fn forfeit_settlement_candidate(
        &self,
        _: ForfeitCandidateIntent,
    ) -> Result<AuctionMutationOutcome, ProcessError> {
        unreachable!()
    }

    async fn confirm_winner(
        &self,
        _: ConfirmWinnerIntent,
    ) -> Result<AuctionMutationOutcome, ProcessError> {
        self.confirmations.fetch_add(1, Ordering::SeqCst);
        Ok(AuctionMutationOutcome { version: 2 })
    }
}

#[async_trait]
impl PaymentsActivities for SuccessfulSettlementOwner {
    async fn attempt_payment(
        &self,
        _: AttemptPaymentIntent,
    ) -> Result<SettlementAttemptSnapshot, ProcessError> {
        Ok(successful_attempt())
    }

    async fn get_settlement_attempt(
        &self,
        _: SettlementAttemptRead,
    ) -> Result<SettlementAttemptSnapshot, ProcessError> {
        Ok(successful_attempt())
    }

    async fn expire_payment(
        &self,
        _: ExpirePaymentIntent,
    ) -> Result<SettlementAttemptSnapshot, ProcessError> {
        unreachable!()
    }
}

#[async_trait]
impl NotifyActivities for SuccessfulSettlementOwner {
    async fn notify(&self, _: NotifyIntent) -> Result<NotificationAcknowledgement, ProcessError> {
        self.notifications.fetch_add(1, Ordering::SeqCst);
        Ok(NotificationAcknowledgement {
            notification_id: "redacted-notification".to_owned(),
            accepted: true,
        })
    }

    async fn notify_payment_recovery(
        &self,
        _: RecoveryNotificationIntent,
    ) -> Result<NotificationAcknowledgement, ProcessError> {
        unreachable!()
    }
}

fn successful_attempt() -> SettlementAttemptSnapshot {
    SettlementAttemptSnapshot {
        attempt_id: PaymentAttemptId::parse("attempt-1").unwrap(),
        candidate_id: CandidateId::parse("candidate-1").unwrap(),
        status: SettlementAttemptStatus::Succeeded {
            payment_id: "payment-1".to_owned(),
        },
        version: 1,
        checked_at_token: "redacted".to_owned(),
    }
}

#[async_trait]
impl AuctionActivities for CountingAuction {
    async fn close_auction(
        &self,
        intent: CloseAuctionIntent,
    ) -> Result<AuctionMutationOutcome, ProcessError> {
        self.closes.fetch_add(1, Ordering::SeqCst);
        self.keys.lock().unwrap().push(intent.idempotency_key);
        Ok(AuctionMutationOutcome { version: 3 })
    }

    async fn forfeit_settlement_candidate(
        &self,
        _: ForfeitCandidateIntent,
    ) -> Result<AuctionMutationOutcome, ProcessError> {
        unreachable!()
    }

    async fn confirm_winner(
        &self,
        _: ConfirmWinnerIntent,
    ) -> Result<AuctionMutationOutcome, ProcessError> {
        unreachable!()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "run with `make integration-check`; downloads a pinned Temporal CLI dev server"]
async fn synthetic_start_timer_activity_completion_and_cleanup() {
    let version = std::env::var("TEMPORAL_CLI_VERSION").unwrap_or_else(|_| "v1.8.3".to_owned());
    let environment = WorkflowEnvironment::start_local(
        LocalWorkflowEnvironmentOptions::builder()
            .server_executable(EphemeralExe::CachedDownload {
                version: EphemeralExeVersion::Fixed(version),
                dest_dir: Some("target/temporal-cli-cache".to_owned()),
                ttl: None,
            })
            .build(),
    )
    .await
    .unwrap();
    let runtime = Runtime::from_current_tokio(Default::default()).unwrap();
    let client = environment.client().clone();
    let queue = "gavel-processes-synthetic-v1";
    let options = WorkerOptions::new(queue)
        .register_workflow::<SyntheticWorkflow>()
        .unwrap()
        .register_activities(SyntheticActivities)
        .build();
    let mut worker = Worker::new(&runtime, client.clone(), options).unwrap();
    let shutdown = worker.shutdown_handle();

    let scenario = async move {
        let workflow_id = format!("gavel-processes-synthetic-{}", std::process::id());
        let handle = client
            .start_workflow(
                SyntheticWorkflow::run,
                "value".to_owned(),
                WorkflowStartOptions::new(queue, workflow_id).build(),
            )
            .await
            .unwrap();
        let result = handle
            .get_result(WorkflowGetResultOptions::default())
            .await
            .unwrap();
        let history = handle
            .fetch_history(WorkflowFetchHistoryOptions::default())
            .to_json()
            .await
            .unwrap();
        shutdown();
        (result, history)
    };

    let (worker_result, (result, history)) = tokio::join!(worker.run(), scenario);
    worker_result.unwrap();
    assert_eq!(result, "completed:value");
    drop(worker);
    drop(runtime);
    environment.shutdown().await.unwrap();

    let options = WorkflowReplayerOptions::new()
        .register_workflow::<SyntheticWorkflow>()
        .unwrap()
        .build();
    WorkflowReplayer::new(options)
        .unwrap()
        .replay_workflow(WorkflowHistory::from_json(&history).unwrap())
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "run with `make integration-check`; downloads a pinned Temporal CLI dev server"]
async fn lifecycle_replaces_old_timer_and_replays_once_after_worker_restart() {
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
    let auction = Arc::new(CountingAuction {
        closes: AtomicUsize::new(0),
        keys: Mutex::new(Vec::new()),
    });
    let queue = "gavel-processes-lifecycle-restart-v1";

    let worker_options = WorkerOptions::new(queue)
        .max_cached_workflows(0)
        .register_workflow::<AuctionLifecycleWorkflow>()
        .unwrap()
        .register_activities(OwnerTemporalActivities::new(auction.clone()))
        .build();
    let mut first_worker = Worker::new(&runtime, client.clone(), worker_options).unwrap();
    let first_shutdown = first_worker.shutdown_handle();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    let first_phase = async {
        let handle = client
            .start_workflow(
                AuctionLifecycleWorkflow::run,
                AuctionLifecycleInput {
                    process_id: ProcessId::parse("auction-lifecycle/temporal-test").unwrap(),
                    auction_id: AuctionId::parse("temporal-test").unwrap(),
                    opened_version: 1,
                    close_at_millis: now + 400,
                    correlation: CorrelationContext {
                        correlation_id: "integration".to_owned(),
                        causation_id: "opened".to_owned(),
                    },
                },
                WorkflowStartOptions::new(queue, "auction-lifecycle/temporal-test").build(),
            )
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        handle
            .signal(
                AuctionLifecycleWorkflow::owner_event,
                LifecycleSignal {
                    event_id: "rescheduled-2".to_owned(),
                    fingerprint: "rescheduled-fingerprint-2".to_owned(),
                    aggregate_version: 2,
                    kind: LifecycleEventKind::AuctionRescheduled {
                        close_at_millis: now + 1_200,
                    },
                },
                WorkflowSignalOptions::default(),
            )
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(auction.closes.load(Ordering::SeqCst), 0);
        let state = handle
            .query(
                AuctionLifecycleWorkflow::lifecycle_state,
                (),
                WorkflowQueryOptions::default(),
            )
            .await
            .unwrap();
        assert_eq!(state.close_at_millis, now + 1_200);
        first_shutdown();
        handle
    };
    let (first_result, handle) = tokio::join!(first_worker.run(), first_phase);
    first_result.unwrap();
    drop(first_worker);

    let worker_options = WorkerOptions::new(queue)
        .max_cached_workflows(0)
        .register_workflow::<AuctionLifecycleWorkflow>()
        .unwrap()
        .register_activities(OwnerTemporalActivities::new(auction.clone()))
        .build();
    let mut second_worker = Worker::new(&runtime, client, worker_options).unwrap();
    let second_shutdown = second_worker.shutdown_handle();
    let second_phase = async {
        let closed = tokio::time::timeout(Duration::from_secs(5), async {
            while auction.closes.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await;
        if closed.is_err() {
            let state = handle
                .query(
                    AuctionLifecycleWorkflow::lifecycle_state,
                    (),
                    WorkflowQueryOptions::default(),
                )
                .await
                .unwrap();
            panic!("close activity did not execute after restart; state={state:?}");
        }
        handle
            .signal(
                AuctionLifecycleWorkflow::owner_event,
                LifecycleSignal {
                    event_id: "terminal-3".to_owned(),
                    fingerprint: "terminal-fingerprint-3".to_owned(),
                    aggregate_version: 3,
                    kind: LifecycleEventKind::Terminal {
                        outcome: LifecycleTerminal::Unsold,
                    },
                },
                WorkflowSignalOptions::default(),
            )
            .await
            .unwrap();
        let result = handle
            .get_result(WorkflowGetResultOptions::default())
            .await
            .unwrap();
        let history = handle
            .fetch_history(WorkflowFetchHistoryOptions::default())
            .to_json()
            .await
            .unwrap();
        second_shutdown();
        (result, history)
    };
    let (second_result, (result, history)) = tokio::join!(second_worker.run(), second_phase);
    second_result.unwrap();
    assert_eq!(result.terminal, LifecycleTerminal::Unsold);
    assert_eq!(auction.closes.load(Ordering::SeqCst), 1);
    assert_eq!(
        auction.keys.lock().unwrap().as_slice(),
        ["auction-lifecycle/temporal-test:close"]
    );
    drop(second_worker);
    drop(runtime);
    environment.shutdown().await.unwrap();

    let options = WorkflowReplayerOptions::new()
        .register_workflow::<AuctionLifecycleWorkflow>()
        .unwrap()
        .build();
    WorkflowReplayer::new(options)
        .unwrap()
        .replay_workflow(WorkflowHistory::from_json(&history).unwrap())
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "run with `make integration-check`; downloads a pinned Temporal CLI dev server"]
async fn settlement_success_history_replays_and_keeps_secrets_out() {
    let version = std::env::var("TEMPORAL_CLI_VERSION").unwrap_or_else(|_| "v1.8.3".to_owned());
    let environment = WorkflowEnvironment::start_local(
        LocalWorkflowEnvironmentOptions::builder()
            .server_executable(EphemeralExe::CachedDownload {
                version: EphemeralExeVersion::Fixed(version),
                dest_dir: Some("target/temporal-cli-cache".to_owned()),
                ttl: None,
            })
            .build(),
    )
    .await
    .unwrap();
    let runtime = Runtime::from_current_tokio(Default::default()).unwrap();
    let client = environment.client().clone();
    let owners = Arc::new(SuccessfulSettlementOwner {
        confirmations: AtomicUsize::new(0),
        notifications: AtomicUsize::new(0),
    });
    let queue = "gavel-processes-settlement-success-v1";
    let options = WorkerOptions::new(queue)
        .register_workflow::<SettleAuctionWorkflow>()
        .unwrap()
        .register_activities(SettlementTemporalActivities::new(
            owners.clone(),
            owners.clone(),
            owners.clone(),
        ))
        .build();
    let mut worker = Worker::new(&runtime, client.clone(), options).unwrap();
    let shutdown = worker.shutdown_handle();

    let scenario = async {
        let handle = client
            .start_workflow(
                SettleAuctionWorkflow::run,
                SettleAuctionInput {
                    auction_id: AuctionId::parse("settlement-temporal-test").unwrap(),
                    correlation: CorrelationContext {
                        correlation_id: "integration".to_owned(),
                        causation_id: "candidate".to_owned(),
                    },
                },
                WorkflowStartOptions::new(queue, "settle-auction/temporal-test").build(),
            )
            .await
            .unwrap();
        handle
            .signal(
                SettleAuctionWorkflow::owner_event,
                SettlementSignal {
                    event_id: "candidate-1".to_owned(),
                    fingerprint: "candidate-fingerprint-1".to_owned(),
                    owner_version: 1,
                    kind: SettlementOwnerSignal::CandidateSelected {
                        candidate: SettlementCandidate {
                            auction_id: AuctionId::parse("settlement-temporal-test").unwrap(),
                            candidate_id: CandidateId::parse("candidate-1").unwrap(),
                            round: SettlementRound::new(1).unwrap(),
                            amount: Money::new(500, "USD").unwrap(),
                            auction_version: 1,
                        },
                    },
                },
                WorkflowSignalOptions::default(),
            )
            .await
            .unwrap();
        let result = handle
            .get_result(WorkflowGetResultOptions::default())
            .await
            .unwrap();
        let history = handle
            .fetch_history(WorkflowFetchHistoryOptions::default())
            .to_json()
            .await
            .unwrap();
        shutdown();
        (result, history)
    };
    let (worker_result, (result, history)) = tokio::join!(worker.run(), scenario);
    worker_result.unwrap();
    assert_eq!(result.terminal, SettlementTerminal::Sold);
    assert_eq!(owners.confirmations.load(Ordering::SeqCst), 1);
    assert_eq!(owners.notifications.load(Ordering::SeqCst), 2);
    let history_text = String::from_utf8(history.clone())
        .unwrap()
        .to_ascii_lowercase();
    for forbidden in ["recovery_url", "https://", "provider_reference", "max_bid"] {
        assert!(!history_text.contains(forbidden));
    }
    drop(worker);
    drop(runtime);
    environment.shutdown().await.unwrap();

    let options = WorkflowReplayerOptions::new()
        .register_workflow::<SettleAuctionWorkflow>()
        .unwrap()
        .build();
    WorkflowReplayer::new(options)
        .unwrap()
        .replay_workflow(WorkflowHistory::from_json(&history).unwrap())
        .await
        .unwrap();
}
