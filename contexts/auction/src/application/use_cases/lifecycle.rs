use std::sync::Arc;

use serde::Serialize;

use crate::domain::{AuctionCommand, BidderId, PaymentId, Timestamp};

use super::super::{
    ApplicationError, AuctionProjection, AuctionUnitOfWork, AuctionView, Authorization,
    AuthorizationDecision, CancelAuctionInput, CloseAuctionInput, CommandContext, CommandExecutor,
    CommandResult, ConfirmWinnerInput, ForfeitSettlementCandidateInput, LifecycleResult, MoneyView,
    PermissionCheck, PreparedCommand, Principal, PrincipalKind, PublicAuctionStatus,
};

pub struct AuctionLifecycle<U, A> {
    executor: CommandExecutor<U>,
    authorization: Arc<A>,
}

impl<U, A> AuctionLifecycle<U, A>
where
    U: AuctionUnitOfWork,
    A: Authorization,
{
    pub fn new(
        unit_of_work: Arc<U>,
        authorization: Arc<A>,
        retry_attempts: usize,
        fingerprint_key: [u8; 32],
    ) -> Self {
        Self {
            executor: CommandExecutor::new(unit_of_work, retry_attempts, fingerprint_key),
            authorization,
        }
    }

    pub async fn close(
        &self,
        context: &CommandContext,
        input: CloseAuctionInput,
    ) -> Result<LifecycleResult, ApplicationError> {
        let payload = canonical(&(input.auction_id.as_str(), input.now.as_millis()))?;
        self.run(
            context,
            "close_auction",
            "close",
            &input.auction_id,
            payload,
            |aggregate| {
                let decision = aggregate
                    .decide(AuctionCommand::Close { now: input.now })
                    .map_err(ApplicationError::from)?;
                prepare_lifecycle(aggregate, decision, input.now, |updated| {
                    Ok(LifecycleResult::Closed {
                        status: PublicAuctionStatus::from(updated.status()),
                        closing_price: MoneyView::from(
                            updated.closing_price().ok_or(ApplicationError::Internal)?,
                        ),
                        version: updated.version(),
                    })
                })
            },
        )
        .await
    }

    pub async fn cancel(
        &self,
        context: &CommandContext,
        input: CancelAuctionInput,
    ) -> Result<LifecycleResult, ApplicationError> {
        let payload = canonical(&(input.auction_id.as_str(),))?;
        self.run(
            context,
            "cancel_auction",
            "cancel",
            &input.auction_id,
            payload,
            |aggregate| {
                let decision = aggregate
                    .decide(AuctionCommand::Cancel)
                    .map_err(ApplicationError::from)?;
                prepare_lifecycle(aggregate, decision, input.occurred_at, |updated| {
                    Ok(LifecycleResult::Cancelled {
                        version: updated.version(),
                    })
                })
            },
        )
        .await
    }

    pub async fn forfeit(
        &self,
        context: &CommandContext,
        input: ForfeitSettlementCandidateInput,
    ) -> Result<LifecycleResult, ApplicationError> {
        let payload = canonical(&(
            input.auction_id.as_str(),
            input.candidate_id.as_str(),
            input.settlement_round,
        ))?;
        self.run(
            context,
            "forfeit_settlement_candidate",
            "settle",
            &input.auction_id,
            payload,
            |aggregate| {
                let decision = aggregate
                    .decide(AuctionCommand::ForfeitSettlementCandidate {
                        bidder_id: BidderId::from(input.candidate_id.clone()),
                        round: input.settlement_round,
                    })
                    .map_err(ApplicationError::from)?;
                prepare_lifecycle(aggregate, decision, input.occurred_at, |updated| {
                    Ok(LifecycleResult::SettlementAdvanced {
                        status: PublicAuctionStatus::from(updated.status()),
                        settlement_round: updated
                            .settlement_candidate()
                            .map_or(input.settlement_round, |(_, _, round)| round),
                        version: updated.version(),
                    })
                })
            },
        )
        .await
    }

    pub async fn confirm(
        &self,
        context: &CommandContext,
        input: ConfirmWinnerInput,
    ) -> Result<LifecycleResult, ApplicationError> {
        let payload = canonical(&(
            input.auction_id.as_str(),
            input.candidate_id.as_str(),
            input.payment_id.as_str(),
        ))?;
        self.run(
            context,
            "confirm_winner",
            "settle",
            &input.auction_id,
            payload,
            |aggregate| {
                let decision = aggregate
                    .decide(AuctionCommand::ConfirmWinner {
                        bidder_id: BidderId::from(input.candidate_id.clone()),
                        payment_id: PaymentId::from(input.payment_id.clone()),
                    })
                    .map_err(ApplicationError::from)?;
                prepare_lifecycle(aggregate, decision, input.occurred_at, |updated| {
                    Ok(LifecycleResult::WinnerConfirmed {
                        version: updated.version(),
                    })
                })
            },
        )
        .await
    }

    async fn run<Decide>(
        &self,
        context: &CommandContext,
        operation: &'static str,
        permission: &'static str,
        auction_id: &str,
        payload: Vec<u8>,
        decide: Decide,
    ) -> Result<LifecycleResult, ApplicationError>
    where
        Decide: FnMut(&crate::domain::Auction) -> Result<PreparedCommand, ApplicationError>,
    {
        let authorization = Arc::clone(&self.authorization);
        let principal = context.principal.clone();
        let resource_id = auction_id.to_owned();
        let result = self
            .executor
            .execute(
                context,
                operation,
                &payload,
                auction_id,
                move || authorize(authorization, principal, resource_id, permission),
                decide,
            )
            .await?;
        match result {
            CommandResult::Lifecycle(result) => Ok(result),
            _ => Err(ApplicationError::Internal),
        }
    }
}

