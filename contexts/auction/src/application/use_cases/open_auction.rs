use std::sync::Arc;

use serde::Serialize;

use crate::domain::{
    AuctionCommand, AuctionId, ClosingPolicy, DurationMillis, LotId, Money, SellerId, Timestamp,
};

use super::super::{
    ApplicationError, AuctionProjection, AuctionUnitOfWork, AuctionView, CommandContext,
    CommandExecutor, CommandResult, MoneyView, PreparedCommand, Principal, PrincipalKind,
    PublicAuctionStatus, ReconciliationOperation,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LotListed {
    pub event_id: String,
    pub correlation_id: String,
    pub auction_id: String,
    pub lot_id: String,
    pub seller_id: String,
    pub start_price_minor: u64,
    pub currency_code: String,
    pub bid_step_minor: u64,
    pub closing_policy: ClosingPolicy,
    pub listed_at: Timestamp,
}

pub struct OpenAuction<U> {
    executor: CommandExecutor<U>,
}

impl<U> OpenAuction<U>
where
    U: AuctionUnitOfWork,
{
    pub fn new(unit_of_work: Arc<U>, retry_attempts: usize, fingerprint_key: [u8; 32]) -> Self {
        Self {
            executor: CommandExecutor::new(unit_of_work, retry_attempts, fingerprint_key),
        }
    }

    pub async fn handle(&self, event: LotListed) -> Result<u64, ApplicationError> {
        let context = CommandContext {
            principal: Principal {
                kind: PrincipalKind::ServiceAccount,
                id: "catalog".to_owned(),
            },
            idempotency_key: event.event_id.clone(),
            correlation_id: event.correlation_id.clone(),
            causation_id: event.event_id.clone(),
        };
        let payload = canonical_payload(&event)?;
        let result = self
            .executor
            .execute(
                &context,
                "open_auction_from_lot_listed",
                &payload,
                &event.auction_id,
                || async { Ok(()) },
                |_| prepare_open(&event),
            )
            .await?;

        match result {
            CommandResult::Opened { version } => Ok(version),
            _ => Err(ApplicationError::Internal),
        }
    }
}

fn prepare_open(event: &LotListed) -> Result<PreparedCommand, ApplicationError> {
    let start_price =
        Money::new(event.start_price_minor, event.currency_code.clone()).map_err(|_| {
            ApplicationError::InvalidArgument {
                reason: "invalid_currency",
                safe_message: "currency must be a three-letter uppercase code".to_owned(),
            }
        })?;
    let decision = crate::domain::Auction::default()
        .decide(AuctionCommand::Open {
            auction_id: AuctionId::from(event.auction_id.clone()),
            lot_id: LotId::from(event.lot_id.clone()),
            seller_id: SellerId::from(event.seller_id.clone()),
            start_price: start_price.clone(),
            bid_step_minor: event.bid_step_minor,
            closing_policy: event.closing_policy.clone(),
            opened_at: event.listed_at,
        })
        .map_err(ApplicationError::from)?;
    let ends_at = decision
        .public_events
        .iter()
        .find_map(|public| match public {
            crate::domain::PublicEvent::AuctionOpened { ends_at, .. } => Some(*ends_at),
            _ => None,
        })
        .ok_or(ApplicationError::Internal)?;
    let version = decision.resulting_version;
    let projection = AuctionProjection {
        view: AuctionView {
            auction_id: event.auction_id.clone(),
            lot_id: event.lot_id.clone(),
            status: PublicAuctionStatus::Open,
            current_price: MoneyView::from(&start_price),
            ends_at_millis: ends_at.as_millis(),
            version,
        },
        updated_at: event.listed_at,
    };
    let reconciliation = [
        ("platform", "platform", "gavel"),
        ("lot", "lot", event.lot_id.as_str()),
    ]
    .into_iter()
    .map(
        |(relation, subject_type, subject_id)| ReconciliationOperation {
            operation_id: format!("{}:{relation}", event.event_id),
            resource_type: "auction".to_owned(),
            resource_id: event.auction_id.clone(),
            relation: relation.to_owned(),
            subject_type: subject_type.to_owned(),
            subject_id: subject_id.to_owned(),
        },
    )
    .collect();

    Ok(PreparedCommand {
        private_events: decision.events,
        public_events: decision.public_events,
        result: CommandResult::Opened { version },
        projection: Some(projection),
        reconciliation,
    })
}

#[derive(Serialize)]
struct CanonicalLotListed<'a> {
    auction_id: &'a str,
    lot_id: &'a str,
    seller_id: &'a str,
    start_price_minor: u64,
    currency_code: &'a str,
    bid_step_minor: u64,
    scheduled_end_millis: Option<u64>,
    inactivity_millis: Option<u64>,
    hard_deadline_millis: Option<u64>,
    listed_at_millis: u64,
}

fn canonical_payload(event: &LotListed) -> Result<Vec<u8>, ApplicationError> {
    serde_json::to_vec(&CanonicalLotListed {
        auction_id: &event.auction_id,
        lot_id: &event.lot_id,
        seller_id: &event.seller_id,
        start_price_minor: event.start_price_minor,
        currency_code: &event.currency_code,
        bid_step_minor: event.bid_step_minor,
        scheduled_end_millis: event
            .closing_policy
            .scheduled_end()
            .map(Timestamp::as_millis),
        inactivity_millis: event
            .closing_policy
            .inactivity()
            .map(DurationMillis::as_millis),
        hard_deadline_millis: event
            .closing_policy
            .hard_deadline()
            .map(Timestamp::as_millis),
        listed_at_millis: event.listed_at.as_millis(),
    })
    .map_err(|_| ApplicationError::Internal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{
        AuctionProjectionReader, ReconciliationQueue, testing::InMemoryAuctionStore,
    };

    fn event() -> LotListed {
        LotListed {
            event_id: "catalog-event-1".to_owned(),
            correlation_id: "correlation-1".to_owned(),
            auction_id: "auction-1".to_owned(),
            lot_id: "lot-1".to_owned(),
            seller_id: "seller-1".to_owned(),
            start_price_minor: 100,
            currency_code: "USD".to_owned(),
            bid_step_minor: 10,
            closing_policy: ClosingPolicy::new(
                Some(Timestamp::from_millis(1_000)),
                DurationMillis::new(100),
                Some(Timestamp::from_millis(2_000)),
            )
            .unwrap(),
            listed_at: Timestamp::from_millis(100),
        }
    }

    #[tokio::test]
    async fn first_delivery_opens_and_exact_duplicate_replays() {
        let store = Arc::new(InMemoryAuctionStore::default());
        let use_case = OpenAuction::new(Arc::clone(&store), 3, [9; 32]);

        assert_eq!(use_case.handle(event()).await.unwrap(), 1);
        assert_eq!(use_case.handle(event()).await.unwrap(), 1);
        assert_eq!(store.stream("auction-1").len(), 1);
        assert_eq!(store.outbox().len(), 1);
        assert_eq!(
            store
                .auction("auction-1")
                .await
                .unwrap()
                .unwrap()
                .view
                .version,
            1
        );
        assert_eq!(store.pending(10).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn same_event_id_with_changed_payload_conflicts() {
        let store = Arc::new(InMemoryAuctionStore::default());
        let use_case = OpenAuction::new(store, 3, [9; 32]);
        use_case.handle(event()).await.unwrap();
        let mut changed = event();
        changed.start_price_minor = 200;

        assert_eq!(
            use_case.handle(changed).await,
            Err(ApplicationError::IdempotencyConflict)
        );
    }
}
