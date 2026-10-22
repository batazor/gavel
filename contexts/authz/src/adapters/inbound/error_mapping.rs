use crate::{
    application::{ApplicationError, ErrorCode},
    rpc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RpcKind {
    PermissionCheck,
    Mutation,
}

#[derive(Debug)]
pub enum RpcError {
    Rejection(rpc::DomainRejection),
    Status(tonic::Status),
}

pub fn map_rpc_error(error: ApplicationError, rpc_kind: RpcKind) -> RpcError {
    let code = error.code();
    let reason = error.reason();
    let safe_message = error.to_string();

    if rpc_kind == RpcKind::Mutation
        && matches!(code, ErrorCode::Forbidden | ErrorCode::IdempotencyConflict)
    {
        return RpcError::Rejection(rpc::DomainRejection {
            reason: reason.to_owned(),
            safe_message,
        });
    }

    let status = match code {
        ErrorCode::InvalidArgument => tonic::Status::invalid_argument(safe_message),
        ErrorCode::BatchTooLarge => tonic::Status::resource_exhausted(safe_message),
        ErrorCode::Unauthenticated => tonic::Status::unauthenticated(safe_message),
        ErrorCode::Forbidden => tonic::Status::permission_denied(safe_message),
        ErrorCode::IdempotencyConflict => tonic::Status::already_exists(safe_message),
        ErrorCode::CommandInProgress => tonic::Status::aborted(safe_message),
        ErrorCode::DependencyUnavailable => tonic::Status::unavailable(safe_message),
        ErrorCode::Internal => tonic::Status::internal(safe_message),
    };
    RpcError::Status(status)
}

#[cfg(test)]
mod tests {
    use tonic::Code;

    use super::*;

    fn status_code(error: ApplicationError, rpc_kind: RpcKind) -> Code {
        match map_rpc_error(error, rpc_kind) {
            RpcError::Status(status) => status.code(),
            RpcError::Rejection(rejection) => {
                panic!("unexpected rejection: {}", rejection.reason)
            }
        }
    }

    #[test]
    fn permission_rpc_failures_have_stable_statuses() {
        let cases = [
            (
                ApplicationError::InvalidArgument {
                    reason: "invalid_permission",
                    safe_message: "permission is invalid".to_owned(),
                },
                Code::InvalidArgument,
            ),
            (ApplicationError::BatchTooLarge, Code::ResourceExhausted),
            (ApplicationError::Unauthenticated, Code::Unauthenticated),
            (ApplicationError::Forbidden, Code::PermissionDenied),
            (ApplicationError::IdempotencyConflict, Code::AlreadyExists),
            (ApplicationError::CommandInProgress, Code::Aborted),
            (
                ApplicationError::DependencyUnavailable {
                    dependency: "policy_engine",
                },
                Code::Unavailable,
            ),
            (ApplicationError::Internal, Code::Internal),
        ];

        for (error, expected) in cases {
            assert_eq!(status_code(error, RpcKind::PermissionCheck), expected);
        }
    }

    #[test]
    fn mutation_scope_and_conflict_are_typed_domain_rejections() {
        let cases = [
            (ApplicationError::Forbidden, "FORBIDDEN"),
            (
                ApplicationError::IdempotencyConflict,
                "idempotency_conflict",
            ),
        ];

        for (error, expected_reason) in cases {
            match map_rpc_error(error, RpcKind::Mutation) {
                RpcError::Rejection(rejection) => {
                    assert_eq!(rejection.reason, expected_reason);
                    assert!(!rejection.safe_message.is_empty());
                }
                RpcError::Status(status) => panic!("unexpected status: {status}"),
            }
        }
    }

    #[test]
    fn dependency_details_never_reach_the_wire() {
        let mapped = map_rpc_error(
            ApplicationError::DependencyUnavailable {
                dependency: "spicedb-prod.secret.svc",
            },
            RpcKind::PermissionCheck,
        );
        match mapped {
            RpcError::Status(status) => {
                assert_eq!(status.code(), Code::Unavailable);
                assert_eq!(status.message(), "required dependency is unavailable");
                assert!(!status.message().contains("spicedb-prod"));
            }
            RpcError::Rejection(_) => panic!("unexpected rejection"),
        }
    }
}
