use std::sync::Arc;

use async_graphql::{
    Context, EmptySubscription, Enum, Error, ErrorExtensions, InputObject, Object, Result, Schema,
    SimpleObject,
};
use async_trait::async_trait;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use crate::application::{
    ApplicationError, ClosingPolicyInput, CommandContext, CompleteUploadInput,
    ImageAttachmentInput, ListLotInput, LotPage, MoneyInput, Principal, PublicLot,
    RequestUploadInput, UpdateLotInput, UploadCompletedResult, UploadIntentResult,
};

#[derive(Clone)]
pub struct VerifiedRequestContext {
    pub principal: Principal,
    pub correlation_id: String,
    pub now_millis: u64,
}

#[async_trait]
pub trait CatalogGraphqlApi: Send + Sync {
    async fn lot(&self, lot_id: &str) -> std::result::Result<Option<PublicLot>, ApplicationError>;
    async fn lots(
        &self,
        first: usize,
        after: Option<String>,
    ) -> std::result::Result<LotPage, ApplicationError>;
    async fn request_upload(
        &self,
        input: RequestUploadInput,
    ) -> std::result::Result<UploadIntentResult, ApplicationError>;
    async fn complete_upload(
        &self,
        input: CompleteUploadInput,
    ) -> std::result::Result<UploadCompletedResult, ApplicationError>;
    async fn list_lot(
        &self,
        input: ListLotInput,
    ) -> std::result::Result<PublicLot, ApplicationError>;
    async fn update_lot(
        &self,
        input: UpdateLotInput,
    ) -> std::result::Result<PublicLot, ApplicationError>;
}

pub type CatalogSchema = Schema<QueryRoot, MutationRoot, EmptySubscription>;

pub fn schema(api: Arc<dyn CatalogGraphqlApi>) -> CatalogSchema {
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
    api: Arc<dyn CatalogGraphqlApi>,
}

#[Object(name = "Query")]
impl QueryRoot {
    async fn lot(&self, id: async_graphql::ID) -> Result<Option<LotObject>> {
        self.api
            .lot(id.as_str())
            .await
            .map(|lot| lot.map(Into::into))
            .map_err(graphql_error)
    }

    async fn lots(
        &self,
        #[graphql(default = 20)] first: i32,
        after: Option<String>,
    ) -> Result<LotConnectionObject> {
        let first = usize::try_from(first).map_err(|_| {
            graphql_error(ApplicationError::InvalidArgument {
                reason: "invalid_page_size",
                safe_message: "page size is invalid".to_owned(),
            })
        })?;
        self.api
            .lots(first, after)
            .await
            .map(Into::into)
            .map_err(graphql_error)
    }

    #[graphql(entity)]
    async fn find_lot_by_id(&self, id: async_graphql::ID) -> Result<Option<LotObject>> {
        self.api
            .lot(id.as_str())
            .await
            .map(|lot| lot.map(Into::into))
            .map_err(graphql_error)
    }
}

pub struct MutationRoot {
    api: Arc<dyn CatalogGraphqlApi>,
}

#[Object(name = "Mutation")]
impl MutationRoot {
    async fn request_lot_image_upload(
        &self,
        context: &Context<'_>,
        input: RequestLotImageUploadGraphqlInput,
        idempotency_key: String,
    ) -> Result<LotImageUploadObject> {
        let verified = verified(context)?;
        self.api
            .request_upload(RequestUploadInput {
                context: command_context(&verified, idempotency_key)?,
                content_type: input.content_type,
                content_length: u64::try_from(input.content_length).map_err(|_| {
                    graphql_error(ApplicationError::InvalidArgument {
                        reason: "invalid_content_length",
                        safe_message: "content length is invalid".to_owned(),
                    })
                })?,
                checksum_sha256: input.checksum_sha256,
            })
            .await
            .map(LotImageUploadObject::try_from)
            .map_err(graphql_error)?
            .map_err(graphql_error)
    }

    async fn complete_lot_image_upload(
        &self,
        context: &Context<'_>,
        image_id: async_graphql::ID,
        idempotency_key: String,
    ) -> Result<UploadedLotImageObject> {
        let verified = verified(context)?;
        self.api
            .complete_upload(CompleteUploadInput {
                context: command_context(&verified, idempotency_key)?,
                image_id: image_id.to_string(),
                now_millis: verified.now_millis,
            })
            .await
            .map(Into::into)
            .map_err(graphql_error)
    }

