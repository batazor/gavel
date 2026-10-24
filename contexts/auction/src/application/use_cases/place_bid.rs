use std::sync::Arc;

use serde::Serialize;

use crate::domain::{AuctionCommand, BidId, BidderId, Money};

use super::super::{
    ApplicationError, AuctionProjection, AuctionUnitOfWork, AuctionView, Authorization,
    AuthorizationDecision, CommandContext, CommandExecutor, CommandResult, MoneyView,
    PaymentReadiness, PermissionCheck, PlaceBidInput, PlaceBidResult, PreparedCommand,
    PrincipalKind, PublicAuctionStatus, ReconciliationOperation,
};

pub struct PlaceBid<U, A, P> {
    executor: CommandExecutor<U>,
    authorization: Arc<A>,
    payment_readiness: Arc<P>,
}

impl<U, A, P> PlaceBid<U, A, P>
where
    U: AuctionUnitOfWork,
    A: Authorization,
    P: PaymentReadiness,
{
    pub fn new(
        unit_of_work: Arc<U>,
        authorization: Arc<A>,
        payment_readiness: Arc<P>,
        retry_attempts: usize,
        fingerprint_key: [u8; 32],
    ) -> Self {
        Self {
            executor: CommandExecutor::new(unit_of_work, retry_attempts, fingerprint_key),
            authorization,
            payment_readiness,
        }
    }

    pub async fn handle(
        &self,
        context: &CommandContext,
        input: PlaceBidInput,
    ) -> Result<PlaceBidResult, ApplicationError> {
        let payload = canonical_payload(&input)?;
        let principal = context.principal.clone();
        let bidder_id = principal.id.clone();
        let auction_id = input.auction_id.clone();
        let authorization_resource_id = auction_id.clone();
        let authorization = Arc::clone(&self.authorization);
        let payment_readiness = Arc::clone(&self.payment_readiness);

        let result = self
            .executor
            .execute(
                context,
                "place_bid",
                &payload,
                &auction_id,
                move || async move {
                    if principal.kind != PrincipalKind::User {
                        return Err(ApplicationError::Forbidden);
                    }
                    match authorization
                        .check(PermissionCheck {
                            principal: principal.clone(),
                            resource_type: "auction".to_owned(),
                            resource_id: authorization_resource_id,
                            permission: "place_bid".to_owned(),
                        })
                        .await
                    {
                        Ok(AuthorizationDecision::Allowed) => {}
                        Ok(AuthorizationDecision::Denied) => {
                            return Err(ApplicationError::Forbidden);
                        }
                        Err(_) => {
                            return Err(ApplicationError::DependencyUnavailable {
                                dependency: "authz",
                            });
                        }
                    }
                    match payment_readiness.is_ready(&bidder_id).await {
                        Ok(true) => Ok(()),
                        Ok(false) => Err(ApplicationError::PaymentMethodRequired),
                        Err(_) => Err(ApplicationError::DependencyUnavailable {
                            dependency: "payments_projection",
                        }),
                    }
                },
                |aggregate| prepare_bid(aggregate, context, &input),
            )
            .await?;

        match result {
            CommandResult::PlaceBid(result) => Ok(result),
            _ => Err(ApplicationError::Internal),
        }
    }
}

fn prepare_bid(
    aggregate: &crate::domain::Auction,
    context: &CommandContext,
    input: &PlaceBidInput,
) -> Result<PreparedCommand, ApplicationError> {
    let maximum = Money::new(input.maximum_minor, input.currency_code.clone()).map_err(|_| {
        ApplicationError::InvalidArgument {
            reason: "invalid_currency",
            safe_message: "currency must be a three-letter uppercase code".to_owned(),
        }
    })?;
    let bidder_id = BidderId::from(context.principal.id.clone());
    let decision = aggregate
        .decide(AuctionCommand::PlaceBid {
            bid_id: BidId::from(input.bid_id.clone()),
            bidder_id: bidder_id.clone(),
            maximum,
            placed_at: input.placed_at,
            payment_instrument_ready: true,
        })
        .map_err(ApplicationError::from)?;
    let mut updated = aggregate.clone();
    for event in decision.events.iter().cloned() {
        updated.apply(event).map_err(ApplicationError::from)?;
    }
    let current_price = updated.current_price().ok_or(ApplicationError::Internal)?;
    let lot_id = updated.lot_id().ok_or(ApplicationError::Internal)?;
    let ends_at = updated.ends_at().ok_or(ApplicationError::Internal)?;
    let view = AuctionView {
        auction_id: input.auction_id.clone(),
        lot_id: lot_id.to_string(),
        status: PublicAuctionStatus::from(updated.status()),
        current_price: MoneyView::from(current_price),
        ends_at_millis: ends_at.as_millis(),
        version: updated.version(),
    };
    let result = PlaceBidResult {
        accepted_version: updated.version(),
        outbid: updated.leader() != Some(&bidder_id),
        auction: view.clone(),
    };
    let reconciliation = vec![ReconciliationOperation {
        operation_id: format!("participant:{}:{}", input.auction_id, context.principal.id),
        resource_type: "auction".to_owned(),
        resource_id: input.auction_id.clone(),
        relation: "participant".to_owned(),
        subject_type: "user".to_owned(),
        subject_id: context.principal.id.clone(),
    }];

    Ok(PreparedCommand {
        private_events: decision.events,
        public_events: decision.public_events,
        result: CommandResult::PlaceBid(result),
        projection: Some(AuctionProjection {
            view,
            updated_at: input.placed_at,
        }),
        reconciliation,
    })
}

