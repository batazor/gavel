use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    application::{AuctionEventInbox, FreezeFromAuction, FreezeInboxOutcome, PortError},
    domain::LotId,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsumeOutcome {
    Applied,
    Duplicate,
    Stale,
    Quarantined,
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConsumerError {
    #[error("event payload is malformed")]
    Malformed,
    #[error("event schema is unsupported")]
    Unsupported,
    #[error("catalog inbox is unavailable")]
    Unavailable,
    #[error("catalog inbox failed")]
    Internal,
}

pub struct AuctionBidConsumer<'a, I> {
    inbox: &'a I,
}

impl<'a, I> AuctionBidConsumer<'a, I>
where
    I: AuctionEventInbox,
{
    pub const fn new(inbox: &'a I) -> Self {
        Self { inbox }
    }

    pub async fn handle(&self, bytes: &[u8]) -> Result<ConsumeOutcome, ConsumerError> {
        let envelope: AuctionEnvelope =
            serde_json::from_slice(bytes).map_err(|_| ConsumerError::Malformed)?;
        if envelope.schema_version != 1 {
            return Err(ConsumerError::Unsupported);
        }
        if envelope.event_id.is_empty()
            || envelope.aggregate_id.is_empty()
            || envelope.aggregate_version == 0
            || envelope.bid_recorded.bid_id.is_empty()
        {
            return Err(ConsumerError::Malformed);
        }
        let lot_id = LotId::new(envelope.aggregate_id).map_err(|_| ConsumerError::Malformed)?;
        let outcome = self
            .inbox
            .freeze_from_first_bid(FreezeFromAuction {
                event_id: envelope.event_id,
                payload_fingerprint: fingerprint(bytes),
                lot_id,
                source_version: envelope.aggregate_version,
            })
            .await
            .map_err(map_port)?;
        Ok(match outcome {
            FreezeInboxOutcome::Applied => ConsumeOutcome::Applied,
            FreezeInboxOutcome::Duplicate => ConsumeOutcome::Duplicate,
            FreezeInboxOutcome::Stale => ConsumeOutcome::Stale,
            FreezeInboxOutcome::Quarantined => ConsumeOutcome::Quarantined,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuctionEnvelope {
    event_id: String,
    aggregate_id: String,
    aggregate_version: u64,
    schema_version: u32,
    #[allow(dead_code)]
    occurred_at: String,
    #[allow(dead_code)]
    correlation_id: String,
    #[allow(dead_code)]
    causation_id: String,
    bid_recorded: BidRecorded,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BidRecorded {
    bid_id: String,
    #[allow(dead_code)]
    bidder_alias: String,
    #[allow(dead_code)]
    resulting_price: Price,
    #[allow(dead_code)]
    recorded_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Price {
    #[allow(dead_code)]
    minor_units: u64,
    #[allow(dead_code)]
    currency_code: String,
}

fn fingerprint(bytes: &[u8]) -> String {
    base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        Sha256::digest(bytes),
    )
}

fn map_port(error: PortError) -> ConsumerError {
    match error {
        PortError::Timeout | PortError::Unavailable => ConsumerError::Unavailable,
        PortError::NotFound | PortError::Internal => ConsumerError::Internal,
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use async_trait::async_trait;

    use super::*;

    #[derive(Default)]
    struct State {
        events: HashMap<String, String>,
        versions: HashMap<LotId, u64>,
        freezes: usize,
    }

    #[derive(Default)]
    struct FakeInbox(Mutex<State>);

    #[async_trait]
    impl AuctionEventInbox for FakeInbox {
        async fn freeze_from_first_bid(
            &self,
            event: FreezeFromAuction,
        ) -> Result<FreezeInboxOutcome, PortError> {
            let mut state = self.0.lock().unwrap();
            if let Some(existing) = state.events.get(&event.event_id) {
                return Ok(if existing == &event.payload_fingerprint {
                    FreezeInboxOutcome::Duplicate
                } else {
                    FreezeInboxOutcome::Quarantined
                });
            }
            state
                .events
                .insert(event.event_id, event.payload_fingerprint);
            if state
                .versions
                .get(&event.lot_id)
                .is_some_and(|version| *version >= event.source_version)
            {
                return Ok(FreezeInboxOutcome::Stale);
            }
            state.versions.insert(event.lot_id, event.source_version);
            state.freezes += 1;
            Ok(FreezeInboxOutcome::Applied)
        }
    }

    fn fixture() -> Vec<u8> {
        include_bytes!("../../../tests/fixtures/auction_bid_recorded_v1.json").to_vec()
    }

    #[tokio::test]
    async fn fixture_applies_once_and_duplicate_or_changed_redelivery_is_safe() {
        let inbox = FakeInbox::default();
        let consumer = AuctionBidConsumer::new(&inbox);
        let bytes = fixture();
        assert_eq!(
            consumer.handle(&bytes).await.unwrap(),
            ConsumeOutcome::Applied
        );
        assert_eq!(
            consumer.handle(&bytes).await.unwrap(),
            ConsumeOutcome::Duplicate
        );
        let changed = String::from_utf8(bytes)
            .unwrap()
            .replace("12500", "12600")
            .into_bytes();
        assert_eq!(
            consumer.handle(&changed).await.unwrap(),
            ConsumeOutcome::Quarantined
        );
        assert_eq!(inbox.0.lock().unwrap().freezes, 1);
    }

    #[tokio::test]
    async fn stale_source_version_does_not_unfreeze_or_apply_again() {
        let inbox = FakeInbox::default();
        let consumer = AuctionBidConsumer::new(&inbox);
        let newer = String::from_utf8(fixture())
            .unwrap()
            .replace("\"aggregateVersion\": 7", "\"aggregateVersion\": 8")
            .replace("evt-01JTEST", "evt-newer")
            .into_bytes();
        assert_eq!(
            consumer.handle(&newer).await.unwrap(),
            ConsumeOutcome::Applied
        );
        let older = String::from_utf8(fixture())
            .unwrap()
            .replace("evt-01JTEST", "evt-older")
            .into_bytes();
        assert_eq!(
            consumer.handle(&older).await.unwrap(),
            ConsumeOutcome::Stale
        );
        assert_eq!(inbox.0.lock().unwrap().freezes, 1);
    }

    #[tokio::test]
    async fn unknown_schema_version_is_rejected_before_inbox() {
        let inbox = FakeInbox::default();
        let consumer = AuctionBidConsumer::new(&inbox);
        let unsupported = String::from_utf8(fixture())
            .unwrap()
            .replace("\"schemaVersion\": 1", "\"schemaVersion\": 99")
            .into_bytes();
        assert_eq!(
            consumer.handle(&unsupported).await.unwrap_err(),
            ConsumerError::Unsupported
        );
        assert!(inbox.0.lock().unwrap().events.is_empty());
    }
}
