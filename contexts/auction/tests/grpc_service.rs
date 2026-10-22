use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use gavel_auction::{
    Timestamp,
    adapters::inbound::grpc::{
        AuctionGrpcApi, AuctionGrpcService, CAUSATION_ID_HEADER, CORRELATION_ID_HEADER, Clock,
        PRINCIPAL_ID_HEADER, PRINCIPAL_TYPE_HEADER,
    },
    application::{
        ApplicationError, CancelAuctionInput, CloseAuctionInput, CommandContext,
        ConfirmWinnerInput, ForfeitSettlementCandidateInput, LifecycleResult, MoneyView,
        PublicAuctionStatus,
    },
    rpc::{
        CancelAuctionRequest, CloseAuctionRequest, ConfirmWinnerRequest,
        ForfeitSettlementCandidateRequest,
        auction_service_client::AuctionServiceClient,
        auction_service_server::{AuctionService, AuctionServiceServer},
        cancel_auction_response, close_auction_response, confirm_winner_response,
        forfeit_settlement_candidate_response,
    },
};
use tokio::{net::TcpListener, sync::oneshot};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Code, Request, transport::Server};

#[derive(Clone, Copy)]
enum Behavior {
    Success,
    DomainRejection,
    Unavailable,
    Delayed,
}

struct MockApi {
    behavior: Behavior,
    contexts: Mutex<Vec<CommandContext>>,
}

impl MockApi {
    async fn before(&self, context: &CommandContext) -> Result<(), ApplicationError> {
        self.contexts.lock().unwrap().push(context.clone());
        match self.behavior {
            Behavior::Success => Ok(()),
            Behavior::DomainRejection => Err(ApplicationError::DomainRejected {
                reason: "too_early",
                safe_message: "auction deadline has not yet been reached".to_owned(),
            }),
            Behavior::Unavailable => Err(ApplicationError::DependencyUnavailable {
                dependency: "authz",
            }),
            Behavior::Delayed => {
                tokio::time::sleep(Duration::from_millis(200)).await;
                Ok(())
            }
        }
    }
}

#[async_trait]
impl AuctionGrpcApi for MockApi {
    async fn close(
        &self,
        context: &CommandContext,
        _input: CloseAuctionInput,
    ) -> Result<LifecycleResult, ApplicationError> {
        self.before(context).await?;
        Ok(LifecycleResult::Closed {
            status: PublicAuctionStatus::Settling,
            closing_price: MoneyView {
                minor_units: 100,
                currency_code: "USD".to_owned(),
            },
            version: 2,
        })
    }

    async fn cancel(
        &self,
        context: &CommandContext,
        _input: CancelAuctionInput,
    ) -> Result<LifecycleResult, ApplicationError> {
        self.before(context).await?;
        Ok(LifecycleResult::Cancelled { version: 3 })
    }

    async fn forfeit(
        &self,
        context: &CommandContext,
        _input: ForfeitSettlementCandidateInput,
    ) -> Result<LifecycleResult, ApplicationError> {
        self.before(context).await?;
        Ok(LifecycleResult::SettlementAdvanced {
            status: PublicAuctionStatus::Unsold,
            settlement_round: 3,
            version: 4,
        })
    }

    async fn confirm(
        &self,
        context: &CommandContext,
        _input: ConfirmWinnerInput,
    ) -> Result<LifecycleResult, ApplicationError> {
        self.before(context).await?;
        Ok(LifecycleResult::WinnerConfirmed { version: 5 })
    }
}

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_millis(1_700_000_000_000)
    }
}

