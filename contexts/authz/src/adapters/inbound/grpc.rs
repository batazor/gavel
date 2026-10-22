#![allow(clippy::result_large_err)]

use std::{sync::Arc, time::Duration};

use tonic::{Request, Response, Status};

use crate::{
    application::{
        BulkCheckPermissionsInput, CheckPermissionInput, CommandReceiptStore, PermissionCheck,
        PermissionDecision, PolicyEngine, PrincipalReference, RegisterPrincipalInput, Relationship,
        ResourceReference, WorkloadIdentity, WriteRelationshipsInput,
        use_cases::{
            BulkCheckPermissionsUseCase, CheckPermissionUseCase, RegisterPrincipalUseCase,
            WriteRelationshipsUseCase,
        },
    },
    rpc::{self, authz_service_server::AuthzService},
};

use super::{RpcError, RpcKind, map_rpc_error};

pub struct AuthzGrpcService<P, S> {
    check: CheckPermissionUseCase<P>,
    bulk_check: BulkCheckPermissionsUseCase<P>,
    register: RegisterPrincipalUseCase<P, S>,
    write: WriteRelationshipsUseCase<P, S>,
    max_request_timeout: Duration,
}

impl<P, S> AuthzGrpcService<P, S>
where
    P: PolicyEngine,
    S: CommandReceiptStore,
{
    pub fn new(
        policy_engine: Arc<P>,
        receipt_store: Arc<S>,
        principal_key: [u8; 32],
        fingerprint_key: [u8; 32],
        max_request_timeout: Duration,
    ) -> Result<Self, &'static str> {
        if max_request_timeout.is_zero() {
            return Err("max request timeout must be positive");
        }
        Ok(Self {
            check: CheckPermissionUseCase::new(Arc::clone(&policy_engine)),
            bulk_check: BulkCheckPermissionsUseCase::new(Arc::clone(&policy_engine)),
            register: RegisterPrincipalUseCase::new(
                Arc::clone(&policy_engine),
                Arc::clone(&receipt_store),
                principal_key,
                fingerprint_key,
            ),
            write: WriteRelationshipsUseCase::new(policy_engine, receipt_store, fingerprint_key),
            max_request_timeout,
        })
    }
}

