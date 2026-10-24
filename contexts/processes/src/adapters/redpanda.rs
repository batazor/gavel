//! Production Redpanda adapter. Redpanda is Kafka-protocol compatible, so the
//! adapter deliberately depends on `rdkafka` while the ingress policy remains
//! broker-neutral.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use rdkafka::{
    ClientConfig, Message, Offset, TopicPartitionList,
    consumer::{CommitMode, Consumer, StreamConsumer},
    producer::{FutureProducer, FutureRecord},
    util::Timeout,
};

use crate::ingress::{BrokerDelivery, ConsumedRecord, DeliveryError, QuarantineRecord};

pub const AUCTION_PUBLIC_TOPIC: &str = "gavel.auction.public.v1";
pub const AUCTION_RESTRICTED_TOPIC: &str = "gavel.auction.processes.v1";
pub const PAYMENTS_ATTEMPTS_TOPIC: &str = "gavel.payments.attempts.v1";
pub const QUARANTINE_TOPIC: &str = "gavel.processes.quarantine.v1";
pub const DEFAULT_CONSUMER_GROUP: &str = "gavel-processes-v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RedpandaConfig {
    pub brokers: String,
    pub group_id: String,
    pub client_id: String,
    pub security_protocol: String,
    pub sasl_username: Option<String>,
    pub sasl_password: Option<String>,
    pub quarantine_timeout: Duration,
}

impl RedpandaConfig {
    pub fn validate(&self) -> Result<(), DeliveryError> {
        if self.brokers.trim().is_empty()
            || self.group_id.trim().is_empty()
            || self.client_id.trim().is_empty()
            || self.quarantine_timeout.is_zero()
        {
            return Err(DeliveryError::InvalidConfiguration);
        }
        if self.sasl_username.is_some() != self.sasl_password.is_some() {
            return Err(DeliveryError::InvalidConfiguration);
        }
        Ok(())
    }
}

pub struct RedpandaBroker {
    consumer: StreamConsumer,
    producer: FutureProducer,
    quarantine_timeout: Duration,
}

impl RedpandaBroker {
    pub fn connect(config: &RedpandaConfig) -> Result<Arc<Self>, DeliveryError> {
        config.validate()?;
        let mut consumer_config = ClientConfig::new();
        configure_common(&mut consumer_config, config);
        consumer_config
            .set("group.id", &config.group_id)
            .set("enable.auto.commit", "false")
            .set("enable.auto.offset.store", "false")
            .set("auto.offset.reset", "earliest")
            .set("partition.assignment.strategy", "cooperative-sticky")
            .set("isolation.level", "read_committed");
        let consumer: StreamConsumer = consumer_config
            .create()
            .map_err(|_| DeliveryError::Broker)?;
        consumer
            .subscribe(&[
                AUCTION_PUBLIC_TOPIC,
                AUCTION_RESTRICTED_TOPIC,
                PAYMENTS_ATTEMPTS_TOPIC,
            ])
            .map_err(|_| DeliveryError::Broker)?;
        let mut producer_config = ClientConfig::new();
        configure_common(&mut producer_config, config);
        let producer = producer_config
            .create()
            .map_err(|_| DeliveryError::Broker)?;
        Ok(Arc::new(Self {
            consumer,
            producer,
            quarantine_timeout: config.quarantine_timeout,
        }))
    }

    fn commit_next_offset(&self, record: &ConsumedRecord) -> Result<(), DeliveryError> {
        let next = record.offset.checked_add(1).ok_or(DeliveryError::Broker)?;
        let mut offsets = TopicPartitionList::new();
        offsets
            .add_partition_offset(&record.topic, record.partition, Offset::Offset(next))
            .map_err(|_| DeliveryError::Broker)?;
        self.consumer
            .commit(&offsets, CommitMode::Sync)
            .map_err(|_| DeliveryError::Broker)
    }

    /// Bounded metadata probe used by readiness. It verifies the broker and all
    /// owned ingress/DLQ topics without consuming or logging record contents.
    pub fn is_ready(&self, timeout: Duration) -> bool {
        self.consumer
            .fetch_metadata(None, timeout)
            .ok()
            .is_some_and(|metadata| {
                [
                    AUCTION_PUBLIC_TOPIC,
                    AUCTION_RESTRICTED_TOPIC,
                    PAYMENTS_ATTEMPTS_TOPIC,
                    QUARANTINE_TOPIC,
                ]
                .iter()
                .all(|expected| {
                    metadata
                        .topics()
                        .iter()
                        .any(|topic| topic.name() == *expected && !topic.partitions().is_empty())
                })
            })
    }
}

fn configure_common(client: &mut ClientConfig, config: &RedpandaConfig) {
    client
        .set("bootstrap.servers", &config.brokers)
        .set("client.id", &config.client_id)
        .set("security.protocol", &config.security_protocol);
    if let (Some(username), Some(password)) = (&config.sasl_username, &config.sasl_password) {
        client
            .set("sasl.mechanism", "SCRAM-SHA-256")
            .set("sasl.username", username)
            .set("sasl.password", password);
    }
}

#[async_trait]
impl BrokerDelivery for RedpandaBroker {
    async fn receive(&self) -> Result<ConsumedRecord, DeliveryError> {
        let message = self
            .consumer
            .recv()
            .await
            .map_err(|_| DeliveryError::Broker)?;
        // A compacted-topic tombstone is an invalid envelope, not a transport
        // failure; pass an empty payload through validation so it is quarantined.
        let payload = message.payload().unwrap_or_default().to_vec();
        Ok(ConsumedRecord {
            topic: message.topic().to_owned(),
            partition: message.partition(),
            offset: message.offset(),
            key: message.key().map(ToOwned::to_owned),
            payload,
        })
    }

    async fn commit(&self, record: &ConsumedRecord) -> Result<(), DeliveryError> {
        self.commit_next_offset(record)
    }

    async fn quarantine(&self, record: &QuarantineRecord) -> Result<(), DeliveryError> {
        let payload =
            serde_json::to_vec(record).map_err(|_| DeliveryError::QuarantineUnavailable)?;
        self.producer
            .send(
                FutureRecord::to(QUARANTINE_TOPIC)
                    .key(&record.source.partition_key_hash)
                    .payload(&payload),
                Timeout::After(self.quarantine_timeout),
            )
            .await
            .map_err(|_| DeliveryError::QuarantineUnavailable)?;
        Ok(())
    }
}
