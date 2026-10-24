use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use gavel_processes::{
    activities::{
        AttemptPaymentIntent, AuctionActivities, AuctionMutationOutcome, CloseAuctionIntent,
        ConfirmWinnerIntent, ExpirePaymentIntent, ForfeitCandidateIntent,
        NotificationAcknowledgement, NotifyActivities, NotifyIntent, OutbidNotificationIntent,
        PaymentsActivities, RecoveryNotificationIntent, SettlementAttemptRead,
        SettlementAttemptSnapshot, SettlementTemporalActivities,
    },
    workflows::{
        AuctionId, CandidateId, CorrelationContext, Money, PaymentAttemptId, ProcessError,
        SettleAuctionInput, SettleAuctionRun, SettleAuctionWorkflow, SettlementAttemptStatus,
        SettlementCandidate, SettlementOwnerSignal, SettlementPhase, SettlementRound,
        SettlementSignal, SettlementTerminal,
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
use tonic_temporal::IntoRequest;
use url::Url;

const QUEUE: &str = "gavel-processes-settlement-recovery-e2e-v1";

#[derive(Clone, Copy, Debug)]
enum ExpiryOutcome {
    Expired,
    ConcurrentSuccess,
}

#[derive(Clone, Debug)]
struct PaymentRecord {
    attempt_id: PaymentAttemptId,
    candidate_id: CandidateId,
    status: SettlementAttemptStatus,
    version: u64,
    expiry_outcome: ExpiryOutcome,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Effects {
    expire: usize,
    forfeit: usize,
    confirm: usize,
    recovery_notify: usize,
}

#[derive(Default)]
struct RecoveryOwners {
    records: Mutex<HashMap<String, PaymentRecord>>,
    effects: Mutex<HashMap<String, Effects>>,
}

impl RecoveryOwners {
    fn register(&self, auction: &str, expiry_outcome: ExpiryOutcome) {
        self.records.lock().unwrap().insert(
            auction.to_owned(),
            PaymentRecord {
                attempt_id: PaymentAttemptId::parse(format!("attempt-{auction}")).unwrap(),
                candidate_id: CandidateId::parse(format!("candidate-{auction}")).unwrap(),
                status: SettlementAttemptStatus::RecoveryRequired,
                version: 1,
                expiry_outcome,
            },
        );
        self.effects
            .lock()
            .unwrap()
            .insert(auction.to_owned(), Effects::default());
    }

    fn succeed(&self, auction: &str) -> (PaymentAttemptId, u64) {
        let mut records = self.records.lock().unwrap();
        let record = records.get_mut(auction).unwrap();
        record.version += 1;
        record.status = SettlementAttemptStatus::Succeeded {
            payment_id: format!("payment-{auction}"),
        };
        (record.attempt_id.clone(), record.version)
    }

    fn snapshot(&self, auction: &str) -> SettlementAttemptSnapshot {
        let records = self.records.lock().unwrap();
        let record = records.get(auction).unwrap();
        SettlementAttemptSnapshot {
            attempt_id: record.attempt_id.clone(),
            candidate_id: record.candidate_id.clone(),
            status: record.status.clone(),
            version: record.version,
            checked_at_token: "redacted".to_owned(),
        }
    }

    fn effects(&self, auction: &str) -> Effects {
        self.effects.lock().unwrap().get(auction).unwrap().clone()
    }

    fn record_effect(&self, auction: &str, update: impl FnOnce(&mut Effects)) {
        update(self.effects.lock().unwrap().get_mut(auction).unwrap());
    }
}

#[async_trait]
impl PaymentsActivities for RecoveryOwners {
    async fn attempt_payment(
        &self,
        intent: AttemptPaymentIntent,
    ) -> Result<SettlementAttemptSnapshot, ProcessError> {
        let snapshot = self.snapshot(intent.auction_id.as_str());
        assert_eq!(snapshot.candidate_id, intent.candidate_id);
        Ok(snapshot)
    }

    async fn get_settlement_attempt(
        &self,
        read: SettlementAttemptRead,
    ) -> Result<SettlementAttemptSnapshot, ProcessError> {
        let snapshot = self.snapshot(read.expected_auction_id.as_str());
        assert_eq!(snapshot.attempt_id, read.attempt_id);
        Ok(snapshot)
    }

    async fn expire_payment(
        &self,
        intent: ExpirePaymentIntent,
    ) -> Result<SettlementAttemptSnapshot, ProcessError> {
        let auction = intent.auction_id.as_str();
        self.record_effect(auction, |effects| effects.expire += 1);
        let mut records = self.records.lock().unwrap();
        let record = records.get_mut(auction).unwrap();
        assert_eq!(record.attempt_id, intent.attempt_id);
        record.version += 1;
        record.status = match record.expiry_outcome {
            ExpiryOutcome::Expired => SettlementAttemptStatus::Expired,
            ExpiryOutcome::ConcurrentSuccess => SettlementAttemptStatus::Succeeded {
                payment_id: format!("payment-{auction}"),
            },
        };
        Ok(SettlementAttemptSnapshot {
            attempt_id: record.attempt_id.clone(),
            candidate_id: record.candidate_id.clone(),
            status: record.status.clone(),
            version: record.version,
            checked_at_token: "redacted".to_owned(),
        })
    }
}

#[async_trait]
impl AuctionActivities for RecoveryOwners {
    async fn close_auction(
        &self,
        _: CloseAuctionIntent,
    ) -> Result<AuctionMutationOutcome, ProcessError> {
        unreachable!()
    }

    async fn forfeit_settlement_candidate(
        &self,
        intent: ForfeitCandidateIntent,
    ) -> Result<AuctionMutationOutcome, ProcessError> {
        self.record_effect(intent.auction_id.as_str(), |effects| effects.forfeit += 1);
        Ok(AuctionMutationOutcome { version: 2 })
    }

    async fn confirm_winner(
        &self,
        intent: ConfirmWinnerIntent,
    ) -> Result<AuctionMutationOutcome, ProcessError> {
        self.record_effect(intent.auction_id.as_str(), |effects| effects.confirm += 1);
        Ok(AuctionMutationOutcome { version: 2 })
    }
}

#[async_trait]
impl NotifyActivities for RecoveryOwners {
    async fn notify(&self, _: NotifyIntent) -> Result<NotificationAcknowledgement, ProcessError> {
        Ok(NotificationAcknowledgement {
            notification_id: "redacted-notification".to_owned(),
            accepted: true,
        })
    }

    async fn notify_payment_recovery(
        &self,
        intent: RecoveryNotificationIntent,
    ) -> Result<NotificationAcknowledgement, ProcessError> {
        self.record_effect(intent.attempt.expected_auction_id.as_str(), |effects| {
            effects.recovery_notify += 1;
        });
        Ok(NotificationAcknowledgement {
            notification_id: "redacted-recovery-notification".to_owned(),
            accepted: true,
        })
    }

    async fn notify_outbid(
        &self,
        _: OutbidNotificationIntent,
    ) -> Result<NotificationAcknowledgement, ProcessError> {
        unreachable!()
    }
}

type SettlementHandle = WorkflowHandle<Client, SettleAuctionRun>;

fn worker_options(owners: Arc<RecoveryOwners>) -> WorkerOptions {
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

async fn start_recovery(
    client: &Client,
    owners: &RecoveryOwners,
    auction: &str,
    expiry_outcome: ExpiryOutcome,
) -> SettlementHandle {
    owners.register(auction, expiry_outcome);
    let handle = client
        .start_workflow(
            SettleAuctionWorkflow::run,
            SettleAuctionInput {
                auction_id: AuctionId::parse(auction).unwrap(),
                correlation: CorrelationContext {
                    correlation_id: format!("correlation-{auction}"),
                    causation_id: format!("candidate-{auction}"),
                },
            },
            WorkflowStartOptions::new(QUEUE, format!("settle-auction/{auction}")).build(),
        )
        .await
        .unwrap();
    handle
        .signal(
            SettleAuctionWorkflow::owner_event,
            SettlementSignal {
                event_id: format!("candidate-event-{auction}"),
                fingerprint: format!("candidate-fingerprint-{auction}"),
                owner_version: 1,
                kind: SettlementOwnerSignal::CandidateSelected {
                    candidate: SettlementCandidate {
                        auction_id: AuctionId::parse(auction).unwrap(),
                        candidate_id: CandidateId::parse(format!("candidate-{auction}")).unwrap(),
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
    handle
}

async fn signal_payment(
    handle: &SettlementHandle,
    auction: &str,
    attempt_id: PaymentAttemptId,
    version: u64,
) {
    handle
        .signal(
            SettleAuctionWorkflow::owner_event,
            SettlementSignal {
                event_id: format!("payment-event-{auction}-{version}"),
                fingerprint: format!("payment-fingerprint-{auction}-{version}"),
                owner_version: version,
                kind: SettlementOwnerSignal::PaymentChanged {
                    attempt_id,
                    round: SettlementRound::new(1).unwrap(),
                    attempt_version: version,
                },
            },
            WorkflowSignalOptions::default(),
        )
        .await
        .unwrap();
}

async fn signal_unsold(handle: &SettlementHandle, auction: &str) {
    handle
        .signal(
            SettleAuctionWorkflow::owner_event,
            SettlementSignal {
                event_id: format!("terminal-event-{auction}"),
                fingerprint: format!("terminal-fingerprint-{auction}"),
                owner_version: 2,
                kind: SettlementOwnerSignal::AuctionTerminal {
                    outcome: SettlementTerminal::Unsold,
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

async fn result(handle: &SettlementHandle) -> SettlementTerminal {
    handle
        .get_result(WorkflowGetResultOptions::default())
        .await
        .unwrap()
        .terminal
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "runs the SDK-pinned Temporal test server with virtual time"]
async fn recovery_races_restart_replay_and_converge_authoritatively() {
    let mut server = TestServerConfig::builder()
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
        ConnectionOptions::new(Url::parse(&format!("http://{}", server.target)).unwrap())
            .identity("gavel-processes-recovery-test".to_owned())
            .client_name("gavel-processes".to_owned())
            .client_version(env!("CARGO_PKG_VERSION").to_owned())
            .build(),
        ClientOptions::new("default").build(),
    )
    .await
    .unwrap();
    let runtime = Runtime::from_current_tokio(Default::default()).unwrap();
    let owners = Arc::new(RecoveryOwners::default());
    let mut first_worker =
        Worker::new(&runtime, client.clone(), worker_options(owners.clone())).unwrap();
    let first_shutdown = first_worker.shutdown_handle();

    let first_phase = async {
        let success = start_recovery(
            &client,
            &owners,
            "recovery-success-before-expiry",
            ExpiryOutcome::Expired,
        )
        .await;
        wait_for_phase(&success, SettlementPhase::RecoveryRequired).await;
        let (attempt_id, version) = owners.succeed("recovery-success-before-expiry");
        signal_payment(
            &success,
            "recovery-success-before-expiry",
            attempt_id,
            version,
        )
        .await;
        assert_eq!(result(&success).await, SettlementTerminal::Sold);
        assert_eq!(
            owners.effects("recovery-success-before-expiry"),
            Effects {
                confirm: 1,
                recovery_notify: 1,
                ..Effects::default()
            }
        );

        let expired = start_recovery(
            &client,
            &owners,
            "recovery-expiry-before-late-success",
            ExpiryOutcome::Expired,
        )
        .await;
        wait_for_phase(&expired, SettlementPhase::RecoveryRequired).await;
        skip_recovery_window(&client).await;
        wait_for_phase(&expired, SettlementPhase::AwaitingNextRound).await;
        let (attempt_id, version) = owners.succeed("recovery-expiry-before-late-success");
        signal_payment(
            &expired,
            "recovery-expiry-before-late-success",
            attempt_id,
            version,
        )
        .await;
        signal_unsold(&expired, "recovery-expiry-before-late-success").await;
        assert_eq!(result(&expired).await, SettlementTerminal::Unsold);
        assert_eq!(
            owners.effects("recovery-expiry-before-late-success"),
            Effects {
                expire: 1,
                forfeit: 1,
                recovery_notify: 1,
                ..Effects::default()
            }
        );

        let ambiguous = start_recovery(
            &client,
            &owners,
            "recovery-ambiguous-expiry",
            ExpiryOutcome::ConcurrentSuccess,
        )
        .await;
        wait_for_phase(&ambiguous, SettlementPhase::RecoveryRequired).await;
        skip_recovery_window(&client).await;
        assert_eq!(result(&ambiguous).await, SettlementTerminal::Sold);
        assert_eq!(
            owners.effects("recovery-ambiguous-expiry"),
            Effects {
                expire: 1,
                confirm: 1,
                recovery_notify: 1,
                ..Effects::default()
            }
        );

        let restart = start_recovery(
            &client,
            &owners,
            "recovery-worker-restart",
            ExpiryOutcome::Expired,
        )
        .await;
        wait_for_phase(&restart, SettlementPhase::RecoveryRequired).await;
        first_shutdown();
        restart
    };

    let (first_result, restart) = tokio::join!(first_worker.run(), first_phase);
    first_result.unwrap();
    drop(first_worker);

    let mut second_worker =
        Worker::new(&runtime, client.clone(), worker_options(owners.clone())).unwrap();
    let second_shutdown = second_worker.shutdown_handle();
    let second_phase = async {
        let (attempt_id, version) = owners.succeed("recovery-worker-restart");
        signal_payment(&restart, "recovery-worker-restart", attempt_id, version).await;
        assert_eq!(result(&restart).await, SettlementTerminal::Sold);
        let history = restart
            .fetch_history(WorkflowFetchHistoryOptions::default())
            .to_json()
            .await
            .unwrap();
        second_shutdown();
        history
    };
    let (second_result, history) = tokio::join!(second_worker.run(), second_phase);
    second_result.unwrap();
    assert_eq!(
        owners.effects("recovery-worker-restart"),
        Effects {
            confirm: 1,
            recovery_notify: 1,
            ..Effects::default()
        }
    );
    drop(second_worker);
    drop(runtime);
    drop(client);
    server.shutdown().await.unwrap();

    let replay_options = WorkflowReplayerOptions::new()
        .register_workflow::<SettleAuctionWorkflow>()
        .unwrap()
        .build();
    WorkflowReplayer::new(replay_options)
        .unwrap()
        .replay_workflow(WorkflowHistory::from_json(&history).unwrap())
        .await
        .unwrap();
}
