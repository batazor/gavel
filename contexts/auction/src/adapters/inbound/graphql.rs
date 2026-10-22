use std::sync::Arc;

use async_graphql::{
    Context, EmptySubscription, Enum, Error, ErrorExtensions, InputObject, Object, Result, Schema,
    SimpleObject,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    application::{
        ApplicationError, AuctionProjectionReader, AuctionUnitOfWork, AuctionView, Authorization,
        CommandContext, PaymentReadiness, PlaceBid, PlaceBidInput, PlaceBidResult, Principal,
        PublicAuctionStatus, RescheduleAuction, RescheduleAuctionInput,
    },
    domain::Timestamp,
};

#[derive(Clone)]
pub struct VerifiedRequestContext {
    pub principal: Principal,
    pub correlation_id: String,
    pub causation_id: String,
    pub now: Timestamp,
}

#[async_trait]
pub trait AuctionGraphqlApi: Send + Sync {
    async fn auction(&self, auction_id: &str) -> Result<Option<AuctionView>, ApplicationError>;
    async fn place_bid(
        &self,
        context: &CommandContext,
        input: PlaceBidInput,
    ) -> Result<PlaceBidResult, ApplicationError>;
    async fn reschedule_auction(
        &self,
        context: &CommandContext,
        input: RescheduleAuctionInput,
    ) -> Result<AuctionView, ApplicationError>;
}

pub struct AuctionGraphqlApplication<U, A, P> {
    reader: Arc<U>,
    place_bid: PlaceBid<U, A, P>,
    reschedule: RescheduleAuction<U, A>,
}

impl<U, A, P> AuctionGraphqlApplication<U, A, P>
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
            reader: Arc::clone(&unit_of_work),
            place_bid: PlaceBid::new(
                Arc::clone(&unit_of_work),
                Arc::clone(&authorization),
                payment_readiness,
                retry_attempts,
                fingerprint_key,
            ),
            reschedule: RescheduleAuction::new(
                unit_of_work,
                authorization,
                retry_attempts,
                fingerprint_key,
            ),
        }
    }
}

#[async_trait]
impl<U, A, P> AuctionGraphqlApi for AuctionGraphqlApplication<U, A, P>
where
    U: AuctionUnitOfWork + AuctionProjectionReader + 'static,
    A: Authorization + 'static,
    P: PaymentReadiness + 'static,
{
    async fn auction(&self, auction_id: &str) -> Result<Option<AuctionView>, ApplicationError> {
        self.reader
            .auction(auction_id)
            .await
            .map(|projection| projection.map(|projection| projection.view))
            .map_err(|_| ApplicationError::DependencyUnavailable {
                dependency: "auction_db",
            })
    }

    async fn place_bid(
        &self,
        context: &CommandContext,
        input: PlaceBidInput,
    ) -> Result<PlaceBidResult, ApplicationError> {
        self.place_bid.handle(context, input).await
    }

    async fn reschedule_auction(
        &self,
        context: &CommandContext,
        input: RescheduleAuctionInput,
    ) -> Result<AuctionView, ApplicationError> {
        self.reschedule.handle(context, input).await
    }
}

pub type AuctionSchema = Schema<QueryRoot, MutationRoot, EmptySubscription>;

pub fn schema(api: Arc<dyn AuctionGraphqlApi>) -> AuctionSchema {
    Schema::build(
        QueryRoot {
            api: Arc::clone(&api),
        },
        MutationRoot { api },
        EmptySubscription,
    )
    .finish()
}

pub struct QueryRoot {
    api: Arc<dyn AuctionGraphqlApi>,
}

#[Object]
impl QueryRoot {
    async fn auction(&self, id: async_graphql::ID) -> Result<Option<AuctionObject>> {
        self.api
            .auction(id.as_str())
            .await
            .map(|view| view.map(AuctionObject::try_from).transpose())
            .map_err(graphql_error)?
            .map_err(graphql_error)
    }

    async fn current_price(
        &self,
        auction_id: async_graphql::ID,
    ) -> Result<Option<CurrentPriceObject>> {
        self.api
            .auction(auction_id.as_str())
            .await
            .map(|view| view.map(CurrentPriceObject::try_from).transpose())
            .map_err(graphql_error)?
            .map_err(graphql_error)
    }
}

pub struct MutationRoot {
    api: Arc<dyn AuctionGraphqlApi>,
}

#[Object]
impl MutationRoot {
    async fn place_bid(
        &self,
        context: &Context<'_>,
        input: PlaceBidGraphqlInput,
        idempotency_key: String,
    ) -> Result<PlaceBidObject> {
        let verified = verified_context(context)?;
        let command_context = command_context(&verified, idempotency_key)?;
        let maximum_minor = input.maximum_amount.parse().map_err(|_| {
            graphql_error(ApplicationError::InvalidArgument {
                reason: "invalid_maximum_amount",
                safe_message: "maximumAmount must be an unsigned integer".to_owned(),
            })
        })?;
        let result = self
            .api
            .place_bid(
                &command_context,
                PlaceBidInput {
                    auction_id: input.auction_id.to_string(),
                    bid_id: Uuid::now_v7().to_string(),
                    maximum_minor,
                    currency_code: input.currency,
                    placed_at: verified.now,
                },
            )
            .await
            .map_err(graphql_error)?;
        PlaceBidObject::try_from(result).map_err(graphql_error)
    }

