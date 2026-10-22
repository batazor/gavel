use crate::{
    application::{ApplicationError, ErrorCode},
    rpc,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphqlErrorView {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}

impl From<&ApplicationError> for GraphqlErrorView {
    fn from(error: &ApplicationError) -> Self {
        Self {
            code: error.code().as_str(),
            message: error.to_string(),
            retryable: error.retryable(),
        }
    }
}

#[derive(Debug)]
pub enum GrpcError {
    Rejection(rpc::DomainRejection),
    Status(tonic::Status),
}

impl From<ApplicationError> for GrpcError {
    fn from(error: ApplicationError) -> Self {
        let code = error.code();
        let reason = error.reason();
        let message = error.to_string();
        match code {
            ErrorCode::DomainRejected | ErrorCode::PaymentMethodRequired | ErrorCode::NotFound => {
                Self::Rejection(rpc::DomainRejection {
                    reason: reason.to_owned(),
                    safe_message: message,
                })
            }
            ErrorCode::InvalidArgument => Self::Status(tonic::Status::invalid_argument(message)),
            ErrorCode::Unauthenticated => Self::Status(tonic::Status::unauthenticated(message)),
            ErrorCode::Forbidden => Self::Status(tonic::Status::permission_denied(message)),
            ErrorCode::IdempotencyConflict => Self::Status(tonic::Status::already_exists(message)),
            ErrorCode::Contention | ErrorCode::DependencyUnavailable => {
                Self::Status(tonic::Status::unavailable(message))
            }
            ErrorCode::Internal => Self::Status(tonic::Status::internal(message)),
        }
    }
}

#[cfg(test)]
mod tests {
    use tonic::Code;

    use super::*;

    #[test]
    fn graphql_mapping_is_stable_and_marks_only_retryable_failures() {
        let cases = [
            (ApplicationError::Forbidden, "FORBIDDEN", false),
            (ApplicationError::Contention, "CONTENTION", true),
            (
                ApplicationError::DependencyUnavailable {
                    dependency: "authz",
                },
                "DEPENDENCY_UNAVAILABLE",
                true,
            ),
            (ApplicationError::Internal, "INTERNAL", false),
        ];

        for (error, code, retryable) in cases {
            let mapped = GraphqlErrorView::from(&error);
            assert_eq!(mapped.code, code);
            assert_eq!(mapped.retryable, retryable);
        }
    }

    #[test]
    fn grpc_mapping_separates_domain_rejections_from_transport_statuses() {
        let domain = GrpcError::from(ApplicationError::DomainRejected {
            reason: "too_early",
            safe_message: "auction deadline has not yet been reached".to_owned(),
        });
        match domain {
            GrpcError::Rejection(rejection) => assert_eq!(rejection.reason, "too_early"),
            GrpcError::Status(status) => panic!("unexpected status: {status}"),
        }

        let cases = [
            (ApplicationError::Unauthenticated, Code::Unauthenticated),
            (ApplicationError::Forbidden, Code::PermissionDenied),
            (ApplicationError::IdempotencyConflict, Code::AlreadyExists),
            (ApplicationError::Contention, Code::Unavailable),
            (ApplicationError::Internal, Code::Internal),
        ];
        for (error, expected_code) in cases {
            match GrpcError::from(error) {
                GrpcError::Status(status) => assert_eq!(status.code(), expected_code),
                GrpcError::Rejection(rejection) => {
                    panic!("unexpected rejection: {}", rejection.reason)
                }
            }
        }
    }
}
