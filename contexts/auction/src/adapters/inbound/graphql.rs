use std::sync::Arc;

use async_graphql::{
    Context, EmptySubscription, Enum, Error, ErrorExtensions, InputObject, Object, Result, Schema,
    SimpleObject,
};
use async_trait::async_trait;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use crate::{
    application::{
        ApplicationError, AuctionProjectionReader, AuctionUnitOfWork, AuctionView, Authorization,
        AuthorizationDecision, CommandContext, PaymentReadiness, PermissionCheck, PlaceBid,
        PlaceBidInput, PlaceBidResult, Principal, PrincipalKind, PublicAuctionStatus,
        RescheduleAuction, RescheduleAuctionInput,
    },
    domain::{Auction, BidderId, Timestamp},
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
    async fn auction_by_lot_id(
        &self,
        lot_id: &str,
    ) -> Result<Option<AuctionView>, ApplicationError>;
    async fn viewer(
        &self,
        _auction_id: &str,
        _principal: &Principal,
    ) -> Result<Option<AuctionViewerObject>, ApplicationError> {
        Ok(None)
    }
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
    unit_of_work: Arc<U>,
    authorization: Arc<A>,
    payment_readiness: Arc<P>,
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
            unit_of_work: Arc::clone(&unit_of_work),
            authorization: Arc::clone(&authorization),
            payment_readiness: Arc::clone(&payment_readiness),
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

    async fn auction_by_lot_id(
        &self,
        lot_id: &str,
    ) -> Result<Option<AuctionView>, ApplicationError> {
        self.reader
            .auction_by_lot_id(lot_id)
            .await
            .map(|projection| projection.map(|projection| projection.view))
            .map_err(|_| ApplicationError::DependencyUnavailable {
                dependency: "auction_db",
            })
    }

    async fn viewer(
        &self,
        auction_id: &str,
        principal: &Principal,
    ) -> Result<Option<AuctionViewerObject>, ApplicationError> {
        if principal.kind != PrincipalKind::User {
            return Ok(None);
        }
        let loaded = self
            .unit_of_work
            .load(auction_id)
            .await
            .map_err(|_| ApplicationError::DependencyUnavailable {
                dependency: "auction_db",
            })?
            .ok_or(ApplicationError::NotFound)?;
        let mut aggregate: Auction = loaded.snapshot.unwrap_or_default();
        for event in loaded.events {
            aggregate.apply(event).map_err(ApplicationError::from)?;
        }
        let bidder = BidderId::from(principal.id.clone());
        let can_bid = if aggregate.status() == crate::domain::AuctionStatus::Open {
            let authorized = self
                .authorization
                .check(PermissionCheck {
                    principal: principal.clone(),
                    resource_type: "auction".to_owned(),
                    resource_id: auction_id.to_owned(),
                    permission: "place_bid".to_owned(),
                })
                .await
                .map_err(|_| ApplicationError::DependencyUnavailable {
                    dependency: "authz",
                })?
                == AuthorizationDecision::Allowed;
            authorized
                && self
                    .payment_readiness
                    .is_ready(&principal.id)
                    .await
                    .map_err(|_| ApplicationError::DependencyUnavailable {
                        dependency: "payments_projection",
                    })?
        } else {
            false
        };
        Ok(Some(AuctionViewerObject {
            is_leader: aggregate.leader() == Some(&bidder),
            is_settlement_candidate: aggregate
                .settlement_candidate()
                .is_some_and(|(candidate, _, _)| candidate == &bidder),
            is_winner: aggregate.winner() == Some(&bidder),
            can_bid,
        }))
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
    .enable_federation()
    .finish()
}

pub struct QueryRoot {
    api: Arc<dyn AuctionGraphqlApi>,
}

#[Object(name = "Query")]
impl QueryRoot {
    async fn auction(
        &self,
        context: &Context<'_>,
        id: async_graphql::ID,
    ) -> Result<Option<AuctionObject>> {
        let view = self.api.auction(id.as_str()).await.map_err(graphql_error)?;
        auction_object(&self.api, context, view).await
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

    #[graphql(entity)]
    async fn find_auction_by_id(
        &self,
        context: &Context<'_>,
        id: async_graphql::ID,
    ) -> Result<Option<AuctionObject>> {
        let view = self.api.auction(id.as_str()).await.map_err(graphql_error)?;
        auction_object(&self.api, context, view).await
    }

    #[graphql(entity)]
    async fn find_lot_by_id(&self, id: async_graphql::ID) -> LotObject {
        LotObject {
            id,
            api: Arc::clone(&self.api),
        }
    }
}

pub struct MutationRoot {
    api: Arc<dyn AuctionGraphqlApi>,
}

#[Object(name = "Mutation")]
impl MutationRoot {
    async fn place_bid(
        &self,
        context: &Context<'_>,
        input: PlaceBidGraphqlInput,
        idempotency_key: String,
    ) -> Result<PlaceBidObject> {
        let verified = verified_context(context)?;
        let command_context = command_context(&verified, idempotency_key)?;
        let bid_id = deterministic_bid_id(
            &verified.principal.id,
            input.auction_id.as_str(),
            &command_context.idempotency_key,
        );
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
                    bid_id,
                    maximum_minor,
                    currency_code: input.currency,
                    placed_at: verified.now,
                },
            )
            .await
            .map_err(graphql_error)?;
        let is_leader = !result.outbid;
        let mut object = PlaceBidObject::try_from(result).map_err(graphql_error)?;
        object.auction.viewer = Some(AuctionViewerObject {
            is_leader,
            is_settlement_candidate: false,
            is_winner: false,
            can_bid: true,
        });
        Ok(object)
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

pub struct LotObject {
    id: async_graphql::ID,
    api: Arc<dyn AuctionGraphqlApi>,
}

#[Object(name = "Lot", extends)]
impl LotObject {
    #[graphql(external)]
    async fn id(&self) -> &async_graphql::ID {
        &self.id
    }

    async fn auction(&self, context: &Context<'_>) -> Result<Option<AuctionObject>> {
        let view = self
            .api
            .auction_by_lot_id(self.id.as_str())
            .await
            .map_err(graphql_error)?;
        auction_object(&self.api, context, view).await
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

#[derive(Clone, SimpleObject)]
#[graphql(name = "AuctionViewer")]
pub struct AuctionViewerObject {
    pub is_leader: bool,
    pub is_settlement_candidate: bool,
    pub is_winner: bool,
    pub can_bid: bool,
}

async fn auction_object(
    api: &Arc<dyn AuctionGraphqlApi>,
    context: &Context<'_>,
    view: Option<AuctionView>,
) -> Result<Option<AuctionObject>> {
    let Some(view) = view else { return Ok(None) };
    let mut object = AuctionObject::try_from(view).map_err(graphql_error)?;
    if let Some(verified) = context.data_opt::<VerifiedRequestContext>() {
        object.viewer = api
            .viewer(object.id.as_str(), &verified.principal)
            .await
            .map_err(graphql_error)?;
    }
    Ok(Some(object))
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

fn deterministic_bid_id(principal_id: &str, auction_id: &str, idempotency_key: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"gavel-auction-place-bid-id\0");
    digest.update(principal_id.as_bytes());
    digest.update([0]);
    digest.update(auction_id.as_bytes());
    digest.update([0]);
    digest.update(idempotency_key.as_bytes());
    format!(
        "bid-{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest.finalize())
    )
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