    async fn reschedule_auction(
        &self,
        context: &Context<'_>,
        input: RescheduleAuctionGraphqlInput,
        idempotency_key: String,
    ) -> Result<AuctionObject> {
        let verified = verified_context(context)?;
        let command_context = command_context(&verified, idempotency_key)?;
        let scheduled_end = DateTime::parse_from_rfc3339(&input.scheduled_end)
            .ok()
            .and_then(|value| u64::try_from(value.timestamp_millis()).ok())
            .map(Timestamp::from_millis)
            .ok_or_else(|| {
                graphql_error(ApplicationError::InvalidArgument {
                    reason: "invalid_scheduled_end",
                    safe_message: "scheduledEnd must be an RFC 3339 timestamp".to_owned(),
                })
            })?;
        let view = self
            .api
            .reschedule_auction(
                &command_context,
                RescheduleAuctionInput {
                    auction_id: input.auction_id.to_string(),
                    scheduled_end,
                    now: verified.now,
                },
            )
            .await
            .map_err(graphql_error)?;
        AuctionObject::try_from(view).map_err(graphql_error)
    }
}

#[derive(InputObject)]
#[graphql(name = "PlaceBidInput")]
pub struct PlaceBidGraphqlInput {
    pub auction_id: async_graphql::ID,
    pub maximum_amount: String,
    pub currency: String,
}

#[derive(InputObject)]
#[graphql(name = "RescheduleAuctionInput")]
pub struct RescheduleAuctionGraphqlInput {
    pub auction_id: async_graphql::ID,
    pub scheduled_end: String,
}

#[derive(SimpleObject)]
#[graphql(name = "PlaceBidResult")]
pub struct PlaceBidObject {
    pub auction: AuctionObject,
    pub accepted_version: i32,
    pub outbid: bool,
}

impl TryFrom<PlaceBidResult> for PlaceBidObject {
    type Error = ApplicationError;

    fn try_from(value: PlaceBidResult) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            auction: AuctionObject::try_from(value.auction)?,
            accepted_version: version_to_i32(value.accepted_version)?,
            outbid: value.outbid,
        })
    }
}

#[derive(SimpleObject)]
#[graphql(name = "Auction")]
pub struct AuctionObject {
    pub id: async_graphql::ID,
    pub lot_id: async_graphql::ID,
    pub status: AuctionStatusObject,
    pub current_price: CurrentPriceObject,
    pub ends_at: String,
    pub viewer: Option<AuctionViewerObject>,
}

impl TryFrom<AuctionView> for AuctionObject {
    type Error = ApplicationError;

    fn try_from(value: AuctionView) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            id: value.auction_id.into(),
            lot_id: value.lot_id.into(),
            status: value.status.into(),
            current_price: CurrentPriceObject {
                amount: value.current_price.minor_units.to_string(),
                currency: value.current_price.currency_code,
                version: version_to_i32(value.version)?,
            },
            ends_at: millis_to_rfc3339(value.ends_at_millis)?,
            viewer: None,
        })
    }
}

#[derive(SimpleObject)]
#[graphql(name = "CurrentPrice")]
pub struct CurrentPriceObject {
    pub amount: String,
    pub currency: String,
    pub version: i32,
}

impl TryFrom<AuctionView> for CurrentPriceObject {
    type Error = ApplicationError;

    fn try_from(value: AuctionView) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            amount: value.current_price.minor_units.to_string(),
            currency: value.current_price.currency_code,
            version: version_to_i32(value.version)?,
        })
    }
}

#[derive(SimpleObject)]
#[graphql(name = "AuctionViewer")]
pub struct AuctionViewerObject {
    pub is_leader: bool,
    pub is_settlement_candidate: bool,
    pub is_winner: bool,
    pub can_bid: bool,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(name = "AuctionStatus")]
pub enum AuctionStatusObject {
    Scheduled,
    Open,
    Settling,
    Sold,
    Unsold,
    Cancelled,
}

impl From<PublicAuctionStatus> for AuctionStatusObject {
    fn from(value: PublicAuctionStatus) -> Self {
        match value {
            PublicAuctionStatus::Scheduled => Self::Scheduled,
            PublicAuctionStatus::Open => Self::Open,
            PublicAuctionStatus::Settling => Self::Settling,
            PublicAuctionStatus::Sold => Self::Sold,
            PublicAuctionStatus::Unsold => Self::Unsold,
            PublicAuctionStatus::Cancelled => Self::Cancelled,
        }
    }
}

fn verified_context(context: &Context<'_>) -> Result<VerifiedRequestContext> {
    context
        .data_opt::<VerifiedRequestContext>()
        .cloned()
        .ok_or_else(|| graphql_error(ApplicationError::Unauthenticated))
}

fn command_context(
    verified: &VerifiedRequestContext,
    idempotency_key: String,
) -> Result<CommandContext> {
    if idempotency_key.trim().is_empty() {
        return Err(graphql_error(ApplicationError::InvalidArgument {
            reason: "missing_idempotency_key",
            safe_message: "idempotency key is required".to_owned(),
        }));
    }
    Ok(CommandContext {
        principal: verified.principal.clone(),
        idempotency_key,
        correlation_id: verified.correlation_id.clone(),
        causation_id: verified.causation_id.clone(),
    })
}

fn graphql_error(error: ApplicationError) -> Error {
    let code = error.code().as_str();
    let retryable = error.retryable();
    Error::new(error.to_string()).extend_with(|_, extensions| {
        extensions.set("code", code);
        extensions.set("retryable", retryable);
    })
}

fn version_to_i32(version: u64) -> std::result::Result<i32, ApplicationError> {
    version.try_into().map_err(|_| ApplicationError::Internal)
}

fn millis_to_rfc3339(millis: u64) -> std::result::Result<String, ApplicationError> {
    let millis = i64::try_from(millis).map_err(|_| ApplicationError::Internal)?;
    DateTime::<Utc>::from_timestamp_millis(millis)
        .map(|value| value.to_rfc3339())
        .ok_or(ApplicationError::Internal)
}
