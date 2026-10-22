use prost::Message;
use prost_types::{DescriptorProto, FileDescriptorSet};

const FORBIDDEN_FIELDS: &[&str] = &[
    "maximum",
    "user_id",
    "bidder_id",
    "leader_id",
    "storage_key",
    "payment_id",
    "provider_object",
];

fn inspect_message(message: &DescriptorProto, prefix: &str, violations: &mut Vec<String>) {
    let message_name = message.name.as_deref().unwrap_or("<unnamed>");
    let qualified = format!("{prefix}.{message_name}");

    for field in &message.field {
        let field_name = field.name.as_deref().unwrap_or("<unnamed>");
        if FORBIDDEN_FIELDS
            .iter()
            .any(|forbidden| field_name.contains(forbidden))
        {
            violations.push(format!("{qualified}.{field_name}"));
        }
    }

    for nested in &message.nested_type {
        inspect_message(nested, &qualified, violations);
    }
}

#[test]
fn public_event_descriptor_cannot_represent_private_fields() {
    let descriptor = FileDescriptorSet::decode(gavel_auction::AUCTION_DESCRIPTOR_SET)
        .expect("generated Auction descriptor set must decode");
    let mut violations = Vec::new();

    for file in descriptor.file {
        if file.name.as_deref() != Some("gavel/auction/v1/auction_events.proto") {
            continue;
        }
        for message in &file.message_type {
            inspect_message(message, "gavel.auction.v1", &mut violations);
        }
    }

    assert!(
        violations.is_empty(),
        "public event contract exposes private fields: {violations:?}"
    );
}

#[test]
fn public_event_fixture_contains_only_safe_shape() {
    let fixture = include_str!("fixtures/public_events_v1.json").to_ascii_lowercase();

    for forbidden in FORBIDDEN_FIELDS {
        assert!(
            !fixture.contains(forbidden),
            "public event fixture contains forbidden field {forbidden}"
        );
    }
    assert!(fixture.contains("bidderalias"));
    assert!(fixture.contains("resultingprice"));
}

#[test]
fn current_price_cache_fixture_contains_only_public_versioned_data() {
    let fixture = include_str!("fixtures/current_price_cache_v1.json").to_ascii_lowercase();
    for forbidden in FORBIDDEN_FIELDS {
        assert!(
            !fixture.contains(forbidden),
            "cache fixture contains forbidden field {forbidden}"
        );
    }
    for required in [
        "auctionid",
        "amountminor",
        "currencycode",
        "version",
        "updatedatmillis",
    ] {
        assert!(
            fixture.contains(required),
            "cache fixture misses {required}"
        );
    }
}
