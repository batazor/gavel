use std::{sync::Arc, time::Duration};

use tonic_bsr::{Request, Response, Status};

use crate::{
    application::{
        ApplicationError, AttemptPaymentCommand, ExpirePaymentCommand, PaymentApplication,
        PaymentAttemptView,
    },
    rpc::{
        self, attempt_payment_response, expire_payment_response,
        payments_service_server::PaymentsService,
    },
};

#[derive(Clone, Debug)]
pub struct AuthenticatedWorkload {
    pub spiffe_id: String,
}

pub struct GrpcPayments {
    application: Arc<PaymentApplication>,
    allowed_processes_spiffe_id: String,
    max_deadline: Duration,
}
impl GrpcPayments {
    pub fn new(
        application: Arc<PaymentApplication>,
        allowed_processes_spiffe_id: String,
        max_deadline: Duration,
    ) -> Self {
        Self {
            application,
            allowed_processes_spiffe_id,
            max_deadline,
        }
    }
    #[allow(clippy::result_large_err)]
    fn authenticate<T>(&self, request: &Request<T>) -> Result<String, Status> {
        let workload = request
            .extensions()
            .get::<AuthenticatedWorkload>()
            .ok_or_else(|| {
                Status::unauthenticated("authenticated workload identity is required")
            })?;
        if workload.spiffe_id != self.allowed_processes_spiffe_id {
            return Err(Status::permission_denied("workload is not authorized"));
        }
        let deadline = request
            .metadata()
            .get("grpc-timeout")
            .and_then(|value| value.to_str().ok())
            .and_then(parse_grpc_timeout)
            .ok_or_else(|| Status::invalid_argument("deadline is required"))?;
        if deadline > self.max_deadline {
            return Err(Status::invalid_argument("deadline exceeds maximum"));
        }
        Ok(workload.spiffe_id.clone())
    }
}

#[tonic_bsr::async_trait]
impl PaymentsService for GrpcPayments {
    async fn attempt_payment(
        &self,
        request: Request<rpc::AttemptPaymentRequest>,
    ) -> Result<Response<rpc::AttemptPaymentResponse>, Status> {
        let workload_id = self.authenticate(&request)?;
        let input = request.into_inner();
        let amount = input
            .amount
            .ok_or_else(|| Status::invalid_argument("amount is required"))?;
        let outcome = self
            .application
            .attempt_payment(AttemptPaymentCommand {
                workload_id,
                auction_id: input.auction_id,
                settlement_round: input.settlement_round,
                candidate_id: input.candidate_id,
                amount_minor_units: amount.minor_units,
                currency_code: amount.currency_code,
                idempotency_key: input.idempotency_key,
                consistency_token: non_empty(input.consistency_token),
            })
            .await;
        Ok(Response::new(rpc::AttemptPaymentResponse {
            outcome: Some(match outcome {
                Ok(view) => attempt_payment_response::Outcome::Result(to_rpc_result(view)),
                Err(error) => attempt_payment_response::Outcome::Rejection(rejection(error)),
            }),
        }))
    }

    async fn expire_payment(
        &self,
        request: Request<rpc::ExpirePaymentRequest>,
    ) -> Result<Response<rpc::ExpirePaymentResponse>, Status> {
        let workload_id = self.authenticate(&request)?;
        let input = request.into_inner();
        let outcome = self
            .application
            .expire_payment(ExpirePaymentCommand {
                workload_id,
                attempt_id: input.payment_attempt_id,
                idempotency_key: input.idempotency_key,
                consistency_token: non_empty(input.consistency_token),
            })
            .await;
        Ok(Response::new(rpc::ExpirePaymentResponse {
            outcome: Some(match outcome {
                Ok(view) if view.status == crate::domain::PaymentAttemptState::Expired => {
                    expire_payment_response::Outcome::Expired(rpc::PaymentExpired {
                        payment_attempt_id: view.id,
                        version: view.version,
                        checked_at_token: view.checked_at_token,
                    })
                }
                Ok(_) => expire_payment_response::Outcome::Rejection(rejection(
                    ApplicationError::AlreadySucceeded,
                )),
                Err(error) => expire_payment_response::Outcome::Rejection(rejection(error)),
            }),
        }))
    }
}

fn to_rpc_result(value: PaymentAttemptView) -> rpc::PaymentAttemptResult {
    let status = match value.status {
        crate::domain::PaymentAttemptState::Pending => rpc::PaymentAttemptStatus::Unspecified,
        crate::domain::PaymentAttemptState::Processing => rpc::PaymentAttemptStatus::Processing,
        crate::domain::PaymentAttemptState::RecoveryRequired => {
            rpc::PaymentAttemptStatus::RecoveryRequired
        }
        crate::domain::PaymentAttemptState::Succeeded => rpc::PaymentAttemptStatus::Succeeded,
        crate::domain::PaymentAttemptState::Expired => rpc::PaymentAttemptStatus::Expired,
    };
    rpc::PaymentAttemptResult {
        payment_attempt_id: value.id,
        status: status as i32,
        recovery_url: value.recovery_url.unwrap_or_default(),
        version: value.version,
        checked_at_token: value.checked_at_token,
    }
}
fn rejection(error: ApplicationError) -> rpc::DomainRejection {
    let code = match error {
        ApplicationError::NotFound => rpc::PaymentRejectionCode::NotFound,
        ApplicationError::PermissionDenied => rpc::PaymentRejectionCode::PermissionDenied,
        ApplicationError::IdempotencyConflict => rpc::PaymentRejectionCode::IdempotencyConflict,
        ApplicationError::AlreadySucceeded => rpc::PaymentRejectionCode::AlreadySucceeded,
        _ => rpc::PaymentRejectionCode::InvalidState,
    };
    rpc::DomainRejection {
        reason: format!("{:?}", error.code()).to_ascii_lowercase(),
        safe_message: error.to_string(),
        code: code as i32,
    }
}
fn non_empty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

fn parse_grpc_timeout(value: &str) -> Option<Duration> {
    let (number, unit) = value.split_at(value.len().checked_sub(1)?);
    let number = number.parse::<u64>().ok()?;
    match unit {
        "H" => Some(Duration::from_secs(number.checked_mul(3600)?)),
        "M" => Some(Duration::from_secs(number.checked_mul(60)?)),
        "S" => Some(Duration::from_secs(number)),
        "m" => Some(Duration::from_millis(number)),
        "u" => Some(Duration::from_micros(number)),
        "n" => Some(Duration::from_nanos(number)),
        _ => None,
    }
}
