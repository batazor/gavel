use crate::{
    application::{ApplicationError, ErrorCode},
    rpc,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphqlErrorContract {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}

pub enum RpcError {
    Rejection(rpc::DomainRejection),
    Status(tonic::Status),
}

pub fn graphql_error(error: &ApplicationError) -> GraphqlErrorContract {
    GraphqlErrorContract {
        code: error.reason(),
        message: error.to_string(),
        retryable: error.retryable(),
    }
}

pub fn rpc_error(error: ApplicationError) -> RpcError {
    if matches!(
        error.code(),
        ErrorCode::Forbidden
            | ErrorCode::NotFound
            | ErrorCode::Conflict
            | ErrorCode::IdempotencyConflict
    ) {
        return RpcError::Rejection(rpc::DomainRejection {
            reason: error.reason().to_owned(),
            safe_message: error.to_string(),
        });
    }
    let message = error.to_string();
    let status = match error.code() {
        ErrorCode::InvalidArgument => tonic::Status::invalid_argument(message),
        ErrorCode::Unauthenticated => tonic::Status::unauthenticated(message),
        ErrorCode::CommandInProgress => tonic::Status::aborted(message),
        ErrorCode::DependencyUnavailable => tonic::Status::unavailable(message),
        ErrorCode::Internal => tonic::Status::internal(message),
        ErrorCode::Forbidden
        | ErrorCode::NotFound
        | ErrorCode::Conflict
        | ErrorCode::IdempotencyConflict => unreachable!("domain rejections returned above"),
    };
    RpcError::Status(status)
}

#[cfg(test)]
mod tests {
    use tonic::Code;

    use super::*;

    #[test]
    fn graphql_contract_is_stable_safe_and_marks_only_retryable_failures() {
        let dependency = ApplicationError::DependencyUnavailable {
            dependency: "postgres.secret.internal",
        };
        let mapped = graphql_error(&dependency);
        assert_eq!(mapped.code, "dependency_unavailable");
        assert!(mapped.retryable);
        assert_eq!(mapped.message, "required dependency is unavailable");
        assert!(!mapped.message.contains("postgres"));
        assert!(!graphql_error(&ApplicationError::Forbidden).retryable);
    }

    #[test]
    fn rpc_contract_separates_domain_rejections_from_transport_statuses() {
        for error in [
            ApplicationError::Forbidden,
            ApplicationError::NotFound,
            ApplicationError::Conflict {
                reason: "description_frozen",
                safe_message: "lot description is frozen".to_owned(),
            },
            ApplicationError::IdempotencyConflict,
        ] {
            assert!(matches!(rpc_error(error), RpcError::Rejection(_)));
        }
        let cases = [
            (
                ApplicationError::InvalidArgument {
                    reason: "invalid_lot",
                    safe_message: "lot is invalid".to_owned(),
                },
                Code::InvalidArgument,
            ),
            (ApplicationError::Unauthenticated, Code::Unauthenticated),
            (ApplicationError::CommandInProgress, Code::Aborted),
            (
                ApplicationError::DependencyUnavailable {
                    dependency: "authz.secret.internal",
                },
                Code::Unavailable,
            ),
            (ApplicationError::Internal, Code::Internal),
        ];
        for (error, code) in cases {
            match rpc_error(error) {
                RpcError::Status(status) => {
                    assert_eq!(status.code(), code);
                    assert!(!status.message().contains("secret.internal"));
                }
                RpcError::Rejection(_) => panic!("unexpected domain rejection"),
            }
        }
    }
}
