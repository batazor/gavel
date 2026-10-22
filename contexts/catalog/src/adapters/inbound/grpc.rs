#![allow(clippy::result_large_err)]

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use tonic::{Request, Response, Status};

use crate::{
    application::{
        ApplicationError, Clock, CommandContext, LotMutationResult, WithdrawLotInput,
        WorkloadIdentity,
    },
    rpc::{self, catalog_service_server::CatalogService},
};

use super::error_mapping::{RpcError, rpc_error};

const CALLER_SCOPE: &str = "service-account:processes";
const MAX_IDENTIFIER_LENGTH: usize = 128;
const MAX_CORRELATION_ID_LENGTH: usize = 256;

#[async_trait]
pub trait CatalogGrpcApi: Send + Sync {
    async fn withdraw(
        &self,
        input: WithdrawLotInput,
    ) -> Result<LotMutationResult, ApplicationError>;
}

pub struct CatalogGrpcService {
    api: Arc<dyn CatalogGrpcApi>,
    clock: Arc<dyn Clock>,
    max_request_timeout: Duration,
}

impl CatalogGrpcService {
    pub fn new(
        api: Arc<dyn CatalogGrpcApi>,
        clock: Arc<dyn Clock>,
        max_request_timeout: Duration,
    ) -> Result<Self, &'static str> {
        if max_request_timeout.is_zero() {
            return Err("max request timeout must be positive");
        }
        Ok(Self {
            api,
            clock,
            max_request_timeout,
        })
    }
}

#[tonic::async_trait]
impl CatalogService for CatalogGrpcService {
    async fn withdraw_lot(
        &self,
        request: Request<rpc::WithdrawLotRequest>,
    ) -> Result<Response<rpc::WithdrawLotResponse>, Status> {
        let timeout = request_timeout(&request, self.max_request_timeout)?;
        let caller = request
            .extensions()
            .get::<WorkloadIdentity>()
            .copied()
            .ok_or_else(|| Status::unauthenticated("authentication is required"))?;
        if caller != WorkloadIdentity::Processes {
            return Err(Status::permission_denied(
                "processes workload identity is required",
            ));
        }

        let request = request.into_inner();
        validate_required(&request.lot_id, "lot_id", MAX_IDENTIFIER_LENGTH)?;
        validate_required(
            &request.idempotency_key,
            "idempotency_key",
            MAX_IDENTIFIER_LENGTH,
        )?;
        validate_required(
            &request.correlation_id,
            "correlation_id",
            MAX_CORRELATION_ID_LENGTH,
        )?;
        if request.consistency_token.len() > MAX_CORRELATION_ID_LENGTH {
            return Err(Status::invalid_argument("consistency_token is too long"));
        }

        let input = WithdrawLotInput {
            context: CommandContext {
                caller_scope: CALLER_SCOPE.to_owned(),
                principal: None,
                idempotency_key: request.idempotency_key,
                correlation_id: request.correlation_id,
                consistency_token: non_empty(request.consistency_token),
            },
            caller,
            lot_id: request.lot_id,
            occurred_at_millis: self.clock.now_millis(),
        };

        let result = tokio::time::timeout(timeout, self.api.withdraw(input))
            .await
            .map_err(|_| Status::deadline_exceeded("request deadline exceeded"))?;
        match result {
            Ok(result) => Ok(Response::new(rpc::WithdrawLotResponse {
                outcome: Some(rpc::withdraw_lot_response::Outcome::Withdrawn(
                    rpc::LotWithdrawn {
                        version: result.version,
                        checked_at_token: result.checked_at_token,
                    },
                )),
            })),
            Err(error) => mutation_error(error),
        }
    }
}

fn mutation_error(error: ApplicationError) -> Result<Response<rpc::WithdrawLotResponse>, Status> {
    match rpc_error(error) {
        RpcError::Rejection(rejection) => Ok(Response::new(rpc::WithdrawLotResponse {
            outcome: Some(rpc::withdraw_lot_response::Outcome::Rejection(rejection)),
        })),
        RpcError::Status(status) => Err(status),
    }
}

fn request_timeout<T>(request: &Request<T>, maximum: Duration) -> Result<Duration, Status> {
    let value = request
        .metadata()
        .get("grpc-timeout")
        .ok_or_else(|| Status::invalid_argument("request deadline is required"))?
        .to_str()
        .map_err(|_| Status::invalid_argument("request deadline is invalid"))?;
    if value.len() < 2 || value.len() > 9 {
        return Err(Status::invalid_argument("request deadline is invalid"));
    }
    let (amount, unit) = value.split_at(value.len() - 1);
    let amount = amount
        .parse::<u64>()
        .ok()
        .filter(|amount| *amount > 0)
        .ok_or_else(|| Status::invalid_argument("request deadline is invalid"))?;
    let duration = match unit {
        "H" => Duration::from_secs(amount.saturating_mul(3_600)),
        "M" => Duration::from_secs(amount.saturating_mul(60)),
        "S" => Duration::from_secs(amount),
        "m" => Duration::from_millis(amount),
        "u" => Duration::from_micros(amount),
        "n" => Duration::from_nanos(amount),
        _ => return Err(Status::invalid_argument("request deadline is invalid")),
    };
    if duration > maximum {
        return Err(Status::invalid_argument(
            "request deadline exceeds the service limit",
        ));
    }
    Ok(duration)
}