    async fn list_lot(
        &self,
        context: &Context<'_>,
        input: ListLotGraphqlInput,
        idempotency_key: String,
    ) -> Result<LotObject> {
        let verified = verified(context)?;
        let lot_id = deterministic_lot_id(&verified.principal.principal_id, &idempotency_key);
        let scheduled_end_millis = parse_timestamp(&input.scheduled_end, "invalid_scheduled_end")?;
        let hard_deadline_millis = input
            .hard_deadline
            .as_deref()
            .map(|value| parse_timestamp(value, "invalid_hard_deadline"))
            .transpose()?;
        let inactivity_millis = input
            .inactivity_seconds
            .map(|seconds| {
                u64::try_from(seconds)
                    .ok()
                    .and_then(|seconds| seconds.checked_mul(1_000))
                    .ok_or_else(|| {
                        graphql_error(ApplicationError::InvalidArgument {
                            reason: "invalid_inactivity",
                            safe_message: "inactivity window is invalid".to_owned(),
                        })
                    })
            })
            .transpose()?;
        let start_price = input.start_price.parse::<u64>().map_err(|_| {
            graphql_error(ApplicationError::InvalidArgument {
                reason: "invalid_start_price",
                safe_message: "start price is invalid".to_owned(),
            })
        })?;
        let bid_step_minor = input
            .bid_step
            .as_deref()
            .unwrap_or("1")
            .parse::<u64>()
            .map_err(|_| {
                graphql_error(ApplicationError::InvalidArgument {
                    reason: "invalid_bid_step",
                    safe_message: "bid step is invalid".to_owned(),
                })
            })?;
        self.api
            .list_lot(ListLotInput {
                context: command_context(&verified, idempotency_key)?,
                lot_id,
                title: input.title,
                description: input.description,
                images: input
                    .images
                    .into_iter()
                    .map(|image| ImageAttachmentInput {
                        image_id: image.image_id.to_string(),
                        alt: image.alt,
                    })
                    .collect(),
                start_price: MoneyInput {
                    minor: start_price,
                    currency_code: input.currency,
                },
                bid_step_minor,
                closing_policy: ClosingPolicyInput {
                    scheduled_end_millis,
                    inactivity_millis,
                    hard_deadline_millis,
                },
                occurred_at_millis: verified.now_millis,
            })
            .await
            .map(Into::into)
            .map_err(graphql_error)
    }

    async fn update_lot(
        &self,
        context: &Context<'_>,
        id: async_graphql::ID,
        input: UpdateLotGraphqlInput,
        idempotency_key: String,
    ) -> Result<LotObject> {
        let verified = verified(context)?;
        self.api
            .update_lot(UpdateLotInput {
                context: command_context(&verified, idempotency_key)?,
                lot_id: id.to_string(),
                title: input.title,
                description: input.description.map(Some),
                expected_version: input
                    .expected_version
                    .map(u64::try_from)
                    .transpose()
                    .map_err(|_| {
                        graphql_error(ApplicationError::InvalidArgument {
                            reason: "invalid_expected_version",
                            safe_message: "expected version is invalid".to_owned(),
                        })
                    })?,
                occurred_at_millis: verified.now_millis,
            })
            .await
            .map(Into::into)
            .map_err(graphql_error)
    }
}

fn deterministic_lot_id(principal_id: &str, idempotency_key: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"gavel-catalog-list-lot-id\0");
    digest.update(principal_id.as_bytes());
    digest.update([0]);
    digest.update(idempotency_key.as_bytes());
    format!(
        "lot-{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest.finalize())
    )
}

#[derive(InputObject)]
#[graphql(name = "RequestLotImageUploadInput")]
pub struct RequestLotImageUploadGraphqlInput {
    pub content_type: String,
    pub content_length: i32,
    pub checksum_sha256: String,
}

#[derive(InputObject)]
#[graphql(name = "LotImageInput")]
pub struct LotImageGraphqlInput {
    pub image_id: async_graphql::ID,
    pub alt: String,
}

#[derive(InputObject)]
#[graphql(name = "ListLotInput")]
pub struct ListLotGraphqlInput {
    pub title: String,
    pub description: Option<String>,
    pub images: Vec<LotImageGraphqlInput>,
    pub start_price: String,
    pub currency: String,
    pub bid_step: Option<String>,
    pub scheduled_end: String,
    pub inactivity_seconds: Option<i32>,
    pub hard_deadline: Option<String>,
}

#[derive(InputObject)]
#[graphql(name = "UpdateLotInput")]
pub struct UpdateLotGraphqlInput {
    pub title: Option<String>,
    pub description: Option<String>,
    pub expected_version: Option<i32>,
}

#[derive(SimpleObject)]
#[graphql(name = "Lot", complex)]
pub struct LotObject {
    pub id: async_graphql::ID,
    pub title: String,
    pub description: Option<String>,
    pub images: Vec<LotImageObject>,
}

#[async_graphql::ComplexObject]
impl LotObject {}

