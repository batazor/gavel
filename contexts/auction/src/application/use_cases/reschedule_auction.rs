use std::sync::Arc;

use serde::Serialize;

use crate::domain::{AuctionCommand, SellerId};

use super::super::{
    ApplicationError, AuctionProjection, AuctionUnitOfWork, AuctionView, Authorization,
    AuthorizationDecision, CommandContext, CommandExecutor, CommandResult, MoneyView,
    PermissionCheck, PreparedCommand, PrincipalKind, PublicAuctionStatus, RescheduleAuctionInput,
};

pub struct RescheduleAuction<U, A> {
    executor: CommandExecutor<U>,
    authorization: Arc<A>,
}

impl<U, A> RescheduleAuction<U, A>
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

    pub async fn handle(
        &self,
        context: &CommandContext,
        input: RescheduleAuctionInput,
    ) -> Result<AuctionView, ApplicationError> {
        let payload = canonical_payload(&input)?;
        let principal = context.principal.clone();
        let resource_id = input.auction_id.clone();
        let authorization = Arc::clone(&self.authorization);
        let result = self
            .executor
            .execute(
                context,
                "reschedule_auction",
                &payload,
                &input.auction_id,
                move || async move {
                    if principal.kind != PrincipalKind::User {
                        return Err(ApplicationError::Forbidden);
                    }
                    match authorization
                        .check(PermissionCheck {
                            principal,
                            resource_type: "auction".to_owned(),
                            resource_id,
                            permission: "reschedule".to_owned(),
                        })
                        .await
                    {
                        Ok(AuthorizationDecision::Allowed) => Ok(()),
                        Ok(AuthorizationDecision::Denied) => Err(ApplicationError::Forbidden),
                        Err(_) => Err(ApplicationError::DependencyUnavailable {
                            dependency: "authz",
                        }),
                    }
                },
                |aggregate| prepare_reschedule(aggregate, context, &input),
            )
            .await?;

        match result {
            CommandResult::Auction(view) => Ok(view),
            _ => Err(ApplicationError::Internal),
        }
    }
}

fn prepare_reschedule(
    aggregate: &crate::domain::Auction,
    context: &CommandContext,
    input: &RescheduleAuctionInput,
) -> Result<PreparedCommand, ApplicationError> {
    let decision = aggregate
        .decide(AuctionCommand::Reschedule {
            seller_id: SellerId::from(context.principal.id.clone()),
            scheduled_end: input.scheduled_end,
            now: input.now,
        })
        .map_err(ApplicationError::from)?;
    let mut updated = aggregate.clone();
    for event in decision.events.iter().cloned() {
        updated.apply(event).map_err(ApplicationError::from)?;
    }
    let view = AuctionView {
        auction_id: input.auction_id.clone(),
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
        result: CommandResult::Auction(view.clone()),
        projection: Some(AuctionProjection {
            view,
            updated_at: input.now,
        }),
        reconciliation: Vec::new(),
    })
}

#[derive(Serialize)]
struct CanonicalReschedule<'a> {
    auction_id: &'a str,
    scheduled_end_millis: u64,
    now_millis: u64,
}

fn canonical_payload(input: &RescheduleAuctionInput) -> Result<Vec<u8>, ApplicationError> {
    serde_json::to_vec(&CanonicalReschedule {
        auction_id: &input.auction_id,
        scheduled_end_millis: input.scheduled_end.as_millis(),
        now_millis: input.now.as_millis(),
    })
    .map_err(|_| ApplicationError::Internal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::{
            AuctionUnitOfWork, CommitRequest, LotListed, OpenAuction, Principal,
            testing::{InMemoryAuctionStore, StubAuthorization},
        },
        domain::{Auction, ClosingPolicy, Timestamp},
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
                closing_policy: ClosingPolicy::new(
                    Some(Timestamp::from_millis(1_000)),
                    None,
                    Some(Timestamp::from_millis(2_000)),
                )
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

    fn input(end: u64, now: u64) -> RescheduleAuctionInput {
        RescheduleAuctionInput {
            auction_id: "auction-1".to_owned(),
            scheduled_end: crate::domain::Timestamp::from_millis(end),
            now: crate::domain::Timestamp::from_millis(now),
        }
    }

    #[tokio::test]
    async fn seller_reschedules_and_replay_has_no_second_event() {
        let store = opened_store().await;
        let use_case = RescheduleAuction::new(
            Arc::clone(&store),
            Arc::new(StubAuthorization::allowed()),
            3,
            [3; 32],
        );

        let first = use_case
            .handle(&context("seller-1", "key-1"), input(1_500, 200))
            .await
            .unwrap();
        let replay = use_case
            .handle(&context("seller-1", "key-1"), input(1_500, 200))
            .await
            .unwrap();
        assert_eq!(first, replay);
        assert_eq!(first.ends_at_millis, 1_500);
        assert_eq!(store.stream("auction-1").len(), 2);
        assert_eq!(store.outbox().len(), 2);
    }

    #[tokio::test]
    async fn permission_and_seller_are_both_enforced() {
        let store = opened_store().await;
        let denied = RescheduleAuction::new(
            Arc::clone(&store),
            Arc::new(StubAuthorization::with_result(Ok(
                AuthorizationDecision::Denied,
            ))),
            3,
            [3; 32],
        );
        assert_eq!(
            denied
                .handle(&context("seller-1", "key-1"), input(1_500, 200))
                .await,
            Err(ApplicationError::Forbidden)
        );

        let wrong_seller =
            RescheduleAuction::new(store, Arc::new(StubAuthorization::allowed()), 3, [3; 32]);
        assert_eq!(
            wrong_seller
                .handle(&context("seller-2", "key-2"), input(1_500, 200))
                .await,
            Err(ApplicationError::Forbidden)
        );
    }

    #[tokio::test]
    async fn past_deadline_and_closed_state_are_rejected() {
        let store = opened_store().await;
        let use_case = RescheduleAuction::new(
            Arc::clone(&store),
            Arc::new(StubAuthorization::allowed()),
            3,
            [3; 32],
        );
        let past = use_case
            .handle(&context("seller-1", "key-1"), input(200, 200))
            .await
            .unwrap_err();
        assert_eq!(past.reason(), "deadline_not_future");

        let loaded = store.load("auction-1").await.unwrap().unwrap();
        let aggregate = Auction::rehydrate(loaded.events).unwrap();
        let close = aggregate
            .decide(AuctionCommand::Close {
                now: Timestamp::from_millis(1_000),
            })
            .unwrap();
        store
            .commit(CommitRequest {
                auction_id: "auction-1".to_owned(),
                expected_version: loaded.version,
                private_events: close.events,
                public_events: close.public_events,
                command: None,
                projection: None,
                reconciliation: Vec::new(),
            })
            .await
            .unwrap();
        let closed = use_case
            .handle(&context("seller-1", "key-2"), input(1_500, 1_100))
            .await
            .unwrap_err();
        assert_eq!(closed.reason(), "not_open");
    }
}