fn validate_required(value: &str, field: &'static str, maximum: usize) -> Result<(), Status> {
    if value.trim().is_empty() {
        return Err(Status::invalid_argument(format!("{field} is required")));
    }
    if value.len() > maximum {
        return Err(Status::invalid_argument(format!("{field} is too long")));
    }
    Ok(())
}

fn non_empty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use tonic::Code;

    use super::*;

    struct FixedClock;

    impl Clock for FixedClock {
        fn now_millis(&self) -> u64 {
            42
        }
    }

    struct FakeApi {
        result: Mutex<Result<LotMutationResult, ApplicationError>>,
        captured: Mutex<Vec<WithdrawLotInput>>,
        delay: Duration,
    }

    #[async_trait]
    impl CatalogGrpcApi for FakeApi {
        async fn withdraw(
            &self,
            input: WithdrawLotInput,
        ) -> Result<LotMutationResult, ApplicationError> {
            self.captured.lock().unwrap().push(input);
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            self.result.lock().unwrap().clone()
        }
    }

    fn api(result: Result<LotMutationResult, ApplicationError>) -> Arc<FakeApi> {
        Arc::new(FakeApi {
            result: Mutex::new(result),
            captured: Mutex::new(Vec::new()),
            delay: Duration::ZERO,
        })
    }

    fn service(api: Arc<FakeApi>) -> CatalogGrpcService {
        CatalogGrpcService::new(api, Arc::new(FixedClock), Duration::from_secs(2)).unwrap()
    }

    fn request() -> Request<rpc::WithdrawLotRequest> {
        let mut request = Request::new(rpc::WithdrawLotRequest {
            lot_id: "lot-1".to_owned(),
            idempotency_key: "idem-1".to_owned(),
            correlation_id: "corr-1".to_owned(),
            consistency_token: "token-1".to_owned(),
        });
        request
            .metadata_mut()
            .insert("grpc-timeout", "1S".parse().unwrap());
        request.extensions_mut().insert(WorkloadIdentity::Processes);
        request
    }

    fn success() -> LotMutationResult {
        LotMutationResult {
            lot_id: "lot-1".to_owned(),
            version: 7,
            checked_at_token: "checked-7".to_owned(),
        }
    }

    #[tokio::test]
    async fn maps_trusted_request_and_success_without_accepting_spoofed_identity_headers() {
        let api = api(Ok(success()));
        let service = service(Arc::clone(&api));
        let mut request = request();
        request
            .metadata_mut()
            .insert("x-workload", "untrusted".parse().unwrap());
        let response = service.withdraw_lot(request).await.unwrap().into_inner();
        assert_eq!(
            response.outcome,
            Some(rpc::withdraw_lot_response::Outcome::Withdrawn(
                rpc::LotWithdrawn {
                    version: 7,
                    checked_at_token: "checked-7".to_owned(),
                }
            ))
        );
        let captured = api.captured.lock().unwrap();
        assert_eq!(captured[0].caller, WorkloadIdentity::Processes);
        assert_eq!(captured[0].context.caller_scope, CALLER_SCOPE);
        assert_eq!(
            captured[0].context.consistency_token.as_deref(),
            Some("token-1")
        );
        assert_eq!(captured[0].occurred_at_millis, 42);
    }

    #[tokio::test]
    async fn returns_domain_rejections_in_the_response_and_dependency_failures_as_statuses() {
        let response = service(api(Err(ApplicationError::Forbidden)))
            .withdraw_lot(request())
            .await
            .unwrap()
            .into_inner();
        assert!(matches!(
            response.outcome,
            Some(rpc::withdraw_lot_response::Outcome::Rejection(_))
        ));

        let error = service(api(Err(ApplicationError::DependencyUnavailable {
            dependency: "authorization.secret.internal",
        })))
        .withdraw_lot(request())
        .await
        .unwrap_err();
        assert_eq!(error.code(), Code::Unavailable);
        assert!(!error.message().contains("secret.internal"));
    }

    #[tokio::test]
    async fn fails_closed_for_identity_deadline_and_input_validation() {
        let service = service(api(Ok(success())));
        let mut missing_identity = request();
        missing_identity
            .extensions_mut()
            .remove::<WorkloadIdentity>();
        assert_eq!(
            service
                .withdraw_lot(missing_identity)
                .await
                .unwrap_err()
                .code(),
            Code::Unauthenticated
        );

        let mut untrusted = request();
        untrusted
            .extensions_mut()
            .insert(WorkloadIdentity::Untrusted);
        assert_eq!(
            service.withdraw_lot(untrusted).await.unwrap_err().code(),
            Code::PermissionDenied
        );

        let mut no_deadline = request();
        no_deadline.metadata_mut().remove("grpc-timeout");
        assert_eq!(
            service.withdraw_lot(no_deadline).await.unwrap_err().code(),
            Code::InvalidArgument
        );

        let mut too_long = request();
        too_long.get_mut().lot_id = "x".repeat(MAX_IDENTIFIER_LENGTH + 1);
        assert_eq!(
            service.withdraw_lot(too_long).await.unwrap_err().code(),
            Code::InvalidArgument
        );
    }

    #[tokio::test]
    async fn enforces_the_transport_deadline() {
        let api = Arc::new(FakeApi {
            result: Mutex::new(Ok(success())),
            captured: Mutex::new(Vec::new()),
            delay: Duration::from_millis(25),
        });
        let service = service(api);
        let mut request = request();
        request
            .metadata_mut()
            .insert("grpc-timeout", "1m".parse().unwrap());
        assert_eq!(
            service.withdraw_lot(request).await.unwrap_err().code(),
            Code::DeadlineExceeded
        );
    }
}