async fn authorize<A: Authorization>(
    authorization: Arc<A>,
    principal: Principal,
    resource_id: String,
    permission: &'static str,
) -> Result<(), ApplicationError> {
    if principal.kind != PrincipalKind::ServiceAccount {
        return Err(ApplicationError::Forbidden);
    }
    match authorization
        .check(PermissionCheck {
            principal,
            resource_type: "auction".to_owned(),
            resource_id,
            permission: permission.to_owned(),
        })
        .await
    {
        Ok(AuthorizationDecision::Allowed) => Ok(()),
        Ok(AuthorizationDecision::Denied) => Err(ApplicationError::Forbidden),
        Err(_) => Err(ApplicationError::DependencyUnavailable {
            dependency: "authz",
        }),
    }
}

fn prepare_lifecycle<ResultFactory>(
    aggregate: &crate::domain::Auction,
    decision: crate::domain::Decision,
    updated_at: Timestamp,
    result_factory: ResultFactory,
) -> Result<PreparedCommand, ApplicationError>
where
    ResultFactory: FnOnce(&crate::domain::Auction) -> Result<LifecycleResult, ApplicationError>,
{
    let mut updated = aggregate.clone();
    for event in decision.events.iter().cloned() {
        updated.apply(event).map_err(ApplicationError::from)?;
    }
    let result = result_factory(&updated)?;
    let view = AuctionView {
        auction_id: updated.id().ok_or(ApplicationError::Internal)?.to_string(),
        lot_id: updated
            .lot_id()
            .ok_or(ApplicationError::Internal)?
            .to_string(),
        status: PublicAuctionStatus::from(updated.status()),
        current_price: MoneyView::from(updated.current_price().ok_or(ApplicationError::Internal)?),
        ends_at_millis: updated
            .ends_at()
            .ok_or(ApplicationError::Internal)?
            .as_millis(),
        version: updated.version(),
    };

    Ok(PreparedCommand {
        private_events: decision.events,
        public_events: decision.public_events,
        result: CommandResult::Lifecycle(result),
        projection: Some(AuctionProjection { view, updated_at }),
        reconciliation: Vec::new(),
    })
}

