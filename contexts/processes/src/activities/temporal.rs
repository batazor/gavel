use std::sync::Arc;

use serde::{Deserialize, Serialize};
use temporalio_macros::activities;
use temporalio_sdk::{
    ApplicationFailure,
    activities::{ActivityContext, ActivityError},
};

use super::{
    AttemptPaymentIntent, AuctionActivities, AuctionMutationOutcome, CloseAuctionIntent,
    ConfirmWinnerIntent, ExpirePaymentIntent, ForfeitCandidateIntent, NotificationAcknowledgement,
    NotifyActivities, NotifyIntent, PaymentsActivities, RecoveryNotificationIntent,
    SettlementAttemptRead, SettlementAttemptSnapshot,
};
use crate::{
    telemetry::{MetricsTelemetry, ProcessTelemetry},
    workflows::ProcessError,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CloseAuctionActivityOutcome {
    Closed(AuctionMutationOutcome),
    Rejected { code: String },
}

pub struct OwnerTemporalActivities {
    auction: Arc<dyn AuctionActivities>,
}

pub struct SettlementTemporalActivities {
    auction: Arc<dyn AuctionActivities>,
    payments: Arc<dyn PaymentsActivities>,
    notify: Arc<dyn NotifyActivities>,
}

impl SettlementTemporalActivities {
    pub fn new(
        auction: Arc<dyn AuctionActivities>,
        payments: Arc<dyn PaymentsActivities>,
        notify: Arc<dyn NotifyActivities>,
    ) -> Self {
        Self {
            auction,
            payments,
            notify,
        }
    }
}

impl OwnerTemporalActivities {
    pub fn new(auction: Arc<dyn AuctionActivities>) -> Self {
        Self { auction }
    }
}

#[activities]
impl SettlementTemporalActivities {
    #[activity]
    pub async fn attempt_payment(
        self: Arc<Self>,
        _context: ActivityContext,
        intent: AttemptPaymentIntent,
    ) -> Result<SettlementAttemptSnapshot, ActivityError> {
        let result = self.payments.attempt_payment(intent).await;
        record_activity_result("attempt_payment", &result);
        result.map_err(process_activity_error)
    }

    #[activity]
    pub async fn get_settlement_attempt(
        self: Arc<Self>,
        _context: ActivityContext,
        read: SettlementAttemptRead,
    ) -> Result<SettlementAttemptSnapshot, ActivityError> {
        let result = self.payments.get_settlement_attempt(read).await;
        record_activity_result("get_settlement_attempt", &result);
        result.map_err(process_activity_error)
    }

    #[activity]
    pub async fn expire_payment(
        self: Arc<Self>,
        _context: ActivityContext,
        intent: ExpirePaymentIntent,
    ) -> Result<SettlementAttemptSnapshot, ActivityError> {
        let result = self.payments.expire_payment(intent).await;
        record_activity_result("expire_payment", &result);
        result.map_err(process_activity_error)
    }

    #[activity]
    pub async fn forfeit_candidate(
        self: Arc<Self>,
        _context: ActivityContext,
        intent: ForfeitCandidateIntent,
    ) -> Result<AuctionMutationOutcome, ActivityError> {
        let result = self.auction.forfeit_settlement_candidate(intent).await;
        record_activity_result("forfeit_candidate", &result);
        result.map_err(process_activity_error)
    }

    #[activity]
    pub async fn confirm_winner(
        self: Arc<Self>,
        _context: ActivityContext,
        intent: ConfirmWinnerIntent,
    ) -> Result<AuctionMutationOutcome, ActivityError> {
        let result = self.auction.confirm_winner(intent).await;
        record_activity_result("confirm_winner", &result);
        result.map_err(process_activity_error)
    }

    #[activity]
    pub async fn notify(
        self: Arc<Self>,
        _context: ActivityContext,
        intent: NotifyIntent,
    ) -> Result<NotificationAcknowledgement, ActivityError> {
        let result = self.notify.notify(intent).await;
        record_activity_result("notify", &result);
        result.map_err(process_activity_error)
    }

    #[activity]
    pub async fn notify_recovery(
        self: Arc<Self>,
        _context: ActivityContext,
        intent: RecoveryNotificationIntent,
    ) -> Result<NotificationAcknowledgement, ActivityError> {
        let result = self.notify.notify_payment_recovery(intent).await;
        record_activity_result("notify_recovery", &result);
        result.map_err(process_activity_error)
    }
}

fn record_activity_result<T>(activity_type: &'static str, result: &Result<T, ProcessError>) {
    let outcome = match result {
        Ok(_) => "success",
        Err(ProcessError::DependencyUnavailable) => "retryable_failure",
        Err(ProcessError::OwnerRejected { .. }) => "owner_rejected",
        Err(ProcessError::InvalidInput) => "invalid_input",
        Err(
            ProcessError::InvariantViolation
            | ProcessError::StaleEvent
            | ProcessError::ChangedRedelivery,
        ) => "invariant_failure",
    };
    MetricsTelemetry.activity(activity_type, outcome);
}

fn process_activity_error(error: ProcessError) -> ActivityError {
    let (type_name, non_retryable) = match error {
        ProcessError::DependencyUnavailable => ("DependencyUnavailable", false),
        ProcessError::OwnerRejected { .. } => ("OwnerRejected", true),
        ProcessError::InvalidInput => ("InvalidInput", true),
        ProcessError::InvariantViolation
        | ProcessError::StaleEvent
        | ProcessError::ChangedRedelivery => ("InvariantViolation", true),
    };
    ActivityError::application(
        ApplicationFailure::builder(error.to_string())
            .type_name(type_name.to_owned())
            .non_retryable(non_retryable)
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use temporalio_sdk::testing::ActivityEnvironment;

    use super::*;
    use crate::{
        activities::{CallContext, ConfirmWinnerIntent, ForfeitCandidateIntent},
        workflows::AuctionId,
    };

    struct SequencedAuction {
        results: Mutex<Vec<Result<AuctionMutationOutcome, ProcessError>>>,
    }

    #[async_trait]
    impl AuctionActivities for SequencedAuction {
        async fn close_auction(
            &self,
            _: CloseAuctionIntent,
        ) -> Result<AuctionMutationOutcome, ProcessError> {
            self.results.lock().unwrap().remove(0)
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

    fn intent(key: &str) -> CloseAuctionIntent {
        CloseAuctionIntent {
            auction_id: AuctionId::parse("auction-1").unwrap(),
            observed_at_millis: 1_000,
            idempotency_key: key.to_owned(),
            call: CallContext {
                correlation_id: "correlation-1".to_owned(),
                causation_id: "event-1".to_owned(),
            },
        }
    }

    #[tokio::test]
    async fn local_temporal_activity_environment_preserves_retry_intent_and_rejection_outcome() {
        let auction = Arc::new(SequencedAuction {
            results: Mutex::new(vec![
                Err(ProcessError::DependencyUnavailable),
                Ok(AuctionMutationOutcome { version: 7 }),
                Err(ProcessError::OwnerRejected {
                    code: "too_early".to_owned(),
                }),
            ]),
        });
        let environment = ActivityEnvironment::builder()
            .register_activities(OwnerTemporalActivities::new(auction))
            .build();

        let stable = intent("auction-lifecycle/auction-1:close");
        let transient = environment
            .run(OwnerTemporalActivities::close_auction, stable.clone())
            .await;
        assert!(transient.is_err());
        let committed = environment
            .run(OwnerTemporalActivities::close_auction, stable)
            .await
            .unwrap();
        assert_eq!(
            committed,
            CloseAuctionActivityOutcome::Closed(AuctionMutationOutcome { version: 7 })
        );

        let rejection = environment
            .run(OwnerTemporalActivities::close_auction, intent("reject"))
            .await
            .unwrap();
        assert_eq!(
            rejection,
            CloseAuctionActivityOutcome::Rejected {
                code: "too_early".to_owned()
            }
        );
    }
}

#[activities]
impl OwnerTemporalActivities {
    #[activity]
    pub async fn close_auction(
        self: Arc<Self>,
        _context: ActivityContext,
        intent: CloseAuctionIntent,
    ) -> Result<CloseAuctionActivityOutcome, ActivityError> {
        let result = self.auction.close_auction(intent).await;
        record_activity_result("close_auction", &result);
        match result {
            Ok(outcome) => Ok(CloseAuctionActivityOutcome::Closed(outcome)),
            Err(ProcessError::OwnerRejected { code }) => {
                Ok(CloseAuctionActivityOutcome::Rejected { code })
            }
            Err(ProcessError::DependencyUnavailable) => Err(ActivityError::application(
                ApplicationFailure::builder("owner dependency unavailable")
                    .type_name("DependencyUnavailable".to_owned())
                    .build(),
            )),
            Err(error) => Err(ActivityError::application(
                ApplicationFailure::builder(error.to_string())
                    .type_name(match error {
                        ProcessError::InvalidInput => "InvalidInput".to_owned(),
                        ProcessError::InvariantViolation
                        | ProcessError::StaleEvent
                        | ProcessError::ChangedRedelivery => "InvariantViolation".to_owned(),
                        ProcessError::OwnerRejected { .. }
                        | ProcessError::DependencyUnavailable => unreachable!(),
                    })
                    .non_retryable(true)
                    .build(),
            )),
        }
    }
}
