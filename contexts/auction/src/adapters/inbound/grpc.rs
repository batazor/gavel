#![allow(clippy::result_large_err)]

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use tonic::{Request, Response, Status};

use crate::{
    application::{
        ApplicationError, AuctionLifecycle, AuctionUnitOfWork, Authorization, CancelAuctionInput,
        CloseAuctionInput, CommandContext, ConfirmWinnerInput, ForfeitSettlementCandidateInput,
        LifecycleResult, Principal, PrincipalKind, PublicAuctionStatus,
    },
    domain::Timestamp,
    rpc::{
        self, CancelAuctionRequest, CancelAuctionResponse, CloseAuctionRequest,
        CloseAuctionResponse, ConfirmWinnerRequest, ConfirmWinnerResponse,
        ForfeitSettlementCandidateRequest, ForfeitSettlementCandidateResponse,
        auction_service_server::AuctionService,
    },
};

use super::GrpcError;

pub const PRINCIPAL_TYPE_HEADER: &str = "x-gavel-principal-type";
pub const PRINCIPAL_ID_HEADER: &str = "x-gavel-principal-id";
pub const CORRELATION_ID_HEADER: &str = "x-correlation-id";
pub const CAUSATION_ID_HEADER: &str = "x-causation-id";
pub const WORKLOAD_SPIFFE_ID_HEADER: &str = "x-gavel-spiffe-id";

#[async_trait]
pub trait AuctionGrpcApi: Send + Sync {
    async fn close(
        &self,
        context: &CommandContext,
        input: CloseAuctionInput,
    ) -> Result<LifecycleResult, ApplicationError>;
    async fn cancel(
        &self,
        context: &CommandContext,
        input: CancelAuctionInput,
    ) -> Result<LifecycleResult, ApplicationError>;
    async fn forfeit(
        &self,
        context: &CommandContext,
        input: ForfeitSettlementCandidateInput,
    ) -> Result<LifecycleResult, ApplicationError>;
    async fn confirm(
        &self,
        context: &CommandContext,
        input: ConfirmWinnerInput,
    ) -> Result<LifecycleResult, ApplicationError>;
}

pub struct AuctionGrpcApplication<U, A> {
    lifecycle: AuctionLifecycle<U, A>,
}

impl<U, A> AuctionGrpcApplication<U, A>
where
    U: AuctionUnitOfWork,
    A: Authorization,
{
    pub fn new(
        unit_of_work: Arc<U>,
        authorization: Arc<A>,
        retry_attempts: usize,
        fingerprint_key: [u8; 32],
    ) -> Self {
        Self {
            lifecycle: AuctionLifecycle::new(
                unit_of_work,
                authorization,
                retry_attempts,
                fingerprint_key,
            ),
        }
    }
}

#[async_trait]
impl<U, A> AuctionGrpcApi for AuctionGrpcApplication<U, A>
where
    U: AuctionUnitOfWork + 'static,
    A: Authorization + 'static,
{
    async fn close(
        &self,
        context: &CommandContext,
        input: CloseAuctionInput,
    ) -> Result<LifecycleResult, ApplicationError> {
        self.lifecycle.close(context, input).await
    }

    async fn cancel(
        &self,
        context: &CommandContext,
        input: CancelAuctionInput,
    ) -> Result<LifecycleResult, ApplicationError> {
        self.lifecycle.cancel(context, input).await
    }

    async fn forfeit(
        &self,
        context: &CommandContext,
        input: ForfeitSettlementCandidateInput,
    ) -> Result<LifecycleResult, ApplicationError> {
        self.lifecycle.forfeit(context, input).await
    }

    async fn confirm(
        &self,
        context: &CommandContext,
        input: ConfirmWinnerInput,
    ) -> Result<LifecycleResult, ApplicationError> {
        self.lifecycle.confirm(context, input).await
    }
}

pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

#[derive(Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        Timestamp::from_millis(u64::try_from(millis).unwrap_or(u64::MAX))
    }
}

#[derive(Clone)]
pub struct AuctionGrpcService {
    api: Arc<dyn AuctionGrpcApi>,
    clock: Arc<dyn Clock>,
    allowed_workload_spiffe_id: Option<String>,
}