#[tonic::async_trait]
impl<P, S> AuthzService for AuthzGrpcService<P, S>
where
    P: PolicyEngine + 'static,
    S: CommandReceiptStore + 'static,
{
    async fn check_permission(
        &self,
        request: Request<rpc::CheckPermissionRequest>,
    ) -> Result<Response<rpc::CheckPermissionResponse>, Status> {
        validate_deadline(&request, self.max_request_timeout)?;
        let caller = caller(&request)?;
        let request = request.into_inner();
        let result = self
            .check
            .execute(CheckPermissionInput {
                caller,
                check: permission_check(request.check)?,
                consistency_token: optional_token(request.consistency_token),
            })
            .await
            .map_err(permission_status)?;
        Ok(Response::new(rpc::CheckPermissionResponse {
            decision: rpc_decision(result.decision) as i32,
            checked_at_token: result.checked_at_token,
        }))
    }

    async fn bulk_check_permissions(
        &self,
        request: Request<rpc::BulkCheckPermissionsRequest>,
    ) -> Result<Response<rpc::BulkCheckPermissionsResponse>, Status> {
        validate_deadline(&request, self.max_request_timeout)?;
        let caller = caller(&request)?;
        let request = request.into_inner();
        let checks = request
            .checks
            .into_iter()
            .map(|check| permission_check(Some(check)))
            .collect::<Result<Vec<_>, _>>()?;
        let result = self
            .bulk_check
            .execute(BulkCheckPermissionsInput {
                caller,
                checks,
                consistency_token: optional_token(request.consistency_token),
            })
            .await
            .map_err(permission_status)?;
        Ok(Response::new(rpc::BulkCheckPermissionsResponse {
            decisions: result
                .decisions
                .into_iter()
                .map(|decision| rpc_decision(decision) as i32)
                .collect(),
            checked_at_token: result.checked_at_token,
        }))
    }

    async fn register_principal(
        &self,
        request: Request<rpc::RegisterPrincipalRequest>,
    ) -> Result<Response<rpc::RegisterPrincipalResponse>, Status> {
        validate_deadline(&request, self.max_request_timeout)?;
        let caller = caller(&request)?;
        let request = request.into_inner();
        match self
            .register
            .execute(RegisterPrincipalInput {
                caller,
                issuer: request.issuer,
                subject: request.subject,
                idempotency_key: request.idempotency_key,
            })
            .await
        {
            Ok(result) => Ok(Response::new(rpc::RegisterPrincipalResponse {
                outcome: Some(rpc::register_principal_response::Outcome::Registered(
                    rpc::PrincipalRegistered {
                        principal_id: result.principal_id,
                        written_at_token: result.written_at_token,
                    },
                )),
            })),
            Err(error) => mutation_response(error, |rejection| rpc::RegisterPrincipalResponse {
                outcome: Some(rpc::register_principal_response::Outcome::Rejection(
                    rejection,
                )),
            }),
        }
    }

    async fn write_relationships(
        &self,
        request: Request<rpc::WriteRelationshipsRequest>,
    ) -> Result<Response<rpc::WriteRelationshipsResponse>, Status> {
        validate_deadline(&request, self.max_request_timeout)?;
        let caller = caller(&request)?;
        let request = request.into_inner();
        let updates = request
            .updates
            .into_iter()
            .map(relationship)
            .collect::<Result<Vec<_>, _>>()?;
        let deletions = request
            .deletions
            .into_iter()
            .map(relationship)
            .collect::<Result<Vec<_>, _>>()?;
        match self
            .write
            .execute(WriteRelationshipsInput {
                caller,
                updates,
                deletions,
                idempotency_key: request.idempotency_key,
            })
            .await
        {
            Ok(result) => Ok(Response::new(rpc::WriteRelationshipsResponse {
                outcome: Some(rpc::write_relationships_response::Outcome::Written(
                    rpc::RelationshipsWritten {
                        written_at_token: result.written_at_token,
                    },
                )),
            })),
            Err(error) => mutation_response(error, |rejection| rpc::WriteRelationshipsResponse {
                outcome: Some(rpc::write_relationships_response::Outcome::Rejection(
                    rejection,
                )),
            }),
        }
    }
}

fn caller<T>(request: &Request<T>) -> Result<WorkloadIdentity, Status> {
    request
        .extensions()
        .get::<WorkloadIdentity>()
        .copied()
        .ok_or_else(|| Status::unauthenticated("authentication is required"))
}

