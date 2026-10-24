use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::domain::{
    AuctionId, CandidateId, DebitAuthorization, DomainEvent, EmailAddress, Money, PaymentAttempt,
    PaymentAttemptId, PaymentAttemptState, PaymentInstrument, PaymentInstrumentState, PrincipalId,
    SettlementRound, TermsVersion,
};

use super::{
    ApplicationError, Authorization, AuthorizationRequest, CallbackClaim, CallbackInbox, Clock,
    Commit, CommitConflict, IdGenerator, PaymentsUnitOfWork, Permission, PortError,
    PrepareInstrumentRequest, ProviderGateway, ProviderOutcome, ReceiptDecision, ReceiptExecutor,
    Telemetry, VerifiedProviderEvent, VerifiedProviderOutcome,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BeginRegistrationCommand {
    pub principal_id: String,
    pub recovery_email: String,
    pub terms_version: String,
    pub idempotency_key: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttemptPaymentCommand {
    pub workload_id: String,
    pub auction_id: String,
    pub settlement_round: u32,
    pub candidate_id: String,
    pub amount_minor_units: u64,
    pub currency_code: String,
    pub idempotency_key: String,
    pub consistency_token: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContinuePaymentCommand {
    pub principal_id: String,
    pub attempt_id: String,
    pub idempotency_key: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExpirePaymentCommand {
    pub workload_id: String,
    pub attempt_id: String,
    pub idempotency_key: String,
    pub consistency_token: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetSettlementAttemptQuery {
    pub workload_id: String,
    pub attempt_id: String,
    pub expected_auction_id: String,
    pub expected_settlement_round: u32,
    pub consistency_token: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RevokeInstrumentCommand {
    pub principal_id: String,
    pub idempotency_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentInstrumentView {
    pub id: String,
    pub status: PaymentInstrumentState,
    pub display: Option<String>,
    pub version: u64,
    pub checked_at_token: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentAttemptView {
    pub id: String,
    pub auction_id: String,
    pub status: PaymentAttemptState,
    pub recovery_url: Option<String>,
    pub version: u64,
    pub checked_at_token: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SettlementAttemptData {
    None,
    RecoveryRequired { recovery_url: String },
    Succeeded { payment_id: String },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementAttemptView {
    pub payment_attempt_id: String,
    pub auction_id: String,
    pub settlement_round: u32,
    pub candidate_id: String,
    pub status: PaymentAttemptState,
    pub version: u64,
    pub checked_at_token: String,
    pub state_data: SettlementAttemptData,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistrationResult {
    pub instrument: PaymentInstrumentView,
    pub provider_url: String,
}

pub struct PaymentApplication {
    pub uow: Arc<dyn PaymentsUnitOfWork>,
    pub provider: Arc<dyn ProviderGateway>,
    pub authz: Arc<dyn Authorization>,
    pub clock: Arc<dyn Clock>,
    pub ids: Arc<dyn IdGenerator>,
    pub receipts: Arc<ReceiptExecutor>,
    pub telemetry: Arc<dyn Telemetry>,
    pub callbacks: Arc<dyn CallbackInbox>,
}

impl PaymentApplication {
    pub async fn begin_registration(
        &self,
        command: BeginRegistrationCommand,
    ) -> Result<RegistrationResult, ApplicationError> {
        let decision: ReceiptDecision<RegistrationResult> = self
            .receipts
            .claim(
                &command.principal_id,
                "begin_instrument_registration",
                &command.idempotency_key,
                &command,
            )
            .await?;
        let ReceiptDecision::Execute { token } = decision else {
            let ReceiptDecision::Replay(result) = decision else {
                unreachable!()
            };
            return Ok(result);
        };
        let result = self.begin_registration_once(&command).await;
        match &result {
            Ok(value) => {
                if let Err(error) = self.receipts.complete(&token, value).await {
                    self.receipts.release(&token).await;
                    return Err(error);
                }
            }
            Err(_) => self.receipts.release(&token).await,
        }
        result
    }

    async fn begin_registration_once(
        &self,
        command: &BeginRegistrationCommand,
    ) -> Result<RegistrationResult, ApplicationError> {
        let owner = PrincipalId::parse(&command.principal_id)?;
        let email = EmailAddress::parse(&command.recovery_email)?;
        let terms = TermsVersion::parse(&command.terms_version)?;
        let auth = self
            .authz
            .check(AuthorizationRequest {
                principal: owner.to_string(),
                permission: Permission::RegisterInstrument,
                resource_id: None,
                consistency_token: None,
                timeout_millis: 2_000,
            })
            .await?;
        if !auth.allowed {
            return Err(ApplicationError::PermissionDenied);
        }
        let provider_key = format!(
            "instrument:{}",
            self.receipts.opaque_key(
                "begin_instrument_registration",
                owner.as_str(),
                &command.idempotency_key,
            )
        );
        let prepared = self
            .provider
            .prepare(PrepareInstrumentRequest {
                provider_key,
                recovery_email: email.expose().to_owned(),
                timeout_millis: 5_000,
            })
            .await?;
        if let Some(existing) = self.uow.instrument_for_owner(&owner).await? {
            if existing.provider_session_ref() != &prepared.provider_session_ref
                || existing.recovery_email() != &email
                || existing.authorization().terms_version() != &terms
            {
                return Err(ApplicationError::IdempotencyConflict);
            }
            self.authz
                .touch_owner_grant(
                    existing.id().as_str(),
                    &owner,
                    &format!("instrument-owner:{}", existing.id()),
                    2_000,
                )
                .await?;
            return Ok(RegistrationResult {
                instrument: instrument_view(&existing, auth.checked_at_token),
                provider_url: prepared.provider_url,
            });
        }
        let id = self.ids.instrument_id()?;
        let instrument = PaymentInstrument::pending(
            id.clone(),
            owner.clone(),
            email,
            DebitAuthorization::new(terms, self.clock.now_millis()),
            prepared.provider_session_ref,
            self.clock.now_millis(),
        );
        self.authz
            .touch_owner_grant(
                id.as_str(),
                &owner,
                &format!("instrument-owner:{id}"),
                2_000,
            )
            .await?;
        if let Err(error) = self
            .uow
            .save_instrument(
                Commit {
                    value: instrument.clone(),
                    events: vec![],
                },
                None,
            )
            .await
        {
            let _ = self
                .authz
                .delete_owner_grant(
                    id.as_str(),
                    &owner,
                    &format!("rollback-instrument-owner:{id}"),
                    2_000,
                )
                .await;
            return Err(map_commit(error));
        }
        self.telemetry
            .command("begin_instrument_registration", "success");
        Ok(RegistrationResult {
            instrument: instrument_view(&instrument, auth.checked_at_token),
            provider_url: prepared.provider_url,
        })
    }

    pub async fn my_instrument(
        &self,
        principal_id: &str,
    ) -> Result<PaymentInstrumentView, ApplicationError> {
        let owner = PrincipalId::parse(principal_id)?;
        let instrument = self
            .uow
            .instrument_for_owner(&owner)
            .await?
            .ok_or(ApplicationError::NotFound)?;
        let auth = self
            .authz
            .check(AuthorizationRequest {
                principal: owner.to_string(),
                permission: Permission::ViewInstrument,
                resource_id: Some(instrument.id().to_string()),
                consistency_token: None,
                timeout_millis: 2_000,
            })
            .await?;
        if !auth.allowed {
            return Err(ApplicationError::PermissionDenied);
        }
        Ok(instrument_view(&instrument, auth.checked_at_token))
    }

    pub async fn payment_attempt(
        &self,
        principal_id: &str,
        attempt_id: &str,
    ) -> Result<PaymentAttemptView, ApplicationError> {
        let principal = PrincipalId::parse(principal_id)?;
        let id = PaymentAttemptId::parse(attempt_id)?;
        let attempt = self
            .uow
            .attempt(&id)
            .await?
            .ok_or(ApplicationError::NotFound)?;
        if attempt.candidate_id().as_str() != principal.as_str() {
            return Err(ApplicationError::NotFound);
        }
        let auth = self
            .authz
            .check(AuthorizationRequest {
                principal: principal.to_string(),
                permission: Permission::ViewAttempt,
                resource_id: Some(id.to_string()),
                consistency_token: None,
                timeout_millis: 2_000,
            })
            .await?;
        if !auth.allowed {
            return Err(ApplicationError::PermissionDenied);
        }
        Ok(attempt_view(
            &attempt,
            attempt.recovery_url().map(str::to_owned),
            auth.checked_at_token,
        ))
    }

    pub async fn get_settlement_attempt(
        &self,
        query: GetSettlementAttemptQuery,
    ) -> Result<SettlementAttemptView, ApplicationError> {
        let attempt_id = PaymentAttemptId::parse(&query.attempt_id)?;
        let expected_auction = AuctionId::parse(&query.expected_auction_id)?;
        let expected_round = SettlementRound::new(query.expected_settlement_round)?;
        let auth = self
            .authz
            .check(AuthorizationRequest {
                principal: query.workload_id,
                permission: Permission::ProcessPayment,
                resource_id: Some(expected_auction.to_string()),
                consistency_token: query.consistency_token,
                timeout_millis: 2_000,
            })
            .await?;
        if !auth.allowed {
            return Err(ApplicationError::PermissionDenied);
        }
        let attempt = self
            .uow
            .attempt(&attempt_id)
            .await?
            .ok_or(ApplicationError::NotFound)?;
        if attempt.auction_id() != &expected_auction || attempt.settlement_round() != expected_round
        {
            return Err(ApplicationError::InvalidState);
        }

        let state_data = match attempt.state() {
            PaymentAttemptState::RecoveryRequired => SettlementAttemptData::RecoveryRequired {
                recovery_url: attempt
                    .recovery_url()
                    .ok_or(ApplicationError::Internal)?
                    .to_owned(),
            },
            PaymentAttemptState::Succeeded => SettlementAttemptData::Succeeded {
                payment_id: attempt.id().to_string(),
            },
            PaymentAttemptState::Pending
            | PaymentAttemptState::Processing
            | PaymentAttemptState::Expired => SettlementAttemptData::None,
        };
        Ok(SettlementAttemptView {
            payment_attempt_id: attempt.id().to_string(),
            auction_id: attempt.auction_id().to_string(),
            settlement_round: attempt.settlement_round().get(),
            candidate_id: attempt.candidate_id().to_string(),
            status: attempt.state(),
            version: attempt.version(),
            checked_at_token: auth.checked_at_token,
            state_data,
        })
    }

    pub async fn revoke_instrument(
        &self,
        command: RevokeInstrumentCommand,
    ) -> Result<PaymentInstrumentView, ApplicationError> {
        let owner = PrincipalId::parse(&command.principal_id)?;
        let mut instrument = self
            .uow
            .instrument_for_owner(&owner)
            .await?
            .ok_or(ApplicationError::NotFound)?;
        let auth = self
            .authz
            .check(AuthorizationRequest {
                principal: owner.to_string(),
                permission: Permission::ManageInstrument,
                resource_id: Some(instrument.id().to_string()),
                consistency_token: None,
                timeout_millis: 2_000,
            })
            .await?;
        if !auth.allowed {
            return Err(ApplicationError::PermissionDenied);
        }
        if instrument.state() != PaymentInstrumentState::Revoked {
            let expected = instrument.version();
            instrument.revoke(self.clock.now_millis())?;
            let event = DomainEvent::PaymentInstrumentRevoked {
                event_id: self.ids.event_id()?,
                instrument_id: instrument.id().clone(),
                owner_id: owner.clone(),
                version: instrument.version(),
                occurred_at_millis: self.clock.now_millis(),
            };
            self.uow
                .save_instrument(
                    Commit {
                        value: instrument.clone(),
                        events: vec![event],
                    },
                    Some(expected),
                )
                .await
                .map_err(map_commit)?;
        }
        self.authz
            .delete_owner_grant(
                instrument.id().as_str(),
                &owner,
                &command.idempotency_key,
                2_000,
            )
            .await?;
        Ok(instrument_view(&instrument, auth.checked_at_token))
    }

    pub async fn reconcile_orphan_grants(&self, limit: usize) -> Result<usize, ApplicationError> {
        let instruments = self.uow.revoked_instruments(limit.min(500)).await?;
        let mut cleaned = 0;
        for instrument in instruments {
            match self
                .authz
                .delete_owner_grant(
                    instrument.id().as_str(),
                    instrument.owner_id(),
                    &format!("reconcile-revoked:{}", instrument.id()),
                    2_000,
                )
                .await
            {
                Ok(()) => cleaned += 1,
                Err(PortError::Timeout | PortError::Unavailable) => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(cleaned)
    }

    pub async fn attempt_payment(
        &self,
        command: AttemptPaymentCommand,
    ) -> Result<PaymentAttemptView, ApplicationError> {
        let decision: ReceiptDecision<PaymentAttemptView> = self
            .receipts
            .claim(
                &command.workload_id,
                "attempt_payment",
                &command.idempotency_key,
                &command,
            )
            .await?;
        let ReceiptDecision::Execute { token } = decision else {
            let ReceiptDecision::Replay(result) = decision else {
                unreachable!()
            };
            return Ok(result);
        };
        let result = self.attempt_payment_once(&command).await;
        match &result {
            Ok(value) => {
                if let Err(error) = self.receipts.complete(&token, value).await {
                    self.receipts.release(&token).await;
                    return Err(error);
                }
            }
            Err(_) => self.receipts.release(&token).await,
        }
        result
    }

    async fn attempt_payment_once(
        &self,
        command: &AttemptPaymentCommand,
    ) -> Result<PaymentAttemptView, ApplicationError> {
        let auction = AuctionId::parse(&command.auction_id)?;
        let round = SettlementRound::new(command.settlement_round)?;
        let candidate = CandidateId::parse(&command.candidate_id)?;
        let currency = command.currency_code.parse()?;
        let amount = Money::new(command.amount_minor_units, currency)?;
        let auth = self
            .authz
            .check(AuthorizationRequest {
                principal: command.workload_id.clone(),
                permission: Permission::ProcessPayment,
                resource_id: Some(auction.to_string()),
                consistency_token: command.consistency_token.clone(),
                timeout_millis: 2_000,
            })
            .await?;
        if !auth.allowed {
            return Err(ApplicationError::PermissionDenied);
        }
        let existing = self
            .uow
            .attempt_for_round(auction.as_str(), round.get())
            .await?;
        if let Some(existing) = &existing {
            if existing.candidate_id() != &candidate || existing.amount() != &amount {
                return Err(ApplicationError::IdempotencyConflict);
            }
            if existing.state() != PaymentAttemptState::Pending {
                return Ok(attempt_view(
                    existing,
                    existing.recovery_url().map(str::to_owned),
                    auth.checked_at_token,
                ));
            }
        }
        let instrument_owner = PrincipalId::parse(candidate.as_str())?;
        let instrument = self
            .uow
            .instrument_for_owner(&instrument_owner)
            .await?
            .ok_or(ApplicationError::NotFound)?;
        if instrument.state() != PaymentInstrumentState::Ready {
            return Err(ApplicationError::InvalidState);
        }
        let mut attempt = if let Some(existing) = existing {
            existing
        } else {
            let value = PaymentAttempt::pending(
                self.ids.attempt_id()?,
                auction.clone(),
                round,
                candidate,
                amount.clone(),
                self.clock.now_millis(),
            );
            let event = DomainEvent::PaymentAttemptStarted {
                event_id: self.ids.event_id()?,
                attempt_id: value.id().clone(),
                auction_id: auction.clone(),
                settlement_round: round,
                candidate_id: value.candidate_id().clone(),
                amount: amount.clone(),
                version: value.version(),
                occurred_at_millis: self.clock.now_millis(),
            };
            self.uow
                .save_attempt(
                    Commit {
                        value: value.clone(),
                        events: vec![event],
                    },
                    None,
                )
                .await
                .map_err(map_commit)?;
            value
        };
        let expected_version = attempt.version();
        let customer_ref = instrument
            .provider_customer_ref()
            .cloned()
            .ok_or(ApplicationError::InvalidState)?;
        let instrument_ref = instrument
            .provider_instrument_ref()
            .cloned()
            .ok_or(ApplicationError::InvalidState)?;
        let outcome = self
            .provider
            .charge(super::ChargeRequest {
                provider_key: format!("settlement:{auction}:{}", round.get()),
                customer_ref,
                instrument_ref,
                amount_minor_units: amount.minor_units(),
                currency_code: amount.currency().to_string(),
                timeout_millis: 8_000,
            })
            .await?;
        let (recovery_url, events) = match outcome {
            ProviderOutcome::Processing { operation_ref } => {
                attempt.mark_processing(operation_ref, self.clock.now_millis())?;
                (None, vec![])
            }
            ProviderOutcome::RecoveryRequired {
                operation_ref,
                recovery_url,
            } => {
                attempt.require_recovery(
                    operation_ref,
                    recovery_url.clone(),
                    self.clock.now_millis(),
                )?;
                let event = DomainEvent::PaymentRecoveryRequired {
                    event_id: self.ids.event_id()?,
                    attempt_id: attempt.id().clone(),
                    auction_id: auction.clone(),
                    settlement_round: round,
                    version: attempt.version(),
                    occurred_at_millis: self.clock.now_millis(),
                };
                (Some(recovery_url), vec![event])
            }
            ProviderOutcome::Succeeded { operation_ref } => {
                attempt.succeed(operation_ref, self.clock.now_millis())?;
                let event = DomainEvent::PaymentSucceeded {
                    event_id: self.ids.event_id()?,
                    attempt_id: attempt.id().clone(),
                    auction_id: auction.clone(),
                    settlement_round: round,
                    version: attempt.version(),
                    occurred_at_millis: self.clock.now_millis(),
                };
                (None, vec![event])
            }
            ProviderOutcome::Declined => return Err(ApplicationError::InvalidState),
        };
        self.uow
            .save_attempt(
                Commit {
                    value: attempt.clone(),
                    events,
                },
                Some(expected_version),
            )
            .await
            .map_err(map_commit)?;
        self.telemetry.command("attempt_payment", "success");
        Ok(attempt_view(&attempt, recovery_url, auth.checked_at_token))
    }

    pub async fn continue_payment(
        &self,
        command: ContinuePaymentCommand,
    ) -> Result<PaymentAttemptView, ApplicationError> {
        let decision: ReceiptDecision<PaymentAttemptView> = self
            .receipts
            .claim(
                &command.principal_id,
                "continue_payment",
                &command.idempotency_key,
                &command,
            )
            .await?;
        let ReceiptDecision::Execute { token } = decision else {
            let ReceiptDecision::Replay(result) = decision else {
                unreachable!()
            };
            return Ok(result);
        };
        let result = self.continue_payment_once(&command).await;
        match &result {
            Ok(value) => {
                if let Err(error) = self.receipts.complete(&token, value).await {
                    self.receipts.release(&token).await;
                    return Err(error);
                }
            }
            Err(_) => self.receipts.release(&token).await,
        }
        result
    }

    async fn continue_payment_once(
        &self,
        command: &ContinuePaymentCommand,
    ) -> Result<PaymentAttemptView, ApplicationError> {
        let principal = PrincipalId::parse(&command.principal_id)?;
        let id = PaymentAttemptId::parse(&command.attempt_id)?;
        let mut attempt = self
            .uow
            .attempt(&id)
            .await?
            .ok_or(ApplicationError::NotFound)?;
        if attempt.candidate_id().as_str() != principal.as_str() {
            return Err(ApplicationError::NotFound);
        }
        let auth = self
            .authz
            .check(AuthorizationRequest {
                principal: principal.to_string(),
                permission: Permission::ContinuePayment,
                resource_id: Some(id.to_string()),
                consistency_token: None,
                timeout_millis: 2_000,
            })
            .await?;
        if !auth.allowed {
            return Err(ApplicationError::PermissionDenied);
        }
        let operation = attempt
            .provider_operation_ref()
            .cloned()
            .ok_or(ApplicationError::InvalidState)?;
        let old_version = attempt.version();
        let outcome = self.provider.continue_payment(&operation, 8_000).await?;
        let mut events = Vec::new();
        match outcome {
            ProviderOutcome::Processing { operation_ref } => {
                attempt.mark_processing(operation_ref, self.clock.now_millis())?;
            }
            ProviderOutcome::RecoveryRequired {
                operation_ref,
                recovery_url,
            } => {
                attempt.require_recovery(operation_ref, recovery_url, self.clock.now_millis())?;
                events.push(DomainEvent::PaymentRecoveryRequired {
                    event_id: self.ids.event_id()?,
                    attempt_id: id,
                    auction_id: attempt.auction_id().clone(),
                    settlement_round: attempt.settlement_round(),
                    version: attempt.version(),
                    occurred_at_millis: self.clock.now_millis(),
                });
            }
            ProviderOutcome::Succeeded { operation_ref } => {
                attempt.succeed(operation_ref, self.clock.now_millis())?;
                events.push(DomainEvent::PaymentSucceeded {
                    event_id: self.ids.event_id()?,
                    attempt_id: id,
                    auction_id: attempt.auction_id().clone(),
                    settlement_round: attempt.settlement_round(),
                    version: attempt.version(),
                    occurred_at_millis: self.clock.now_millis(),
                });
            }
            ProviderOutcome::Declined => return Err(ApplicationError::InvalidState),
        }
        self.uow
            .save_attempt(
                Commit {
                    value: attempt.clone(),
                    events,
                },
                Some(old_version),
            )
            .await
            .map_err(map_commit)?;
        Ok(attempt_view(
            &attempt,
            attempt.recovery_url().map(str::to_owned),
            auth.checked_at_token,
        ))
    }

    pub async fn expire_payment(
        &self,
        command: ExpirePaymentCommand,
    ) -> Result<PaymentAttemptView, ApplicationError> {
        let decision: ReceiptDecision<PaymentAttemptView> = self
            .receipts
            .claim(
                &command.workload_id,
                "expire_payment",
                &command.idempotency_key,
                &command,
            )
            .await?;
        let ReceiptDecision::Execute { token } = decision else {
            let ReceiptDecision::Replay(result) = decision else {
                unreachable!()
            };
            return Ok(result);
        };
        let result = self.expire_payment_once(&command).await;
        match &result {
            Ok(value) => {
                if let Err(error) = self.receipts.complete(&token, value).await {
                    self.receipts.release(&token).await;
                    return Err(error);
                }
            }
            Err(_) => self.receipts.release(&token).await,
        }
        result
    }

    async fn expire_payment_once(
        &self,
        command: &ExpirePaymentCommand,
    ) -> Result<PaymentAttemptView, ApplicationError> {
        let id = PaymentAttemptId::parse(&command.attempt_id)?;
        let mut attempt = self
            .uow
            .attempt(&id)
            .await?
            .ok_or(ApplicationError::NotFound)?;
        let auth = self
            .authz
            .check(AuthorizationRequest {
                principal: command.workload_id.clone(),
                permission: Permission::ProcessPayment,
                resource_id: Some(attempt.auction_id().to_string()),
                consistency_token: command.consistency_token.clone(),
                timeout_millis: 2_000,
            })
            .await?;
        if !auth.allowed {
            return Err(ApplicationError::PermissionDenied);
        }
        if attempt.state() == PaymentAttemptState::Succeeded {
            return Err(ApplicationError::AlreadySucceeded);
        }
        let operation = attempt
            .provider_operation_ref()
            .cloned()
            .ok_or(ApplicationError::InvalidState)?;
        let old_version = attempt.version();
        let outcome = self.provider.query_or_cancel(&operation, 8_000).await?;
        let event = match outcome {
            ProviderOutcome::Succeeded { operation_ref } => {
                attempt.succeed(operation_ref, self.clock.now_millis())?;
                DomainEvent::PaymentSucceeded {
                    event_id: self.ids.event_id()?,
                    attempt_id: id,
                    auction_id: attempt.auction_id().clone(),
                    settlement_round: attempt.settlement_round(),
                    version: attempt.version(),
                    occurred_at_millis: self.clock.now_millis(),
                }
            }
            ProviderOutcome::Declined => {
                attempt.expire(self.clock.now_millis())?;
                DomainEvent::PaymentExpired {
                    event_id: self.ids.event_id()?,
                    attempt_id: id,
                    auction_id: attempt.auction_id().clone(),
                    settlement_round: attempt.settlement_round(),
                    version: attempt.version(),
                    occurred_at_millis: self.clock.now_millis(),
                }
            }
            ProviderOutcome::Processing { .. } | ProviderOutcome::RecoveryRequired { .. } => {
                return Err(ApplicationError::DependencyUnavailable);
            }
        };
        self.uow
            .save_attempt(
                Commit {
                    value: attempt.clone(),
                    events: vec![event],
                },
                Some(old_version),
            )
            .await
            .map_err(map_commit)?;
        Ok(attempt_view(&attempt, None, auth.checked_at_token))
    }

    pub async fn handle_provider_event(
        &self,
        event: VerifiedProviderEvent,
    ) -> Result<(), ApplicationError> {
        let event_digest = event.event_ref_digest.clone();
        match self
            .callbacks
            .claim(
                event.provider,
                &event_digest,
                &event.fingerprint,
                self.clock.now_millis().saturating_add(30_000),
            )
            .await?
        {
            CallbackClaim::Replay => return Ok(()),
            CallbackClaim::InProgress => return Err(ApplicationError::InProgress),
            CallbackClaim::Conflict => return Err(ApplicationError::IdempotencyConflict),
            CallbackClaim::Acquired => {}
        }
        match event.outcome {
            VerifiedProviderOutcome::InstrumentReady {
                session_ref,
                customer_ref,
                instrument_ref,
                display,
            } => {
                let mut instrument = self
                    .uow
                    .instrument_by_session_ref(&session_ref)
                    .await?
                    .ok_or(ApplicationError::NotFound)?;
                if instrument.state() == PaymentInstrumentState::Ready {
                    if instrument.provider_session_ref() == &session_ref
                        && instrument.provider_customer_ref() == Some(&customer_ref)
                        && instrument.provider_instrument_ref() == Some(&instrument_ref)
                        && instrument.display() == Some(&display)
                    {
                        self.callbacks
                            .complete(event.provider, &event_digest)
                            .await?;
                        return Ok(());
                    }
                    return Err(ApplicationError::InvalidState);
                }
                let expected = instrument.version();
                instrument.mark_ready(
                    customer_ref,
                    instrument_ref,
                    display,
                    event.occurred_at_millis,
                )?;
                let domain_event = DomainEvent::PaymentInstrumentReady {
                    event_id: self.ids.event_id()?,
                    instrument_id: instrument.id().clone(),
                    owner_id: instrument.owner_id().clone(),
                    version: instrument.version(),
                    occurred_at_millis: event.occurred_at_millis,
                };
                self.uow
                    .save_instrument(
                        Commit {
                            value: instrument,
                            events: vec![domain_event],
                        },
                        Some(expected),
                    )
                    .await
                    .map_err(map_commit)?;
            }
            VerifiedProviderOutcome::AttemptProcessing { operation_ref } => {
                self.apply_attempt_callback(operation_ref, event.occurred_at_millis, false)
                    .await?;
            }
            VerifiedProviderOutcome::AttemptRecoveryRequired {
                operation_ref,
                recovery_url,
            } => {
                self.apply_attempt_recovery(operation_ref, recovery_url, event.occurred_at_millis)
                    .await?;
            }
            VerifiedProviderOutcome::AttemptSucceeded { operation_ref } => {
                self.apply_attempt_callback(operation_ref, event.occurred_at_millis, true)
                    .await?;
            }
        }
        self.callbacks
            .complete(event.provider, &event_digest)
            .await?;
        Ok(())
    }

    pub async fn reconcile_stale_attempts(
        &self,
        stale_before_millis: u64,
        limit: usize,
    ) -> Result<usize, ApplicationError> {
        let attempts = self
            .uow
            .stale_attempts(stale_before_millis, limit.min(500))
            .await?;
        let mut reconciled = 0;
        for mut attempt in attempts {
            let Some(operation_ref) = attempt.provider_operation_ref().cloned() else {
                continue;
            };
            let expected = attempt.version();
            let outcome = self.provider.query_or_cancel(&operation_ref, 8_000).await?;
            let events = match outcome {
                ProviderOutcome::Processing { .. } => continue,
                ProviderOutcome::RecoveryRequired {
                    operation_ref,
                    recovery_url,
                } => {
                    attempt.require_recovery(
                        operation_ref,
                        recovery_url,
                        self.clock.now_millis(),
                    )?;
                    vec![DomainEvent::PaymentRecoveryRequired {
                        event_id: self.ids.event_id()?,
                        attempt_id: attempt.id().clone(),
                        auction_id: attempt.auction_id().clone(),
                        settlement_round: attempt.settlement_round(),
                        version: attempt.version(),
                        occurred_at_millis: self.clock.now_millis(),
                    }]
                }
                ProviderOutcome::Succeeded { operation_ref } => {
                    attempt.succeed(operation_ref, self.clock.now_millis())?;
                    vec![DomainEvent::PaymentSucceeded {
                        event_id: self.ids.event_id()?,
                        attempt_id: attempt.id().clone(),
                        auction_id: attempt.auction_id().clone(),
                        settlement_round: attempt.settlement_round(),
                        version: attempt.version(),
                        occurred_at_millis: self.clock.now_millis(),
                    }]
                }
                ProviderOutcome::Declined => continue,
            };
            match self
                .uow
                .save_attempt(
                    Commit {
                        value: attempt,
                        events,
                    },
                    Some(expected),
                )
                .await
            {
                Ok(()) => reconciled += 1,
                Err(CommitConflict::Version) => continue,
                Err(error) => return Err(map_commit(error)),
            }
        }
        Ok(reconciled)
    }

    async fn apply_attempt_callback(
        &self,
        operation_ref: crate::domain::ProviderReference,
        now: u64,
        succeeded: bool,
    ) -> Result<(), ApplicationError> {
        let mut attempt = self
            .uow
            .attempt_by_operation_ref(&operation_ref)
            .await?
            .ok_or(ApplicationError::NotFound)?;
        if succeeded && attempt.state() == PaymentAttemptState::Succeeded {
            return Ok(());
        }
        if !succeeded
            && (attempt.state() == PaymentAttemptState::Processing
                && attempt.provider_operation_ref() == Some(&operation_ref)
                || matches!(
                    attempt.state(),
                    PaymentAttemptState::Succeeded | PaymentAttemptState::Expired
                ))
        {
            return Ok(());
        }
        let expected = attempt.version();
        let events = if succeeded {
            attempt.succeed(operation_ref, now)?;
            vec![DomainEvent::PaymentSucceeded {
                event_id: self.ids.event_id()?,
                attempt_id: attempt.id().clone(),
                auction_id: attempt.auction_id().clone(),
                settlement_round: attempt.settlement_round(),
                version: attempt.version(),
                occurred_at_millis: now,
            }]
        } else {
            attempt.mark_processing(operation_ref, now)?;
            vec![]
        };
        self.uow
            .save_attempt(
                Commit {
                    value: attempt,
                    events,
                },
                Some(expected),
            )
            .await
            .map_err(map_commit)
    }

    async fn apply_attempt_recovery(
        &self,
        operation_ref: crate::domain::ProviderReference,
        recovery_url: String,
        now: u64,
    ) -> Result<(), ApplicationError> {
        let mut attempt = self
            .uow
            .attempt_by_operation_ref(&operation_ref)
            .await?
            .ok_or(ApplicationError::NotFound)?;
        if (attempt.state() == PaymentAttemptState::RecoveryRequired
            && attempt.provider_operation_ref() == Some(&operation_ref)
            && attempt.recovery_url() == Some(recovery_url.as_str()))
            || matches!(
                attempt.state(),
                PaymentAttemptState::Succeeded | PaymentAttemptState::Expired
            )
        {
            return Ok(());
        }
        let expected = attempt.version();
        attempt.require_recovery(operation_ref, recovery_url, now)?;
        let event = DomainEvent::PaymentRecoveryRequired {
            event_id: self.ids.event_id()?,
            attempt_id: attempt.id().clone(),
            auction_id: attempt.auction_id().clone(),
            settlement_round: attempt.settlement_round(),
            version: attempt.version(),
            occurred_at_millis: now,
        };
        self.uow
            .save_attempt(
                Commit {
                    value: attempt,
                    events: vec![event],
                },
                Some(expected),
            )
            .await
            .map_err(map_commit)
    }
}

fn instrument_view(value: &PaymentInstrument, checked_at_token: String) -> PaymentInstrumentView {
    PaymentInstrumentView {
        id: value.id().to_string(),
        status: value.state(),
        display: value.display().map(|v| v.as_str().to_owned()),
        version: value.version(),
        checked_at_token,
    }
}
fn attempt_view(
    value: &PaymentAttempt,
    recovery_url: Option<String>,
    checked_at_token: String,
) -> PaymentAttemptView {
    PaymentAttemptView {
        id: value.id().to_string(),
        auction_id: value.auction_id().to_string(),
        status: value.state(),
        recovery_url,
        version: value.version(),
        checked_at_token,
    }
}
fn map_commit(error: CommitConflict) -> ApplicationError {
    match error {
        CommitConflict::SuccessForAuction => ApplicationError::AlreadySucceeded,
        CommitConflict::AttemptForRound => ApplicationError::IdempotencyConflict,
        CommitConflict::Version => ApplicationError::InvalidState,
    }
}
