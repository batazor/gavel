use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventEnvelope<P> {
    pub event_id: String,
    pub event_type: String,
    pub schema_version: u32,
    pub aggregate_version: u64,
    pub occurred_at_millis: u64,
    pub correlation_id: String,
    pub payload: P,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LotListedV1 {
    pub auction_id: String,
    pub lot_id: String,
    pub seller_id: String,
    pub start_price_minor: u64,
    pub currency_code: String,
    pub bid_step_minor: u64,
    pub closing_policy: ClosingPolicyV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClosingPolicyV1 {
    pub scheduled_end_millis: Option<u64>,
    pub inactivity_millis: Option<u64>,
    pub hard_deadline_millis: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LotUpdatedV1 {
    pub lot_id: String,
    pub title: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LotWithdrawnV1 {
    pub lot_id: String,
}

pub fn encode<P: Serialize>(event: &EventEnvelope<P>) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(event)
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    fn fixture(path: &str) -> Value {
        serde_json::from_slice(match path {
            "listed" => include_bytes!("../../../tests/fixtures/lot_listed_v1.json"),
            "updated" => include_bytes!("../../../tests/fixtures/lot_updated_v1.json"),
            "withdrawn" => include_bytes!("../../../tests/fixtures/lot_withdrawn_v1.json"),
            _ => unreachable!(),
        })
        .unwrap()
    }

    #[test]
    fn catalog_events_match_owned_v1_fixtures_and_are_privacy_safe() {
        let listed = EventEnvelope {
            event_id: "catalog-event-1".to_owned(),
            event_type: "LotListed".to_owned(),
            schema_version: 1,
            aggregate_version: 1,
            occurred_at_millis: 1_700_000_000_000,
            correlation_id: "catalog-correlation-1".to_owned(),
            payload: LotListedV1 {
                auction_id: "auction-1".to_owned(),
                lot_id: "lot-1".to_owned(),
                seller_id: "seller-1".to_owned(),
                start_price_minor: 100,
                currency_code: "USD".to_owned(),
                bid_step_minor: 10,
                closing_policy: ClosingPolicyV1 {
                    scheduled_end_millis: Some(1_700_000_060_000),
                    inactivity_millis: None,
                    hard_deadline_millis: None,
                },
            },
        };
        let updated = EventEnvelope {
            event_id: "catalog-event-2".to_owned(),
            event_type: "LotUpdated".to_owned(),
            schema_version: 1,
            aggregate_version: 2,
            occurred_at_millis: 1_700_000_001_000,
            correlation_id: "catalog-correlation-2".to_owned(),
            payload: LotUpdatedV1 {
                lot_id: "lot-1".to_owned(),
                title: "Updated lot".to_owned(),
                description: Some("Public description".to_owned()),
            },
        };
        let withdrawn = EventEnvelope {
            event_id: "catalog-event-3".to_owned(),
            event_type: "LotWithdrawn".to_owned(),
            schema_version: 1,
            aggregate_version: 3,
            occurred_at_millis: 1_700_000_002_000,
            correlation_id: "catalog-correlation-3".to_owned(),
            payload: LotWithdrawnV1 {
                lot_id: "lot-1".to_owned(),
            },
        };

        for (event, expected) in [
            (serde_json::to_value(&listed).unwrap(), fixture("listed")),
            (serde_json::to_value(&updated).unwrap(), fixture("updated")),
            (
                serde_json::to_value(&withdrawn).unwrap(),
                fixture("withdrawn"),
            ),
        ] {
            assert_eq!(event, expected);
            let serialized = event.to_string();
            for forbidden in [
                "objectKey",
                "uploadUrl",
                "authzRevision",
                "consistencyToken",
                "idempotencyKey",
                "credential",
            ] {
                assert!(!serialized.contains(forbidden));
            }
        }
    }
}