impl AuctionGrpcService {
    pub fn new(api: Arc<dyn AuctionGrpcApi>, clock: Arc<dyn Clock>) -> Self {
        Self {
            api,
            clock,
            allowed_workload_spiffe_id: None,
        }
    }

    pub fn new_authenticated(
        api: Arc<dyn AuctionGrpcApi>,
        clock: Arc<dyn Clock>,
        allowed_workload_spiffe_id: impl Into<String>,
    ) -> Self {
        Self {
            api,
            clock,
            allowed_workload_spiffe_id: Some(allowed_workload_spiffe_id.into()),
        }
    }
}

struct TrustedCallContext {
    command: CommandContext,
    timeout: Option<Duration>,
}

#[tonic::async_trait]
impl AuctionService for AuctionGrpcService {
    async fn close_auction(
        &self,
        request: Request<CloseAuctionRequest>,
    ) -> Result<Response<CloseAuctionResponse>, Status> {
        let trusted = trusted_context(&request, self.allowed_workload_spiffe_id.as_deref())?;
        let input = request.into_inner();
        validate_required(&input.auction_id, "auction_id")?;
        let now = input
            .now
            .ok_or_else(|| Status::invalid_argument("now is required"))
            .and_then(prost_timestamp)?;
        let result = with_deadline(
            trusted.timeout,
            self.api.close(
                &trusted.command,
                CloseAuctionInput {
                    auction_id: input.auction_id,
                    now,
                },
            ),
        )
        .await;
        match result {
            Ok(LifecycleResult::Closed {
                status,
                closing_price,
                version,
            }) => Ok(Response::new(CloseAuctionResponse {
                outcome: Some(rpc::close_auction_response::Outcome::Closed(
                    rpc::AuctionClosed {
                        status: grpc_status(status) as i32,
                        closing_price: Some(rpc::Money {
                            minor_units: closing_price.minor_units,
                            currency_code: closing_price.currency_code,
                        }),
                        version,
                    },
                )),
            })),
            Ok(_) => Err(Status::internal("unexpected lifecycle result")),
            Err(error) => map_close_error(error),
        }
    }

    async fn cancel_auction(
        &self,
        request: Request<CancelAuctionRequest>,
    ) -> Result<Response<CancelAuctionResponse>, Status> {
        let trusted = trusted_context(&request, self.allowed_workload_spiffe_id.as_deref())?;
        let input = request.into_inner();
        validate_required(&input.auction_id, "auction_id")?;
        let result = with_deadline(
            trusted.timeout,
            self.api.cancel(
                &trusted.command,
                CancelAuctionInput {
                    auction_id: input.auction_id,
                    occurred_at: self.clock.now(),
                },
            ),
        )
        .await;
        match result {
            Ok(LifecycleResult::Cancelled { version }) => {
                Ok(Response::new(CancelAuctionResponse {
                    outcome: Some(rpc::cancel_auction_response::Outcome::Cancelled(
                        rpc::AuctionCancelled { version },
                    )),
                }))
            }
            Ok(_) => Err(Status::internal("unexpected lifecycle result")),
            Err(error) => map_cancel_error(error),
        }
    }

    async fn forfeit_settlement_candidate(
        &self,
        request: Request<ForfeitSettlementCandidateRequest>,
    ) -> Result<Response<ForfeitSettlementCandidateResponse>, Status> {
        let trusted = trusted_context(&request, self.allowed_workload_spiffe_id.as_deref())?;
        let input = request.into_inner();
        validate_required(&input.auction_id, "auction_id")?;
        validate_required(&input.candidate_id, "candidate_id")?;
        let result = with_deadline(
            trusted.timeout,
            self.api.forfeit(
                &trusted.command,
                ForfeitSettlementCandidateInput {
                    auction_id: input.auction_id,
                    candidate_id: input.candidate_id,
                    settlement_round: input.settlement_round,
                    occurred_at: self.clock.now(),
                },
            ),
        )
        .await;
        match result {
            Ok(LifecycleResult::SettlementAdvanced {
                status,
                settlement_round,
                version,
            }) => Ok(Response::new(ForfeitSettlementCandidateResponse {
                outcome: Some(
                    rpc::forfeit_settlement_candidate_response::Outcome::Advanced(
                        rpc::SettlementAdvanced {
                            status: grpc_status(status) as i32,
                            settlement_round,
                            version,
                        },
                    ),
                ),
            })),
            Ok(_) => Err(Status::internal("unexpected lifecycle result")),
            Err(error) => map_forfeit_error(error),
        }
    }

