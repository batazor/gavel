#![allow(clippy::result_large_err)]

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use tonic::{Request, Response, Status};

use crate::{
    application::{ApplicationError, NotificationView, NotifyApplication},
    domain::{NotificationCommand, NotificationStatus, Template},
    rpc::{
        self, get_notification_response, notify_operator_service_server::NotifyOperatorService,
        notify_service_server::NotifyService, send_notification_response,
    },
};

#[derive(Clone, Debug)]
pub struct AuthenticatedWorkload {
    pub spiffe_id: String,
}

#[derive(Clone, Debug)]
pub struct AuthenticatedOperator {
    pub principal: String,
}

pub fn authenticate_workload(mut request: Request<()>) -> Result<Request<()>, Status> {
    let spiffe_id = authenticated_metadata(&request, "x-gavel-spiffe-id")?;
    request
        .extensions_mut()
        .insert(AuthenticatedWorkload { spiffe_id });
    Ok(request)
}

pub fn authenticate_operator(mut request: Request<()>) -> Result<Request<()>, Status> {
    let principal = authenticated_metadata(&request, "x-gavel-operator-principal")?;
    request
        .extensions_mut()
        .insert(AuthenticatedOperator { principal });
    Ok(request)
}

fn authenticated_metadata<T>(request: &Request<T>, key: &'static str) -> Result<String, Status> {
    request
        .metadata()
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| value.starts_with("spiffe://") && value.len() <= 256)
        .map(ToOwned::to_owned)
        .ok_or_else(|| Status::unauthenticated("authenticated caller identity is required"))
}

pub struct GrpcNotify {
    application: Arc<NotifyApplication>,
    allowed_processes_spiffe_id: String,
    allowed_operator_principal: String,
    max_deadline: Duration,
}

impl GrpcNotify {
    pub fn new(
        application: Arc<NotifyApplication>,
        allowed_processes_spiffe_id: String,
        allowed_operator_principal: String,
        max_deadline: Duration,
    ) -> Self {
        Self {
            application,
            allowed_processes_spiffe_id,
            allowed_operator_principal,
            max_deadline,
        }
    }
    fn deadline<T>(&self, request: &Request<T>) -> Result<(), Status> {
        let deadline = request
            .metadata()
            .get("grpc-timeout")
            .and_then(|v| v.to_str().ok())
            .and_then(parse_grpc_timeout)
            .ok_or_else(|| Status::invalid_argument("deadline is required"))?;
        if deadline > self.max_deadline {
            Err(Status::invalid_argument("deadline exceeds maximum"))
        } else {
            Ok(())
        }
    }
    fn processes<T>(&self, request: &Request<T>) -> Result<(), Status> {
        self.deadline(request)?;
        authorize_processes(request, &self.allowed_processes_spiffe_id)
    }
    fn operator<T>(&self, request: &Request<T>) -> Result<(), Status> {
        self.deadline(request)?;
        authorize_operator(request, &self.allowed_operator_principal)
    }
}

fn authorize_processes<T>(request: &Request<T>, expected: &str) -> Result<(), Status> {
    let identity = request
        .extensions()
        .get::<AuthenticatedWorkload>()
        .ok_or_else(|| Status::unauthenticated("authenticated workload identity is required"))?;
    if identity.spiffe_id == expected {
        Ok(())
    } else {
        Err(Status::permission_denied("workload is not authorized"))
    }
}

fn authorize_operator<T>(request: &Request<T>, expected: &str) -> Result<(), Status> {
    let identity = request
        .extensions()
        .get::<AuthenticatedOperator>()
        .ok_or_else(|| Status::unauthenticated("authenticated operator identity is required"))?;
    if identity.principal == expected {
        Ok(())
    } else {
        Err(Status::permission_denied("operator is not authorized"))
    }
}

#[tonic::async_trait]
impl NotifyService for GrpcNotify {
    async fn send_notification(
        &self,
        request: Request<rpc::SendNotificationRequest>,
    ) -> Result<Response<rpc::SendNotificationResponse>, Status> {
        self.processes(&request)?;
        let input = request.into_inner();
        if input.template_version != 1 {
            return Ok(Response::new(rejected(ApplicationError::InvalidCommand)));
        }
        let template = match rpc::NotificationTemplate::try_from(input.template) {
            Ok(rpc::NotificationTemplate::SettlementCandidate) => Template::SettlementCandidateV1,
            Ok(rpc::NotificationTemplate::PaymentRecoveryRequired) => {
                Template::PaymentRecoveryRequiredV1
            }
            Ok(rpc::NotificationTemplate::WinnerConfirmed) => Template::WinnerConfirmedV1,
            Ok(rpc::NotificationTemplate::SettlementCandidateFailed) => {
                Template::SettlementCandidateFailedV1
            }
            _ => return Ok(Response::new(rejected(ApplicationError::InvalidCommand))),
        };
        let result = self
            .application
            .send(NotificationCommand {
                notification_id: input.notification_id,
                recipient_id: input.recipient_id,
                template,
                data: input.data.into_iter().collect::<BTreeMap<_, _>>(),
                idempotency_key: input.idempotency_key,
            })
            .await;
        Ok(Response::new(match result {
            Ok(view) => accepted(view),
            Err(error) => rejected(error),
        }))
    }
}

