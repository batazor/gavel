use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use gavel_catalog::{
    adapters::inbound::{
        grpc::{CatalogGrpcApi, CatalogGrpcService},
        workload_identity::WorkloadIdentityInterceptor,
    },
    application::{ApplicationError, Clock, LotMutationResult, WithdrawLotInput},
    rpc::{
        WithdrawLotRequest, catalog_service_client::CatalogServiceClient,
        catalog_service_server::CatalogServiceServer, withdraw_lot_response::Outcome,
    },
};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Code, Request, transport::Server};

struct FixedClock;

impl Clock for FixedClock {
    fn now_millis(&self) -> u64 {
        1_760_000_000_000
    }
}

#[derive(Default)]
struct RecordingApi {
    calls: Mutex<Vec<WithdrawLotInput>>,
}

#[async_trait]
impl CatalogGrpcApi for RecordingApi {
    async fn withdraw(
        &self,
        input: WithdrawLotInput,
    ) -> Result<LotMutationResult, ApplicationError> {
        self.calls.lock().unwrap().push(input.clone());
        if input.lot_id == "dependency-failure" {
            return Err(ApplicationError::DependencyUnavailable {
                dependency: "postgresql://secret@internal",
            });
        }
        Ok(LotMutationResult {
            lot_id: input.lot_id,
            version: 3,
            checked_at_token: "opaque-revision".to_owned(),
        })
    }
}

async fn client(
    api: Arc<RecordingApi>,
) -> (
    CatalogServiceClient<tonic::transport::Channel>,
    tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service =
        CatalogGrpcService::new(api, Arc::new(FixedClock), Duration::from_secs(2)).unwrap();
    let interceptor = WorkloadIdentityInterceptor::test_processes(true).unwrap();
    let server = tokio::spawn(
        Server::builder()
            .add_service(CatalogServiceServer::with_interceptor(service, interceptor))
            .serve_with_incoming(TcpListenerStream::new(listener)),
    );
    let client = CatalogServiceClient::connect(format!("http://{address}"))
        .await
        .unwrap();
    (client, server)
}

fn request(lot_id: &str, idempotency_key: &str) -> Request<WithdrawLotRequest> {
    let mut request = Request::new(WithdrawLotRequest {
        lot_id: lot_id.to_owned(),
        idempotency_key: idempotency_key.to_owned(),
        correlation_id: "process-workflow-42".to_owned(),
        consistency_token: "causal-token".to_owned(),
    });
    request
        .metadata_mut()
        .insert("grpc-timeout", "500m".parse().unwrap());
    request
}

#[tokio::test]
async fn generated_client_and_server_preserve_the_processes_contract() {
    let api = Arc::new(RecordingApi::default());
    let (mut client, server) = client(Arc::clone(&api)).await;

    for _ in 0..2 {
        let response = client
            .withdraw_lot(request("lot-42", "withdraw-command-42"))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(
            response.outcome,
            Some(Outcome::Withdrawn(gavel_catalog::rpc::LotWithdrawn {
                version: 3,
                checked_at_token: "opaque-revision".to_owned(),
            }))
        );
    }

    let calls = api.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|call| {
        call.context.idempotency_key == "withdraw-command-42"
            && call.context.correlation_id == "process-workflow-42"
            && call.context.consistency_token.as_deref() == Some("causal-token")
    }));
    drop(calls);
    server.abort();
}

#[tokio::test]
async fn generated_transport_requires_deadline_and_redacts_dependency_details() {
    let api = Arc::new(RecordingApi::default());
    let (mut client, server) = client(api).await;

    let mut missing_deadline = request("lot-42", "withdraw-command-42");
    missing_deadline.metadata_mut().remove("grpc-timeout");
    assert_eq!(
        client
            .withdraw_lot(missing_deadline)
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );

    let failure = client
        .withdraw_lot(request("dependency-failure", "dependency-command"))
        .await
        .unwrap_err();
    assert_eq!(failure.code(), Code::Unavailable);
    assert!(!failure.message().contains("secret"));
    assert!(!failure.message().contains("internal"));
    server.abort();
}