    async fn confirm_winner(
        &self,
        request: Request<ConfirmWinnerRequest>,
    ) -> Result<Response<ConfirmWinnerResponse>, Status> {
        let trusted = trusted_context(&request, self.allowed_workload_spiffe_id.as_deref())?;
        let input = request.into_inner();
        validate_required(&input.auction_id, "auction_id")?;
        validate_required(&input.candidate_id, "candidate_id")?;
        validate_required(&input.payment_id, "payment_id")?;
        let result = with_deadline(
            trusted.timeout,
            self.api.confirm(
                &trusted.command,
                ConfirmWinnerInput {
                    auction_id: input.auction_id,
                    candidate_id: input.candidate_id,
                    payment_id: input.payment_id,
                    occurred_at: self.clock.now(),
                },
            ),
        )
        .await;
        match result {
            Ok(LifecycleResult::WinnerConfirmed { version }) => {
                Ok(Response::new(ConfirmWinnerResponse {
                    outcome: Some(rpc::confirm_winner_response::Outcome::Confirmed(
                        rpc::WinnerConfirmed { version },
                    )),
                }))
            }
            Ok(_) => Err(Status::internal("unexpected lifecycle result")),
            Err(error) => map_confirm_error(error),
        }
    }
}

async fn with_deadline<T>(
    timeout: Option<Duration>,
    future: impl std::future::Future<Output = Result<T, ApplicationError>>,
) -> Result<T, GrpcError> {
    match timeout {
        Some(timeout) => tokio::time::timeout(timeout, future)
            .await
            .map_err(|_| GrpcError::Status(Status::deadline_exceeded("request deadline exceeded")))?
            .map_err(GrpcError::from),
        None => future.await.map_err(GrpcError::from),
    }
}

fn trusted_context<T: IdempotencyKey>(
    request: &Request<T>,
    allowed_workload_spiffe_id: Option<&str>,
) -> Result<TrustedCallContext, Status> {
    let metadata = request.metadata();
    if let Some(expected) = allowed_workload_spiffe_id {
        let actual = metadata_value(metadata, WORKLOAD_SPIFFE_ID_HEADER)?;
        if actual != expected {
            return Err(Status::permission_denied("workload is not authorized"));
        }
    }
    let principal_type = metadata_value(metadata, PRINCIPAL_TYPE_HEADER)?;
    if principal_type != "service_account" {
        return Err(Status::permission_denied(
            "service account identity is required",
        ));
    }
    let principal_id = metadata_value(metadata, PRINCIPAL_ID_HEADER)?;
    let correlation_id = metadata_value(metadata, CORRELATION_ID_HEADER)?;
    let causation_id = metadata_value(metadata, CAUSATION_ID_HEADER)?;
    let idempotency_key = request
        .get_ref()
        .get_ref_idempotency_key()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Status::invalid_argument("idempotency_key is required"))?;
    let timeout = metadata
        .get("grpc-timeout")
        .map(|value| {
            value
                .to_str()
                .map_err(|_| Status::invalid_argument("invalid grpc-timeout"))
        })
        .transpose()?
        .map(parse_grpc_timeout)
        .transpose()?;
    Ok(TrustedCallContext {
        command: CommandContext {
            principal: Principal {
                kind: PrincipalKind::ServiceAccount,
                id: principal_id,
            },
            idempotency_key,
            correlation_id,
            causation_id,
        },
        timeout,
    })
}

trait IdempotencyKey {
    fn get_ref_idempotency_key(&self) -> Option<String>;
}

impl IdempotencyKey for CloseAuctionRequest {
    fn get_ref_idempotency_key(&self) -> Option<String> {
        Some(self.idempotency_key.clone())
    }
}
impl IdempotencyKey for CancelAuctionRequest {
    fn get_ref_idempotency_key(&self) -> Option<String> {
        Some(self.idempotency_key.clone())
    }
}
impl IdempotencyKey for ForfeitSettlementCandidateRequest {
    fn get_ref_idempotency_key(&self) -> Option<String> {
        Some(self.idempotency_key.clone())
    }
}
impl IdempotencyKey for ConfirmWinnerRequest {
    fn get_ref_idempotency_key(&self) -> Option<String> {
        Some(self.idempotency_key.clone())
    }
}

