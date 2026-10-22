use std::sync::{Arc, Mutex};

use async_graphql::{Request, Variables, value};
use async_trait::async_trait;
use gavel_auction::{
    Timestamp,
    adapters::inbound::graphql::{AuctionGraphqlApi, VerifiedRequestContext, schema},
    application::{
        ApplicationError, AuctionView, CommandContext, MoneyView, PlaceBidInput, PlaceBidResult,
        Principal, PrincipalKind, PublicAuctionStatus, RescheduleAuctionInput,
    },
};

struct MockApi {
    error: Option<ApplicationError>,
    placed_maxima: Mutex<Vec<u64>>,
}

impl MockApi {
    fn success() -> Self {
        Self {
            error: None,
            placed_maxima: Mutex::new(Vec::new()),
        }
    }

    fn failing(error: ApplicationError) -> Self {
        Self {
            error: Some(error),
            placed_maxima: Mutex::new(Vec::new()),
        }
    }
}

fn view() -> AuctionView {
    AuctionView {
        auction_id: "auction-1".to_owned(),
        lot_id: "lot-1".to_owned(),
        status: PublicAuctionStatus::Open,
        current_price: MoneyView {
            minor_units: 100,
            currency_code: "USD".to_owned(),
        },
        ends_at_millis: 1_700_000_060_000,
        version: 7,
    }
}

#[async_trait]
impl AuctionGraphqlApi for MockApi {
    async fn auction(&self, _auction_id: &str) -> Result<Option<AuctionView>, ApplicationError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        Ok(Some(view()))
    }

    async fn place_bid(
        &self,
        _context: &CommandContext,
        input: PlaceBidInput,
    ) -> Result<PlaceBidResult, ApplicationError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        self.placed_maxima.lock().unwrap().push(input.maximum_minor);
        Ok(PlaceBidResult {
            auction: view(),
            accepted_version: 7,
            outbid: false,
        })
    }

    async fn reschedule_auction(
        &self,
        _context: &CommandContext,
        _input: RescheduleAuctionInput,
    ) -> Result<AuctionView, ApplicationError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        Ok(view())
    }
}

fn verified() -> VerifiedRequestContext {
    VerifiedRequestContext {
        principal: Principal {
            kind: PrincipalKind::User,
            id: "user-1".to_owned(),
        },
        correlation_id: "correlation-1".to_owned(),
        causation_id: "request-1".to_owned(),
        now: Timestamp::from_millis(1_700_000_000_000),
    }
}

#[tokio::test]
async fn query_operations_return_public_projection_shape() {
    let api = Arc::new(MockApi::success());
    let response = schema(api).execute(
        "{ auction(id: \"auction-1\") { id lotId status currentPrice { amount currency version } endsAt viewer { canBid } } currentPrice(auctionId: \"auction-1\") { amount currency version } }"
    ).await;
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    assert_eq!(
        response.data,
        value!({
            "auction": {
                "id": "auction-1", "lotId": "lot-1", "status": "OPEN",
                "currentPrice": {"amount": "100", "currency": "USD", "version": 7},
                "endsAt": "2023-11-14T22:14:20+00:00", "viewer": null,
            },
            "currentPrice": {"amount": "100", "currency": "USD", "version": 7},
        })
    );
}

#[tokio::test]
async fn mutation_uses_verified_context_and_never_echoes_maximum() {
    let api = Arc::new(MockApi::success());
    let request = Request::new(
        "mutation Bid($input: PlaceBidInput!, $key: String!) { placeBid(input: $input, idempotencyKey: $key) { acceptedVersion outbid auction { id currentPrice { amount } } } }",
    )
    .variables(Variables::from_value(value!({
        "input": {"auctionId": "auction-1", "maximumAmount": "9000", "currency": "USD"},
        "key": "bid-command-1",
    })))
    .data(verified());
    let response = schema(Arc::clone(&api) as Arc<dyn AuctionGraphqlApi>)
        .execute(request)
        .await;
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    assert_eq!(*api.placed_maxima.lock().unwrap(), vec![9_000]);
    assert!(!serde_json::to_string(&response).unwrap().contains("9000"));
}

#[tokio::test]
async fn errors_have_stable_extensions_and_auth_is_required() {
    let unauthenticated = schema(Arc::new(MockApi::success()))
        .execute(
            "mutation { placeBid(input: {auctionId: \"auction-1\", maximumAmount: \"200\", currency: \"USD\"}, idempotencyKey: \"key-1\") { acceptedVersion } }",
        )
        .await;
    assert_eq!(
        unauthenticated.errors[0]
            .extensions
            .as_ref()
            .unwrap()
            .get("code"),
        Some(&value!("UNAUTHENTICATED"))
    );

    let forbidden = schema(Arc::new(MockApi::failing(ApplicationError::Forbidden)))
        .execute(
            Request::new(
                "mutation { rescheduleAuction(input: {auctionId: \"auction-1\", scheduledEnd: \"2023-11-14T23:00:00Z\"}, idempotencyKey: \"key-2\") { id } }",
            )
            .data(verified()),
        )
        .await;
    let extensions = forbidden.errors[0].extensions.as_ref().unwrap();
    assert_eq!(extensions.get("code"), Some(&value!("FORBIDDEN")));
    assert_eq!(extensions.get("retryable"), Some(&value!(false)));
}

#[test]
fn executable_schema_contains_the_owned_operations_only() {
    let sdl = schema(Arc::new(MockApi::success())).sdl();
    for field in ["auction", "currentPrice", "placeBid", "rescheduleAuction"] {
        assert!(sdl.contains(field), "missing {field} in schema:\n{sdl}");
    }
    for secret in ["maximumMinor", "bidderId", "paymentId", "providerObject"] {
        assert!(
            !sdl.contains(secret),
            "secret field leaked into schema: {secret}"
        );
    }
}