impl From<PublicLot> for LotObject {
    fn from(value: PublicLot) -> Self {
        Self {
            id: value.lot_id.into(),
            title: value.title,
            description: value.description,
            images: value.images.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(SimpleObject)]
#[graphql(name = "LotImage")]
pub struct LotImageObject {
    pub id: async_graphql::ID,
    pub url: String,
    pub alt: String,
}

impl From<crate::application::PublicLotImage> for LotImageObject {
    fn from(value: crate::application::PublicLotImage) -> Self {
        Self {
            id: value.image_id.into(),
            url: value.url,
            alt: value.alt,
        }
    }
}

#[derive(SimpleObject)]
#[graphql(name = "LotImageUpload")]
pub struct LotImageUploadObject {
    pub image_id: async_graphql::ID,
    pub upload_url: String,
    pub expires_at: String,
    pub required_headers: Vec<UploadHeaderObject>,
}

impl TryFrom<UploadIntentResult> for LotImageUploadObject {
    type Error = ApplicationError;

    fn try_from(value: UploadIntentResult) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            image_id: value.image_id.into(),
            upload_url: value.upload_url,
            expires_at: millis_to_rfc3339(value.expires_at_millis)?,
            required_headers: value
                .required_headers
                .into_iter()
                .map(|(name, value)| UploadHeaderObject { name, value })
                .collect(),
        })
    }
}

#[derive(SimpleObject)]
#[graphql(name = "UploadHeader")]
pub struct UploadHeaderObject {
    pub name: String,
    pub value: String,
}

#[derive(SimpleObject)]
#[graphql(name = "UploadedLotImage")]
pub struct UploadedLotImageObject {
    pub image_id: async_graphql::ID,
    pub status: LotImageUploadStatusObject,
}

impl From<UploadCompletedResult> for UploadedLotImageObject {
    fn from(value: UploadCompletedResult) -> Self {
        Self {
            image_id: value.image_id.into(),
            status: match value.status.as_str() {
                "ATTACHED" => LotImageUploadStatusObject::Attached,
                "READY" => LotImageUploadStatusObject::Ready,
                _ => LotImageUploadStatusObject::Pending,
            },
        }
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(name = "LotImageUploadStatus")]
pub enum LotImageUploadStatusObject {
    Pending,
    Ready,
    Attached,
}

#[derive(SimpleObject)]
#[graphql(name = "LotConnection")]
pub struct LotConnectionObject {
    pub edges: Vec<LotEdgeObject>,
    pub page_info: PageInfoObject,
}

impl From<LotPage> for LotConnectionObject {
    fn from(value: LotPage) -> Self {
        Self {
            edges: value
                .lots
                .into_iter()
                .zip(value.cursors)
                .map(|(lot, cursor)| LotEdgeObject {
                    cursor,
                    node: lot.into(),
                })
                .collect(),
            page_info: PageInfoObject {
                end_cursor: value.end_cursor,
                has_next_page: value.has_next_page,
            },
        }
    }
}

#[derive(SimpleObject)]
#[graphql(name = "LotEdge")]
pub struct LotEdgeObject {
    pub cursor: String,
    pub node: LotObject,
}

#[derive(SimpleObject)]
#[graphql(name = "PageInfo")]
pub struct PageInfoObject {
    pub end_cursor: Option<String>,
    pub has_next_page: bool,
}

fn verified(context: &Context<'_>) -> Result<VerifiedRequestContext> {
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
            reason: "invalid_idempotency_key",
            safe_message: "idempotency key is invalid".to_owned(),
        }));
    }
    Ok(CommandContext {
        caller_scope: verified.principal.principal_id.clone(),
        principal: Some(verified.principal.clone()),
        idempotency_key,
        correlation_id: verified.correlation_id.clone(),
        consistency_token: None,
    })
}

fn parse_timestamp(value: &str, reason: &'static str) -> Result<u64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .and_then(|value| u64::try_from(value.timestamp_millis()).ok())
        .ok_or_else(|| {
            graphql_error(ApplicationError::InvalidArgument {
                reason,
                safe_message: "timestamp is invalid".to_owned(),
            })
        })
}

fn millis_to_rfc3339(millis: u64) -> std::result::Result<String, ApplicationError> {
    let millis = i64::try_from(millis).map_err(|_| ApplicationError::Internal)?;
    DateTime::<Utc>::from_timestamp_millis(millis)
        .map(|value| value.to_rfc3339())
        .ok_or(ApplicationError::Internal)
}

fn graphql_error(error: ApplicationError) -> Error {
    let contract = super::error_mapping::graphql_error(&error);
    Error::new(contract.message).extend_with(|_, extensions| {
        extensions.set("code", contract.code);
        extensions.set("retryable", contract.retryable);
    })
}