fn metadata_value(
    metadata: &tonic::metadata::MetadataMap,
    key: &'static str,
) -> Result<String, Status> {
    metadata
        .get(key)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| Status::unauthenticated(format!("missing trusted {key}")))
}

fn parse_grpc_timeout(value: &str) -> Result<Duration, Status> {
    if value.len() < 2 || value.len() > 9 {
        return Err(Status::invalid_argument("invalid grpc-timeout"));
    }
    let (number, unit) = value.split_at(value.len() - 1);
    let number: u64 = number
        .parse()
        .map_err(|_| Status::invalid_argument("invalid grpc-timeout"))?;
    match unit {
        "H" => Ok(Duration::from_secs(number.saturating_mul(3_600))),
        "M" => Ok(Duration::from_secs(number.saturating_mul(60))),
        "S" => Ok(Duration::from_secs(number)),
        "m" => Ok(Duration::from_millis(number)),
        "u" => Ok(Duration::from_micros(number)),
        "n" => Ok(Duration::from_nanos(number)),
        _ => Err(Status::invalid_argument("invalid grpc-timeout")),
    }
}

fn prost_timestamp(value: prost_types::Timestamp) -> Result<Timestamp, Status> {
    if value.seconds < 0 || !(0..1_000_000_000).contains(&value.nanos) {
        return Err(Status::invalid_argument("timestamp is out of range"));
    }
    let millis = u64::try_from(value.seconds)
        .ok()
        .and_then(|seconds| seconds.checked_mul(1_000))
        .and_then(|millis| millis.checked_add(u64::from(value.nanos as u32) / 1_000_000))
        .ok_or_else(|| Status::invalid_argument("timestamp is out of range"))?;
    Ok(Timestamp::from_millis(millis))
}

fn validate_required(value: &str, field: &'static str) -> Result<(), Status> {
    if value.trim().is_empty() {
        return Err(Status::invalid_argument(format!("{field} is required")));
    }
    Ok(())
}

fn grpc_status(status: PublicAuctionStatus) -> rpc::AuctionStatus {
    match status {
        PublicAuctionStatus::Settling => rpc::AuctionStatus::Settling,
        PublicAuctionStatus::Sold => rpc::AuctionStatus::Sold,
        PublicAuctionStatus::Unsold => rpc::AuctionStatus::Unsold,
        PublicAuctionStatus::Cancelled => rpc::AuctionStatus::Cancelled,
        PublicAuctionStatus::Scheduled | PublicAuctionStatus::Open => {
            rpc::AuctionStatus::Unspecified
        }
    }
}

fn map_close_error(error: GrpcError) -> Result<Response<CloseAuctionResponse>, Status> {
    map_error_response(error, |rejection| CloseAuctionResponse {
        outcome: Some(rpc::close_auction_response::Outcome::Rejection(rejection)),
    })
}

fn map_cancel_error(error: GrpcError) -> Result<Response<CancelAuctionResponse>, Status> {
    map_error_response(error, |rejection| CancelAuctionResponse {
        outcome: Some(rpc::cancel_auction_response::Outcome::Rejection(rejection)),
    })
}

fn map_forfeit_error(
    error: GrpcError,
) -> Result<Response<ForfeitSettlementCandidateResponse>, Status> {
    map_error_response(error, |rejection| ForfeitSettlementCandidateResponse {
        outcome: Some(rpc::forfeit_settlement_candidate_response::Outcome::Rejection(rejection)),
    })
}

fn map_confirm_error(error: GrpcError) -> Result<Response<ConfirmWinnerResponse>, Status> {
    map_error_response(error, |rejection| ConfirmWinnerResponse {
        outcome: Some(rpc::confirm_winner_response::Outcome::Rejection(rejection)),
    })
}

fn map_error_response<T>(
    error: GrpcError,
    rejection_response: impl FnOnce(rpc::DomainRejection) -> T,
) -> Result<Response<T>, Status> {
    match error {
        GrpcError::Rejection(rejection) => Ok(Response::new(rejection_response(rejection))),
        GrpcError::Status(status) => Err(status),
    }
}