#[tonic::async_trait]
impl NotifyOperatorService for GrpcNotify {
    async fn get_notification(
        &self,
        request: Request<rpc::GetNotificationRequest>,
    ) -> Result<Response<rpc::GetNotificationResponse>, Status> {
        self.operator(&request)?;
        let id = request.into_inner().notification_id;
        if id.is_empty() || id.len() > 160 {
            return Ok(Response::new(operator_rejected(
                ApplicationError::InvalidCommand,
            )));
        }
        let response = match self.application.get(&id).await {
            Ok(Some(view)) => rpc::GetNotificationResponse {
                outcome: Some(get_notification_response::Outcome::Result(record(view))),
            },
            Ok(None) => operator_rejected(ApplicationError::NotFound),
            Err(error) => operator_rejected(error),
        };
        Ok(Response::new(response))
    }
}

fn accepted(view: NotificationView) -> rpc::SendNotificationResponse {
    rpc::SendNotificationResponse {
        outcome: Some(send_notification_response::Outcome::Accepted(
            rpc::NotificationAccepted {
                status: rpc_status(view.status) as i32,
                notification_id: view.notification_id,
                template_version: 1,
                attempt_count: view.attempts,
                version: view.version,
            },
        )),
    }
}
fn record(view: NotificationView) -> rpc::NotificationRecord {
    rpc::NotificationRecord {
        notification_id: view.notification_id,
        recipient_id: view.recipient_id,
        template: rpc_template(&view.template) as i32,
        template_version: 1,
        status: rpc_status(view.status) as i32,
        attempt_count: view.attempts,
        version: view.version,
    }
}
fn rejected(error: ApplicationError) -> rpc::SendNotificationResponse {
    rpc::SendNotificationResponse {
        outcome: Some(send_notification_response::Outcome::Rejection(rejection(
            error,
        ))),
    }
}
fn operator_rejected(error: ApplicationError) -> rpc::GetNotificationResponse {
    rpc::GetNotificationResponse {
        outcome: Some(get_notification_response::Outcome::Rejection(rejection(
            error,
        ))),
    }
}
fn rejection(error: ApplicationError) -> rpc::DomainRejection {
    rpc::DomainRejection {
        reason: error_code(error).into(),
        safe_message: error.to_string(),
        code: rejection_code(error) as i32,
    }
}
fn error_code(error: ApplicationError) -> &'static str {
    match error {
        ApplicationError::IdempotencyConflict => "idempotency_conflict",
        ApplicationError::InvalidCommand => "invalid_command",
        ApplicationError::InvalidTemplateData => "invalid_template_data",
        ApplicationError::InProgress => "in_progress",
        ApplicationError::DependencyUnavailable => "dependency_unavailable",
        ApplicationError::NotFound => "not_found",
        ApplicationError::InvalidConfiguration | ApplicationError::Internal => "internal",
    }
}
fn rejection_code(error: ApplicationError) -> rpc::NotificationRejectionCode {
    match error {
        ApplicationError::InvalidCommand | ApplicationError::InvalidTemplateData => {
            rpc::NotificationRejectionCode::InvalidArgument
        }
        ApplicationError::IdempotencyConflict => {
            rpc::NotificationRejectionCode::IdempotencyConflict
        }
        ApplicationError::InProgress => rpc::NotificationRejectionCode::InProgress,
        ApplicationError::NotFound => rpc::NotificationRejectionCode::NotFound,
        ApplicationError::DependencyUnavailable
        | ApplicationError::InvalidConfiguration
        | ApplicationError::Internal => rpc::NotificationRejectionCode::Internal,
    }
}
fn rpc_status(value: NotificationStatus) -> rpc::NotificationStatus {
    match value {
        NotificationStatus::Pending => rpc::NotificationStatus::Pending,
        NotificationStatus::Sent => rpc::NotificationStatus::Sent,
        NotificationStatus::Failed => rpc::NotificationStatus::Failed,
    }
}
fn rpc_template(reference: &str) -> rpc::NotificationTemplate {
    match reference {
        "settlement_candidate.v1" => rpc::NotificationTemplate::SettlementCandidate,
        "payment_recovery_required.v1" => rpc::NotificationTemplate::PaymentRecoveryRequired,
        "winner_confirmed.v1" => rpc::NotificationTemplate::WinnerConfirmed,
        "settlement_candidate_failed.v1" => rpc::NotificationTemplate::SettlementCandidateFailed,
        _ => rpc::NotificationTemplate::Unspecified,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_bounded_deadline() {
        assert_eq!(
            parse_grpc_timeout("1500m"),
            Some(Duration::from_millis(1500))
        );
        assert_eq!(parse_grpc_timeout("oops"), None);
    }

    #[test]
    fn processes_authentication_fails_closed() {
        let request = Request::new(());
        assert_eq!(
            authorize_processes(&request, "spiffe://processes")
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );
        let mut request = Request::new(());
        request.extensions_mut().insert(AuthenticatedWorkload {
            spiffe_id: "spiffe://other".into(),
        });
        assert_eq!(
            authorize_processes(&request, "spiffe://processes")
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[test]
    fn operator_authentication_is_separate_from_processes() {
        let mut request = Request::new(());
        request.extensions_mut().insert(AuthenticatedOperator {
            principal: "spiffe://smoke".into(),
        });
        assert!(authorize_operator(&request, "spiffe://smoke").is_ok());
        assert!(authorize_processes(&request, "spiffe://processes").is_err());
    }
}
