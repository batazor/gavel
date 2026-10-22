use serde::{Deserialize, Serialize};

use super::{
    DomainError, EmailAddress, PaymentInstrumentId, PrincipalId, ProviderReference,
    RedactedDisplay, TermsVersion,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebitAuthorization {
    terms_version: TermsVersion,
    accepted_at_millis: u64,
}
impl DebitAuthorization {
    pub const fn new(terms_version: TermsVersion, accepted_at_millis: u64) -> Self {
        Self {
            terms_version,
            accepted_at_millis,
        }
    }
    pub const fn accepted_at_millis(&self) -> u64 {
        self.accepted_at_millis
    }
    pub fn terms_version(&self) -> &TermsVersion {
        &self.terms_version
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentInstrumentState {
    Pending,
    Ready,
    Revoked,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstrumentTransition {
    Ready { version: u64 },
    Revoked { version: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentInstrument {
    id: PaymentInstrumentId,
    owner_id: PrincipalId,
    state: PaymentInstrumentState,
    recovery_email: EmailAddress,
    authorization: DebitAuthorization,
    provider_session_ref: ProviderReference,
    provider_customer_ref: Option<ProviderReference>,
    provider_instrument_ref: Option<ProviderReference>,
    display: Option<RedactedDisplay>,
    version: u64,
    updated_at_millis: u64,
}

impl PaymentInstrument {
    pub fn pending(
        id: PaymentInstrumentId,
        owner_id: PrincipalId,
        recovery_email: EmailAddress,
        authorization: DebitAuthorization,
        provider_session_ref: ProviderReference,
        now: u64,
    ) -> Self {
        Self {
            id,
            owner_id,
            state: PaymentInstrumentState::Pending,
            recovery_email,
            authorization,
            provider_session_ref,
            provider_customer_ref: None,
            provider_instrument_ref: None,
            display: None,
            version: 1,
            updated_at_millis: now,
        }
    }
    pub fn mark_ready(
        &mut self,
        customer_ref: ProviderReference,
        instrument_ref: ProviderReference,
        display: RedactedDisplay,
        now: u64,
    ) -> Result<InstrumentTransition, DomainError> {
        if self.state != PaymentInstrumentState::Pending {
            return Err(DomainError::InvalidTransition);
        }
        self.version = self
            .version
            .checked_add(1)
            .ok_or(DomainError::VersionOverflow)?;
        self.state = PaymentInstrumentState::Ready;
        self.provider_customer_ref = Some(customer_ref);
        self.provider_instrument_ref = Some(instrument_ref);
        self.display = Some(display);
        self.updated_at_millis = now;
        Ok(InstrumentTransition::Ready {
            version: self.version,
        })
    }
    pub fn revoke(&mut self, now: u64) -> Result<InstrumentTransition, DomainError> {
        if self.state != PaymentInstrumentState::Ready {
            return Err(DomainError::InvalidTransition);
        }
        self.version = self
            .version
            .checked_add(1)
            .ok_or(DomainError::VersionOverflow)?;
        self.state = PaymentInstrumentState::Revoked;
        self.provider_customer_ref = None;
        self.provider_instrument_ref = None;
        self.updated_at_millis = now;
        Ok(InstrumentTransition::Revoked {
            version: self.version,
        })
    }
    pub fn id(&self) -> &PaymentInstrumentId {
        &self.id
    }
    pub fn owner_id(&self) -> &PrincipalId {
        &self.owner_id
    }
    pub const fn state(&self) -> PaymentInstrumentState {
        self.state
    }
    pub const fn version(&self) -> u64 {
        self.version
    }
    pub fn display(&self) -> Option<&RedactedDisplay> {
        self.display.as_ref()
    }
    pub fn provider_customer_ref(&self) -> Option<&ProviderReference> {
        self.provider_customer_ref.as_ref()
    }
    pub fn recovery_email(&self) -> &EmailAddress {
        &self.recovery_email
    }
    pub fn authorization(&self) -> &DebitAuthorization {
        &self.authorization
    }
    pub fn provider_session_ref(&self) -> &ProviderReference {
        &self.provider_session_ref
    }
    pub fn provider_instrument_ref(&self) -> Option<&ProviderReference> {
        self.provider_instrument_ref.as_ref()
    }
}