async fn client(
    behavior: Behavior,
) -> (
    AuctionServiceClient<tonic::transport::Channel>,
    Arc<MockApi>,
    oneshot::Sender<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let api = Arc::new(MockApi {
        behavior,
        contexts: Mutex::new(Vec::new()),
    });
    let service = AuctionGrpcService::new(
        Arc::clone(&api) as Arc<dyn AuctionGrpcApi>,
        Arc::new(FixedClock),
    );
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    tokio::spawn(async move {
        Server::builder()
            .add_service(AuctionServiceServer::new(service))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    let client = AuctionServiceClient::connect(format!("http://{address}"))
        .await
        .unwrap();
    (client, api, shutdown_tx)
}

fn trusted<T>(message: T, timeout: Duration) -> Request<T> {
    let mut request = Request::new(message);
    request
        .metadata_mut()
        .insert(PRINCIPAL_TYPE_HEADER, "service_account".parse().unwrap());
    request
        .metadata_mut()
        .insert(PRINCIPAL_ID_HEADER, "processes".parse().unwrap());
    request
        .metadata_mut()
        .insert(CORRELATION_ID_HEADER, "correlation-1".parse().unwrap());
    request
        .metadata_mut()
        .insert(CAUSATION_ID_HEADER, "request-1".parse().unwrap());
    request.set_timeout(timeout);
    request
}

fn close_request(key: &str) -> CloseAuctionRequest {
    CloseAuctionRequest {
        auction_id: "auction-1".to_owned(),
        now: Some(prost_types::Timestamp {
            seconds: 1_700_000_000,
            nanos: 0,
        }),
        idempotency_key: key.to_owned(),
    }
}

#[tokio::test]
async fn all_rpc_outcomes_are_typed_and_identity_is_forwarded() {
    let (mut client, api, shutdown) = client(Behavior::Success).await;
    let timeout = Duration::from_secs(1);
    let close = client
        .close_auction(trusted(close_request("close-1"), timeout))
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        close.outcome,
        Some(close_auction_response::Outcome::Closed(_))
    ));

    let cancel = client
        .cancel_auction(trusted(
            CancelAuctionRequest {
                auction_id: "auction-1".to_owned(),
                idempotency_key: "cancel-1".to_owned(),
            },
            timeout,
        ))
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        cancel.outcome,
        Some(cancel_auction_response::Outcome::Cancelled(_))
    ));

    let forfeit = client
        .forfeit_settlement_candidate(trusted(
            ForfeitSettlementCandidateRequest {
                auction_id: "auction-1".to_owned(),
                candidate_id: "user-1".to_owned(),
                settlement_round: 2,
                idempotency_key: "forfeit-1".to_owned(),
            },
            timeout,
        ))
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        forfeit.outcome,
        Some(forfeit_settlement_candidate_response::Outcome::Advanced(_))
    ));

    let confirm = client
        .confirm_winner(trusted(
            ConfirmWinnerRequest {
                auction_id: "auction-1".to_owned(),
                candidate_id: "user-1".to_owned(),
                payment_id: "payment-1".to_owned(),
                idempotency_key: "confirm-1".to_owned(),
            },
            timeout,
        ))
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        confirm.outcome,
        Some(confirm_winner_response::Outcome::Confirmed(_))
    ));

    let contexts = api.contexts.lock().unwrap();
    assert_eq!(contexts.len(), 4);
    assert!(
        contexts
            .iter()
            .all(|context| context.principal.id == "processes")
    );
    assert_eq!(contexts[0].idempotency_key, "close-1");
    let _ = shutdown.send(());
}

#[tokio::test]
async fn missing_identity_is_unauthenticated() {
    let (mut client, _, shutdown) = client(Behavior::Success).await;
    let error = client
        .close_auction(Request::new(close_request("close-1")))
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::Unauthenticated);
    let _ = shutdown.send(());
}

#[tokio::test]
async fn domain_rejection_uses_oneof_and_retryable_failure_uses_status() {
    let (mut domain_client, _, domain_shutdown) = client(Behavior::DomainRejection).await;
    let response = domain_client
        .close_auction(trusted(
            close_request("close-domain"),
            Duration::from_secs(1),
        ))
        .await
        .unwrap()
        .into_inner();
    assert!(
        matches!(response.outcome, Some(close_auction_response::Outcome::Rejection(rejection)) if rejection.reason == "too_early")
    );
    let _ = domain_shutdown.send(());

    let (mut unavailable_client, _, unavailable_shutdown) = client(Behavior::Unavailable).await;
    let error = unavailable_client
        .close_auction(trusted(
            close_request("close-unavailable"),
            Duration::from_secs(1),
        ))
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::Unavailable);
    let _ = unavailable_shutdown.send(());
}

#[tokio::test]
async fn grpc_timeout_cancels_the_application_future() {
    let api = Arc::new(MockApi {
        behavior: Behavior::Delayed,
        contexts: Mutex::new(Vec::new()),
    });
    let service = AuctionGrpcService::new(api, Arc::new(FixedClock));
    let mut request = trusted(close_request("close-timeout"), Duration::from_secs(1));
    request
        .metadata_mut()
        .insert("grpc-timeout", "30m".parse().unwrap());
    let error = service.close_auction(request).await.unwrap_err();
    assert_eq!(error.code(), Code::DeadlineExceeded);
}