fn canonical(value: &impl Serialize) -> Result<Vec<u8>, ApplicationError> {
    serde_json::to_vec(value).map_err(|_| ApplicationError::Internal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::{
            LotListed, OpenAuction, PlaceBid, PlaceBidInput,
            testing::{InMemoryAuctionStore, InMemoryPaymentReadiness, StubAuthorization},
        },
        domain::ClosingPolicy,
    };

    fn process_context(key: &str) -> CommandContext {
        CommandContext {
            principal: Principal {
                kind: PrincipalKind::ServiceAccount,
                id: "processes".to_owned(),
            },
            idempotency_key: key.to_owned(),
            correlation_id: "correlation-1".to_owned(),
            causation_id: "workflow-1".to_owned(),
        }
    }

    fn user_context(user: &str, key: &str) -> CommandContext {
        CommandContext {
            principal: Principal {
                kind: PrincipalKind::User,
                id: user.to_owned(),
            },
            idempotency_key: key.to_owned(),
            correlation_id: "correlation-1".to_owned(),
            causation_id: "graphql-1".to_owned(),
        }
    }

    async fn auction_with_bids() -> Arc<InMemoryAuctionStore> {
        let store = Arc::new(InMemoryAuctionStore::default());
        OpenAuction::new(Arc::clone(&store), 3, [1; 32])
            .handle(LotListed {
                event_id: "listed-1".to_owned(),
                correlation_id: "correlation-1".to_owned(),
                auction_id: "auction-1".to_owned(),
                lot_id: "lot-1".to_owned(),
                seller_id: "seller-1".to_owned(),
                start_price_minor: 100,
                currency_code: "USD".to_owned(),
                bid_step_minor: 10,
                closing_policy: ClosingPolicy::new(Some(Timestamp::from_millis(1_000)), None, None)
                    .unwrap(),
                listed_at: Timestamp::from_millis(100),
            })
            .await
            .unwrap();
        let readiness = Arc::new(InMemoryPaymentReadiness::default());
        readiness.set("user-1", true);
        readiness.set("user-2", true);
        let bidding = PlaceBid::new(
            Arc::clone(&store),
            Arc::new(StubAuthorization::allowed()),
            readiness,
            3,
            [2; 32],
        );
        for (user, key, bid, maximum) in [
            ("user-1", "key-1", "bid-1", 300),
            ("user-2", "key-2", "bid-2", 200),
        ] {
            bidding
                .handle(
                    &user_context(user, key),
                    PlaceBidInput {
                        auction_id: "auction-1".to_owned(),
                        bid_id: bid.to_owned(),
                        maximum_minor: maximum,
                        currency_code: "USD".to_owned(),
                        placed_at: Timestamp::from_millis(200),
                    },
                )
                .await
                .unwrap();
        }
        store
    }

    #[tokio::test]
    async fn close_has_typed_too_early_and_idempotent_terminal_result() {
        let store = auction_with_bids().await;
        let lifecycle = AuctionLifecycle::new(
            Arc::clone(&store),
            Arc::new(StubAuthorization::allowed()),
            3,
            [4; 32],
        );
        let early = lifecycle
            .close(
                &process_context("close-early"),
                CloseAuctionInput {
                    auction_id: "auction-1".to_owned(),
                    now: Timestamp::from_millis(900),
                },
            )
            .await
            .unwrap_err();
        assert_eq!(early.reason(), "too_early");

        let input = CloseAuctionInput {
            auction_id: "auction-1".to_owned(),
            now: Timestamp::from_millis(1_000),
        };
        let first = lifecycle
            .close(&process_context("close-1"), input.clone())
            .await
            .unwrap();
        let event_count = store.stream("auction-1").len();
        let replay = lifecycle
            .close(&process_context("close-1"), input)
            .await
            .unwrap();
        assert_eq!(first, replay);
        assert_eq!(store.stream("auction-1").len(), event_count);
    }

    #[tokio::test]
    async fn stale_forfeiture_is_rejected_and_current_candidate_advances() {
        let store = auction_with_bids().await;
        let lifecycle = AuctionLifecycle::new(
            Arc::clone(&store),
            Arc::new(StubAuthorization::allowed()),
            3,
            [4; 32],
        );
        lifecycle
            .close(
                &process_context("close-1"),
                CloseAuctionInput {
                    auction_id: "auction-1".to_owned(),
                    now: Timestamp::from_millis(1_000),
                },
            )
            .await
            .unwrap();
        let stale = lifecycle
            .forfeit(
                &process_context("forfeit-stale"),
                ForfeitSettlementCandidateInput {
                    auction_id: "auction-1".to_owned(),
                    candidate_id: "user-2".to_owned(),
                    settlement_round: 1,
                    occurred_at: Timestamp::from_millis(1_100),
                },
            )
            .await
            .unwrap_err();
        assert_eq!(stale.reason(), "settlement_candidate_mismatch");

        let advanced = lifecycle
            .forfeit(
                &process_context("forfeit-1"),
                ForfeitSettlementCandidateInput {
                    auction_id: "auction-1".to_owned(),
                    candidate_id: "user-1".to_owned(),
                    settlement_round: 1,
                    occurred_at: Timestamp::from_millis(1_100),
                },
            )
            .await
            .unwrap();
        assert!(matches!(
            advanced,
            LifecycleResult::SettlementAdvanced {
                settlement_round: 2,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn confirm_result_and_public_events_do_not_expose_payment_object() {
        let store = auction_with_bids().await;
        let lifecycle = AuctionLifecycle::new(
            Arc::clone(&store),
            Arc::new(StubAuthorization::allowed()),
            3,
            [4; 32],
        );
        lifecycle
            .close(
                &process_context("close-1"),
                CloseAuctionInput {
                    auction_id: "auction-1".to_owned(),
                    now: Timestamp::from_millis(1_000),
                },
            )
            .await
            .unwrap();
        let result = lifecycle
            .confirm(
                &process_context("confirm-1"),
                ConfirmWinnerInput {
                    auction_id: "auction-1".to_owned(),
                    candidate_id: "user-1".to_owned(),
                    payment_id: "payment-provider-secret".to_owned(),
                    occurred_at: Timestamp::from_millis(1_100),
                },
            )
            .await
            .unwrap();
        let serialized = serde_json::to_string(&result).unwrap().to_ascii_lowercase();
        assert!(!serialized.contains("payment"));
        assert!(matches!(result, LifecycleResult::WinnerConfirmed { .. }));
        assert!(store.outbox().iter().all(|event| {
            let debug = format!("{event:?}").to_ascii_lowercase();
            !debug.contains("payment-provider-secret")
        }));
    }

    #[tokio::test]
    async fn cancel_is_typed_and_idempotent() {
        let store = auction_with_bids().await;
        let lifecycle = AuctionLifecycle::new(
            Arc::clone(&store),
            Arc::new(StubAuthorization::allowed()),
            3,
            [4; 32],
        );
        let input = CancelAuctionInput {
            auction_id: "auction-1".to_owned(),
            occurred_at: Timestamp::from_millis(500),
        };
        let first = lifecycle
            .cancel(&process_context("cancel-1"), input.clone())
            .await
            .unwrap();
        let replay = lifecycle
            .cancel(&process_context("cancel-1"), input)
            .await
            .unwrap();
        assert_eq!(first, replay);
        assert!(matches!(first, LifecycleResult::Cancelled { .. }));
    }
}
