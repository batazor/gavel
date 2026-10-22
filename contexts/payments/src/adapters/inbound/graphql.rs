use std::sync::Arc;

use async_graphql::{
    Context, EmptySubscription, Error, ErrorExtensions, Object, Result, Schema, SimpleObject,
};

use crate::application::{
    ApplicationError, BeginRegistrationCommand, ContinuePaymentCommand, PaymentApplication,
    PaymentAttemptView, PaymentInstrumentView, RevokeInstrumentCommand,
};

#[derive(Clone, Debug)]
pub struct AuthenticatedPrincipal(pub String);

pub type PaymentSchema = Schema<QueryRoot, MutationRoot, EmptySubscription>;
pub fn build_schema(application: Arc<PaymentApplication>) -> PaymentSchema {
    Schema::build(QueryRoot, MutationRoot, EmptySubscription)
        .data(application)
        .finish()
}

pub struct QueryRoot;
#[Object]
impl QueryRoot {
    async fn my_payment_instrument(&self, context: &Context<'_>) -> Result<InstrumentObject> {
        let principal = principal(context)?;
        context
            .data::<Arc<PaymentApplication>>()?
            .my_instrument(&principal)
            .await
            .map(Into::into)
            .map_err(graphql_error)
    }
    async fn payment_attempt(
        &self,
        context: &Context<'_>,
        id: async_graphql::ID,
    ) -> Result<Option<AttemptObject>> {
        let principal = principal(context)?;
        match context
            .data::<Arc<PaymentApplication>>()?
            .payment_attempt(&principal, id.as_str())
            .await
        {
            Ok(value) => Ok(Some(value.into())),
            Err(ApplicationError::NotFound) => Ok(None),
            Err(error) => Err(graphql_error(error)),
        }
    }
}

pub struct MutationRoot;
#[Object]
impl MutationRoot {
    async fn begin_payment_instrument_registration(
        &self,
        context: &Context<'_>,
        input: BeginRegistrationInput,
        idempotency_key: String,
    ) -> Result<RegistrationObject> {
        let principal_id = principal(context)?;
        let result = context
            .data::<Arc<PaymentApplication>>()?
            .begin_registration(BeginRegistrationCommand {
                principal_id,
                recovery_email: input.recovery_email,
                terms_version: input.terms_version,
                idempotency_key,
            })
            .await
            .map_err(graphql_error)?;
        Ok(RegistrationObject {
            instrument: result.instrument.into(),
            provider_url: result.provider_url,
        })
    }
    async fn continue_payment(
        &self,
        context: &Context<'_>,
        attempt_id: async_graphql::ID,
        idempotency_key: String,
    ) -> Result<ContinuationObject> {
        let principal_id = principal(context)?;
        let attempt = context
            .data::<Arc<PaymentApplication>>()?
            .continue_payment(ContinuePaymentCommand {
                principal_id,
                attempt_id: attempt_id.to_string(),
                idempotency_key,
            })
            .await
            .map_err(graphql_error)?;
        Ok(ContinuationObject {
            provider_url: attempt.recovery_url.clone(),
            attempt: attempt.into(),
        })
    }

    async fn revoke_payment_instrument(
        &self,
        context: &Context<'_>,
        idempotency_key: String,
    ) -> Result<InstrumentObject> {
        let principal_id = principal(context)?;
        context
            .data::<Arc<PaymentApplication>>()?
            .revoke_instrument(RevokeInstrumentCommand {
                principal_id,
                idempotency_key,
            })
            .await
            .map(Into::into)
            .map_err(graphql_error)
    }
}

#[derive(async_graphql::InputObject)]
pub struct BeginRegistrationInput {
    pub recovery_email: String,
    pub terms_version: String,
}
#[derive(SimpleObject)]
pub struct InstrumentObject {
    pub id: async_graphql::ID,
    pub status: String,
    pub display: Option<String>,
    pub version: u64,
}
#[derive(SimpleObject)]
pub struct AttemptObject {
    pub id: async_graphql::ID,
    pub auction_id: async_graphql::ID,
    pub status: String,
    pub recovery_url: Option<String>,
    pub version: u64,
}
#[derive(SimpleObject)]
pub struct RegistrationObject {
    pub instrument: InstrumentObject,
    pub provider_url: String,
}
#[derive(SimpleObject)]
pub struct ContinuationObject {
    pub attempt: AttemptObject,
    pub provider_url: Option<String>,
}

impl From<PaymentInstrumentView> for InstrumentObject {
    fn from(value: PaymentInstrumentView) -> Self {
        Self {
            id: value.id.into(),
            status: format!("{:?}", value.status).to_ascii_uppercase(),
            display: value.display,
            version: value.version,
        }
    }
}
impl From<PaymentAttemptView> for AttemptObject {
    fn from(value: PaymentAttemptView) -> Self {
        Self {
            id: value.id.into(),
            auction_id: value.auction_id.into(),
            status: format!("{:?}", value.status).to_ascii_uppercase(),
            recovery_url: value.recovery_url,
            version: value.version,
        }
    }
}
fn principal(context: &Context<'_>) -> Result<String> {
    context
        .data_opt::<AuthenticatedPrincipal>()
        .map(|value| value.0.clone())
        .ok_or_else(|| {
            Error::new("authentication required")
                .extend_with(|_, ext| ext.set("code", "UNAUTHENTICATED"))
        })
}
fn graphql_error(error: ApplicationError) -> Error {
    Error::new(error.to_string())
        .extend_with(|_, ext| ext.set("code", format!("{:?}", error.code()).to_ascii_uppercase()))
}
