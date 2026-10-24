//! Restart-safe Catalog and Payments event delivery into Auction-owned handlers.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use rdkafka::{
    ClientConfig, Message, Offset, TopicPartitionList,
    consumer::{CommitMode, Consumer, StreamConsumer},
};
use tokio_util::sync::CancellationToken;

use crate::{
    application::{AuctionUnitOfWork, Inbox, PaymentReadinessStore},
    bootstrap::BackgroundWorker,
};

use super::events::{CatalogEventConsumer, PaymentsEventConsumer};

pub const CATALOG_EVENTS_TOPIC: &str = "gavel.catalog.events.v1";
pub const PAYMENTS_INSTRUMENTS_TOPIC: &str = "gavel.payments.instruments.v1";
pub const DEFAULT_CONSUMER_GROUP: &str = "gavel-auction-owner-events-v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RedpandaConfig {
    pub brokers: String,
    pub group_id: String,
    pub client_id: String,
    pub security_protocol: String,
    pub sasl_username: Option<String>,
    pub sasl_password: Option<String>,
}

impl RedpandaConfig {
    fn validate(&self) -> Result<(), OwnerDeliveryError> {
        if self.brokers.trim().is_empty()
            || self.group_id.trim().is_empty()
            || self.client_id.trim().is_empty()
            || self.sasl_username.is_some() != self.sasl_password.is_some()
        {
            return Err(OwnerDeliveryError::InvalidConfiguration);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnerRecord {
    pub topic: String,
    pub partition: i32,
    pub offset: i64,
    pub payload: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum OwnerDeliveryError {
    #[error("owner event delivery configuration is invalid")]
    InvalidConfiguration,
    #[error("owner event broker operation failed")]
    Broker,
    #[error("owner event route is not registered")]
    UnknownRoute,
    #[error("owner event handler rejected the record")]
    Handler,
}

#[async_trait]
pub trait OwnerBroker: Send + Sync {
    async fn receive(&self) -> Result<OwnerRecord, OwnerDeliveryError>;
    async fn commit(&self, record: &OwnerRecord) -> Result<(), OwnerDeliveryError>;
}

#[async_trait]
pub trait OwnerEventHandler: Send + Sync {
    async fn handle(&self, record: &OwnerRecord) -> Result<(), OwnerDeliveryError>;
}

pub struct RedpandaOwnerBroker {
    consumer: StreamConsumer,
}

impl RedpandaOwnerBroker {
    pub fn connect(config: &RedpandaConfig) -> Result<Arc<Self>, OwnerDeliveryError> {
        config.validate()?;
        let mut client = ClientConfig::new();
        client
            .set("bootstrap.servers", &config.brokers)
            .set("client.id", &config.client_id)
            .set("group.id", &config.group_id)
            .set("security.protocol", &config.security_protocol)
            .set("enable.auto.commit", "false")
            .set("enable.auto.offset.store", "false")
            .set("auto.offset.reset", "earliest")
            .set("partition.assignment.strategy", "cooperative-sticky")
            .set("isolation.level", "read_committed");
        if let (Some(username), Some(password)) = (&config.sasl_username, &config.sasl_password) {
            client
                .set("sasl.mechanism", "SCRAM-SHA-256")
                .set("sasl.username", username)
                .set("sasl.password", password);
        }
        let consumer: StreamConsumer = client.create().map_err(|_| OwnerDeliveryError::Broker)?;
        consumer
            .subscribe(&[CATALOG_EVENTS_TOPIC, PAYMENTS_INSTRUMENTS_TOPIC])
            .map_err(|_| OwnerDeliveryError::Broker)?;
        Ok(Arc::new(Self { consumer }))
    }

    pub fn is_ready(&self, timeout: Duration) -> bool {
        self.consumer
            .fetch_metadata(None, timeout)
            .ok()
            .is_some_and(|metadata| {
                [CATALOG_EVENTS_TOPIC, PAYMENTS_INSTRUMENTS_TOPIC]
                    .iter()
                    .all(|expected| {
                        metadata.topics().iter().any(|topic| {
                            topic.name() == *expected && !topic.partitions().is_empty()
                        })
                    })
            })
    }
}

#[async_trait]
impl OwnerBroker for RedpandaOwnerBroker {
    async fn receive(&self) -> Result<OwnerRecord, OwnerDeliveryError> {
        let message = self
            .consumer
            .recv()
            .await
            .map_err(|_| OwnerDeliveryError::Broker)?;
        Ok(OwnerRecord {
            topic: message.topic().to_owned(),
            partition: message.partition(),
            offset: message.offset(),
            payload: message.payload().unwrap_or_default().to_vec(),
        })
    }

    async fn commit(&self, record: &OwnerRecord) -> Result<(), OwnerDeliveryError> {
        let next = record
            .offset
            .checked_add(1)
            .ok_or(OwnerDeliveryError::Broker)?;
        let mut offsets = TopicPartitionList::new();
        offsets
            .add_partition_offset(&record.topic, record.partition, Offset::Offset(next))
            .map_err(|_| OwnerDeliveryError::Broker)?;
        self.consumer
            .commit(&offsets, CommitMode::Sync)
            .map_err(|_| OwnerDeliveryError::Broker)
    }
}

pub struct AuctionOwnerEventHandler<U> {
    catalog: CatalogEventConsumer<U, U>,
    payments: PaymentsEventConsumer<U, U>,
}

impl<U> AuctionOwnerEventHandler<U>
where
    U: AuctionUnitOfWork + Inbox + PaymentReadinessStore,
{
    pub fn new(store: Arc<U>, retry_attempts: usize, fingerprint_key: [u8; 32]) -> Self {
        Self {
            catalog: CatalogEventConsumer::new(
                Arc::clone(&store),
                Arc::clone(&store),
                retry_attempts,
                fingerprint_key,
            ),
            payments: PaymentsEventConsumer::new(Arc::clone(&store), store),
        }
    }
}

#[async_trait]
impl<U> OwnerEventHandler for AuctionOwnerEventHandler<U>
where
    U: AuctionUnitOfWork + Inbox + PaymentReadinessStore + 'static,
{
    async fn handle(&self, record: &OwnerRecord) -> Result<(), OwnerDeliveryError> {
        match record.topic.as_str() {
            CATALOG_EVENTS_TOPIC => self.catalog.handle(&record.payload).await,
            PAYMENTS_INSTRUMENTS_TOPIC => self.payments.handle(&record.payload).await,
            _ => return Err(OwnerDeliveryError::UnknownRoute),
        }
        .map(|_| ())
        .map_err(|_| OwnerDeliveryError::Handler)
    }
}

pub struct OwnerEventWorker<B, H> {
    broker: Arc<B>,
    handler: Arc<H>,
}

impl<B, H> OwnerEventWorker<B, H>
where
    B: OwnerBroker,
    H: OwnerEventHandler,
{
    pub fn new(broker: Arc<B>, handler: Arc<H>) -> Self {
        Self { broker, handler }
    }

    pub async fn run_once(&self) -> Result<(), OwnerDeliveryError> {
        let record = self.broker.receive().await?;
        self.handler.handle(&record).await?;
        self.broker.commit(&record).await
    }
}

#[async_trait]
impl<B, H> BackgroundWorker for OwnerEventWorker<B, H>
where
    B: OwnerBroker + 'static,
    H: OwnerEventHandler + 'static,
{
    fn name(&self) -> &'static str {
        "owner-event-consumer"
    }

    async fn run(&self, shutdown: CancellationToken) -> Result<(), String> {
        loop {
            tokio::select! {
                () = shutdown.cancelled() => return Ok(()),
                result = self.run_once() => result.map_err(|error| error.to_string())?,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    struct FakeBroker {
        record: Mutex<Option<OwnerRecord>>,
        commits: Mutex<Vec<i64>>,
    }

    #[async_trait]
    impl OwnerBroker for FakeBroker {
        async fn receive(&self) -> Result<OwnerRecord, OwnerDeliveryError> {
            self.record
                .lock()
                .unwrap()
                .take()
                .ok_or(OwnerDeliveryError::Broker)
        }

        async fn commit(&self, record: &OwnerRecord) -> Result<(), OwnerDeliveryError> {
            self.commits.lock().unwrap().push(record.offset);
            Ok(())
        }
    }

    struct FakeHandler(bool);

    #[async_trait]
    impl OwnerEventHandler for FakeHandler {
        async fn handle(&self, _: &OwnerRecord) -> Result<(), OwnerDeliveryError> {
            self.0.then_some(()).ok_or(OwnerDeliveryError::Handler)
        }
    }

    fn broker() -> Arc<FakeBroker> {
        Arc::new(FakeBroker {
            record: Mutex::new(Some(OwnerRecord {
                topic: CATALOG_EVENTS_TOPIC.to_owned(),
                partition: 1,
                offset: 7,
                payload: b"safe-fixture".to_vec(),
            })),
            commits: Mutex::new(Vec::new()),
        })
    }

    #[tokio::test]
    async fn commits_only_after_durable_handler_result() {
        let success = broker();
        OwnerEventWorker::new(Arc::clone(&success), Arc::new(FakeHandler(true)))
            .run_once()
            .await
            .unwrap();
        assert_eq!(*success.commits.lock().unwrap(), vec![7]);

        let failed = broker();
        assert_eq!(
            OwnerEventWorker::new(Arc::clone(&failed), Arc::new(FakeHandler(false)))
                .run_once()
                .await,
            Err(OwnerDeliveryError::Handler)
        );
        assert!(failed.commits.lock().unwrap().is_empty());
    }

    #[test]
    fn configuration_requires_complete_sasl_pair() {
        let config = RedpandaConfig {
            brokers: "redpanda:9092".to_owned(),
            group_id: DEFAULT_CONSUMER_GROUP.to_owned(),
            client_id: "auction".to_owned(),
            security_protocol: "SASL_PLAINTEXT".to_owned(),
            sasl_username: Some("auction".to_owned()),
            sasl_password: None,
        };
        assert_eq!(
            config.validate(),
            Err(OwnerDeliveryError::InvalidConfiguration)
        );
    }
}