fn validate_deadline<T>(request: &Request<T>, maximum: Duration) -> Result<(), Status> {
    let value = request
        .metadata()
        .get("grpc-timeout")
        .ok_or_else(|| Status::invalid_argument("request deadline is required"))?
        .to_str()
        .map_err(|_| Status::invalid_argument("request deadline is invalid"))?;
    let (amount, unit) = value.split_at(value.len().saturating_sub(1));
    let amount = amount
        .parse::<u64>()
        .ok()
        .filter(|amount| *amount > 0)
        .ok_or_else(|| Status::invalid_argument("request deadline is invalid"))?;
    let duration = match unit {
        "H" => Duration::from_secs(amount.saturating_mul(60 * 60)),
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
    Ok(())
}

fn permission_check(value: Option<rpc::PermissionCheck>) -> Result<PermissionCheck, Status> {
    let value = value.ok_or_else(|| Status::invalid_argument("permission check is required"))?;
    let resource = value
        .resource
        .ok_or_else(|| Status::invalid_argument("resource is required"))?;
    let principal = value
        .principal
        .ok_or_else(|| Status::invalid_argument("principal is required"))?;
    Ok(PermissionCheck {
        resource: ResourceReference {
            resource_type: resource.resource_type,
            resource_id: resource.resource_id,
        },
        permission: value.permission,
        principal: PrincipalReference {
            principal_type: principal.principal_type,
            principal_id: principal.principal_id,
        },
    })
}

fn relationship(value: rpc::Relationship) -> Result<Relationship, Status> {
    let resource = value
        .resource
        .ok_or_else(|| Status::invalid_argument("resource is required"))?;
    let subject = value
        .subject
        .ok_or_else(|| Status::invalid_argument("relationship subject is required"))?;
    Ok(Relationship {
        resource: ResourceReference {
            resource_type: resource.resource_type,
            resource_id: resource.resource_id,
        },
        relation: value.relation,
        subject: PrincipalReference {
            principal_type: subject.principal_type,
            principal_id: subject.principal_id,
        },
    })
}

fn optional_token(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

fn rpc_decision(decision: PermissionDecision) -> rpc::PermissionDecision {
    match decision {
        PermissionDecision::Allowed => rpc::PermissionDecision::Allowed,
        PermissionDecision::Denied => rpc::PermissionDecision::Denied,
    }
}

fn permission_status(error: crate::application::ApplicationError) -> Status {
    match map_rpc_error(error, RpcKind::PermissionCheck) {
        RpcError::Status(status) => status,
        RpcError::Rejection(_) => Status::internal("internal authorization failure"),
    }
}

fn mutation_response<T>(
    error: crate::application::ApplicationError,
    rejection_response: impl FnOnce(rpc::DomainRejection) -> T,
) -> Result<Response<T>, Status> {
    match map_rpc_error(error, RpcKind::Mutation) {
        RpcError::Rejection(rejection) => Ok(Response::new(rejection_response(rejection))),
        RpcError::Status(status) => Err(status),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;

    use crate::application::{
        MutationResult, PolicyBulkCheckResult, PolicyCheckResult, PolicyDecision,
        PolicyEngineError, PolicyWriteResult, ReceiptClaim, ReceiptClaimRequest, ReceiptLease,
        ReceiptStoreError,
    };

    use super::*;

    struct FakePolicy {
        check: Mutex<Result<PolicyCheckResult, PolicyEngineError>>,
        writes: AtomicUsize,
    }

    #[async_trait]
    impl PolicyEngine for FakePolicy {
        async fn check_permission(
            &self,
            _check: &PermissionCheck,
            _consistency_token: Option<&str>,
        ) -> Result<PolicyCheckResult, PolicyEngineError> {
            self.check.lock().unwrap().clone()
        }

        async fn bulk_check_permissions(
            &self,
            checks: &[PermissionCheck],
            _consistency_token: Option<&str>,
        ) -> Result<PolicyBulkCheckResult, PolicyEngineError> {
            Ok(PolicyBulkCheckResult {
                decisions: checks
                    .iter()
                    .enumerate()
                    .map(|(index, _)| {
                        if index % 2 == 0 {
                            PolicyDecision::Allowed
                        } else {
                            PolicyDecision::Denied
                        }
                    })
                    .collect(),
                checked_at_token: "bulk-revision".to_owned(),
            })
        }

        async fn write_relationships(
            &self,
            _updates: &[Relationship],
            _deletions: &[Relationship],
        ) -> Result<PolicyWriteResult, PolicyEngineError> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(PolicyWriteResult {
                written_at_token: "write-revision".to_owned(),
            })
        }
    }

    #[derive(Clone, Copy)]
    enum StoreMode {
        Acquire,
        Conflict,
        InProgress,
        Unavailable,
    }

    struct FakeStore {
        mode: StoreMode,
    }

    #[async_trait]
    impl CommandReceiptStore for FakeStore {
        async fn claim(
            &self,
            request: ReceiptClaimRequest,
        ) -> Result<ReceiptClaim, ReceiptStoreError> {
            match self.mode {
                StoreMode::Acquire => Ok(ReceiptClaim::Acquired(ReceiptLease {
                    caller: request.caller,
                    idempotency_key: request.idempotency_key,
                    fingerprint: request.fingerprint,
                    lease_token: "lease".to_owned(),
                    attempt: 1,
                    recovered: false,
                })),
                StoreMode::Conflict => Ok(ReceiptClaim::Conflict),
                StoreMode::InProgress => Ok(ReceiptClaim::InProgress),
                StoreMode::Unavailable => Err(ReceiptStoreError::Unavailable),
            }
        }

        async fn complete(
            &self,
            _lease: &ReceiptLease,
            _result: &MutationResult,
        ) -> Result<(), ReceiptStoreError> {
            Ok(())
        }
    }

    fn policy(result: Result<PolicyCheckResult, PolicyEngineError>) -> Arc<FakePolicy> {
        Arc::new(FakePolicy {
            check: Mutex::new(result),
            writes: AtomicUsize::new(0),
        })
    }

    fn service(
        policy: Arc<FakePolicy>,
        mode: StoreMode,
    ) -> AuthzGrpcService<FakePolicy, FakeStore> {
        AuthzGrpcService::new(
            policy,
            Arc::new(FakeStore { mode }),
            [7; 32],
            [9; 32],
            Duration::from_secs(5),
        )
        .unwrap()
    }

    fn trusted<T>(body: T, caller: WorkloadIdentity) -> Request<T> {
        let mut request = Request::new(body);
        request.set_timeout(Duration::from_secs(1));
        request.extensions_mut().insert(caller);
        request
    }

    fn rpc_check(id: &str) -> rpc::PermissionCheck {
        rpc::PermissionCheck {
            resource: Some(rpc::ResourceReference {
                resource_type: "auction".to_owned(),
                resource_id: id.to_owned(),
            }),
            permission: "place_bid".to_owned(),
            principal: Some(rpc::PrincipalReference {
                principal_type: "user".to_owned(),
                principal_id: "u1_bidder".to_owned(),
            }),
        }
    }

    #[tokio::test]
    async fn permission_handlers_cover_success_invalid_auth_and_dependency_paths() {
        let service_under_test = service(
            policy(Ok(PolicyCheckResult {
                decision: PolicyDecision::Allowed,
                checked_at_token: "revision".to_owned(),
            })),
            StoreMode::Acquire,
        );
        let response = service_under_test
            .check_permission(trusted(
                rpc::CheckPermissionRequest {
                    check: Some(rpc_check("auction-1")),
                    consistency_token: String::new(),
                },
                WorkloadIdentity::Auction,
            ))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(response.decision, rpc::PermissionDecision::Allowed as i32);

        let bulk = service_under_test
            .bulk_check_permissions(trusted(
                rpc::BulkCheckPermissionsRequest {
                    checks: vec![rpc_check("auction-1"), rpc_check("auction-2")],
                    consistency_token: String::new(),
                },
                WorkloadIdentity::Bff,
            ))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(
            bulk.decisions,
            vec![
                rpc::PermissionDecision::Allowed as i32,
                rpc::PermissionDecision::Denied as i32
            ]
        );

        let invalid = service_under_test
            .check_permission(trusted(
                rpc::CheckPermissionRequest {
                    check: None,
                    consistency_token: String::new(),
                },
                WorkloadIdentity::Auction,
            ))
            .await
            .unwrap_err();
        assert_eq!(invalid.code(), tonic::Code::InvalidArgument);

        let mut unauthenticated = Request::new(rpc::CheckPermissionRequest {
            check: Some(rpc_check("auction-1")),
            consistency_token: String::new(),
        });
        unauthenticated.set_timeout(Duration::from_secs(1));
        assert_eq!(
            service_under_test
                .check_permission(unauthenticated)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );

        let unavailable = service(
            policy(Err(PolicyEngineError::Unavailable)),
            StoreMode::Acquire,
        )
        .check_permission(trusted(
            rpc::CheckPermissionRequest {
                check: Some(rpc_check("auction-1")),
                consistency_token: String::new(),
            },
            WorkloadIdentity::Auction,
        ))
        .await
        .unwrap_err();
        assert_eq!(unavailable.code(), tonic::Code::Unavailable);
    }

    #[tokio::test]
    async fn mutation_handlers_cover_success_rejections_and_retryable_paths() {
        let policy_ref = policy(Err(PolicyEngineError::InvalidResponse));
        let success = service(Arc::clone(&policy_ref), StoreMode::Acquire)
            .register_principal(trusted(
                rpc::RegisterPrincipalRequest {
                    issuer: "https://identity.example.test".to_owned(),
                    subject: "external-user".to_owned(),
                    idempotency_key: "login-1".to_owned(),
                },
                WorkloadIdentity::Bff,
            ))
            .await
            .unwrap()
            .into_inner();
        assert!(matches!(
            success.outcome,
            Some(rpc::register_principal_response::Outcome::Registered(_))
        ));
        assert_eq!(policy_ref.writes.load(Ordering::SeqCst), 1);

        let forbidden = service(
            policy(Err(PolicyEngineError::InvalidResponse)),
            StoreMode::Acquire,
        )
        .register_principal(trusted(
            rpc::RegisterPrincipalRequest {
                issuer: "https://identity.example.test".to_owned(),
                subject: "external-user".to_owned(),
                idempotency_key: "login-2".to_owned(),
            },
            WorkloadIdentity::Auction,
        ))
        .await
        .unwrap()
        .into_inner();
        assert!(matches!(
            forbidden.outcome,
            Some(rpc::register_principal_response::Outcome::Rejection(rejection))
                if rejection.reason == "FORBIDDEN"
        ));

        for (mode, expected) in [
            (StoreMode::InProgress, tonic::Code::Aborted),
            (StoreMode::Unavailable, tonic::Code::Unavailable),
        ] {
            let status = service(policy(Err(PolicyEngineError::InvalidResponse)), mode)
                .register_principal(trusted(
                    rpc::RegisterPrincipalRequest {
                        issuer: "https://identity.example.test".to_owned(),
                        subject: "external-user".to_owned(),
                        idempotency_key: "login-retry".to_owned(),
                    },
                    WorkloadIdentity::Bff,
                ))
                .await
                .unwrap_err();
            assert_eq!(status.code(), expected);
        }

        let conflict = service(
            policy(Err(PolicyEngineError::InvalidResponse)),
            StoreMode::Conflict,
        )
        .register_principal(trusted(
            rpc::RegisterPrincipalRequest {
                issuer: "https://identity.example.test".to_owned(),
                subject: "external-user".to_owned(),
                idempotency_key: "login-conflict".to_owned(),
            },
            WorkloadIdentity::Bff,
        ))
        .await
        .unwrap()
        .into_inner();
        assert!(matches!(
            conflict.outcome,
            Some(rpc::register_principal_response::Outcome::Rejection(rejection))
                if rejection.reason == "idempotency_conflict"
        ));
    }

    #[tokio::test]
    async fn missing_or_excessive_deadline_is_rejected() {
        let service_under_test = service(
            policy(Ok(PolicyCheckResult {
                decision: PolicyDecision::Allowed,
                checked_at_token: "revision".to_owned(),
            })),
            StoreMode::Acquire,
        );
        let mut missing = Request::new(rpc::CheckPermissionRequest {
            check: Some(rpc_check("auction-1")),
            consistency_token: String::new(),
        });
        missing.extensions_mut().insert(WorkloadIdentity::Auction);
        assert_eq!(
            service_under_test
                .check_permission(missing)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::InvalidArgument
        );
        let mut excessive = trusted(
            rpc::CheckPermissionRequest {
                check: Some(rpc_check("auction-1")),
                consistency_token: String::new(),
            },
            WorkloadIdentity::Auction,
        );
        excessive.set_timeout(Duration::from_secs(6));
        assert_eq!(
            service_under_test
                .check_permission(excessive)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::InvalidArgument
        );
    }
}