#[derive(Serialize)]
struct CanonicalPlaceBid<'a> {
    auction_id: &'a str,
    maximum_minor: u64,
    currency_code: &'a str,
}

fn canonical_payload(input: &PlaceBidInput) -> Result<Vec<u8>, ApplicationError> {
    serde_json::to_vec(&CanonicalPlaceBid {
        auction_id: &input.auction_id,
        maximum_minor: input.maximum_minor,
        currency_code: &input.currency_code,
    })
    .map_err(|_| ApplicationError::Internal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::{
            LotListed, OpenAuction, PortError, Principal,
            testing::{InMemoryAuctionStore, InMemoryPaymentReadiness, StubAuthorization},
        },
        domain::{ClosingPolicy, Timestamp},
    };

    async fn opened_store() -> Arc<InMemoryAuctionStore> {
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
        store
    }

    fn context(user: &str, key: &str) -> CommandContext {
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

    fn input(bid: &str, maximum: u64) -> PlaceBidInput {
        PlaceBidInput {
            auction_id: "auction-1".to_owned(),
            bid_id: bid.to_owned(),
            maximum_minor: maximum,
            currency_code: "USD".to_owned(),
            placed_at: Timestamp::from_millis(200),
        }
    }

    #[tokio::test]
    async fn accepted_bid_returns_safe_authoritative_result() {
        let store = opened_store().await;
        let authz = Arc::new(StubAuthorization::allowed());
        let readiness = Arc::new(InMemoryPaymentReadiness::default());
        readiness.set("user-1", true);
        let use_case = PlaceBid::new(Arc::clone(&store), authz, readiness, 3, [2; 32]);

        let result = use_case
            .handle(&context("user-1", "bid-key-1"), input("bid-1", 300))
            .await
            .unwrap();

        assert_eq!(result.accepted_version, 2);
        assert!(!result.outbid);
        assert_eq!(result.auction.current_price.minor_units, 100);
        let serialized = serde_json::to_string(&result).unwrap().to_ascii_lowercase();
        assert!(!serialized.contains("maximum"));
        assert!(!serialized.contains("user-1"));
        assert_eq!(store.outbox().len(), 2);
    }

    #[tokio::test]
    async fn lower_challenger_is_immediately_outbid() {
        let store = opened_store().await;
        let authz = Arc::new(StubAuthorization::allowed());
        let readiness = Arc::new(InMemoryPaymentReadiness::default());
        readiness.set("user-1", true);
        readiness.set("user-2", true);
        let use_case = PlaceBid::new(store, authz, readiness, 3, [2; 32]);
        use_case
            .handle(&context("user-1", "bid-key-1"), input("bid-1", 300))
            .await
            .unwrap();

        let result = use_case
            .handle(&context("user-2", "bid-key-2"), input("bid-2", 200))
            .await
            .unwrap();
        assert!(result.outbid);
        assert_eq!(result.auction.current_price.minor_units, 210);
    }

    #[tokio::test]
    async fn denied_or_unavailable_authz_does_not_change_stream() {
        for (auth_result, expected) in [
            (
                Ok(AuthorizationDecision::Denied),
                ApplicationError::Forbidden,
            ),
            (
                Err(PortError::Unavailable),
                ApplicationError::DependencyUnavailable {
                    dependency: "authz",
                },
            ),
        ] {
            let store = opened_store().await;
            let before = store.stream("auction-1");
            let authz = Arc::new(StubAuthorization::with_result(auth_result));
            let readiness = Arc::new(InMemoryPaymentReadiness::default());
            readiness.set("user-1", true);
            let use_case = PlaceBid::new(Arc::clone(&store), authz, readiness, 3, [2; 32]);

            assert_eq!(
                use_case
                    .handle(&context("user-1", "bid-key-1"), input("bid-1", 300))
                    .await,
                Err(expected)
            );
            assert_eq!(store.stream("auction-1"), before);
        }
    }

    #[tokio::test]
    async fn payment_readiness_is_required() {
        let store = opened_store().await;
        let use_case = PlaceBid::new(
            store,
            Arc::new(StubAuthorization::allowed()),
            Arc::new(InMemoryPaymentReadiness::default()),
            3,
            [2; 32],
        );

        assert_eq!(
            use_case
                .handle(&context("user-1", "bid-key-1"), input("bid-1", 300))
                .await,
            Err(ApplicationError::PaymentMethodRequired)
        );
    }
}
