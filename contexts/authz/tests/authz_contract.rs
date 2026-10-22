use gavel_authz::AUTHZ_DESCRIPTOR_SET;
use prost::Message;
use prost_types::{DescriptorProto, FileDescriptorSet};

fn message<'a>(descriptor: &'a FileDescriptorSet, name: &str) -> &'a DescriptorProto {
    descriptor
        .file
        .iter()
        .flat_map(|file| &file.message_type)
        .find(|message| message.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("missing message {name}"))
}

#[test]
fn additive_registration_and_deletion_fields_are_stable() {
    let descriptor = FileDescriptorSet::decode(AUTHZ_DESCRIPTOR_SET).unwrap();
    let service = descriptor
        .file
        .iter()
        .flat_map(|file| &file.service)
        .find(|service| service.name.as_deref() == Some("AuthzService"))
        .unwrap();
    assert!(
        service
            .method
            .iter()
            .any(|method| method.name.as_deref() == Some("RegisterPrincipal"))
    );

    let write = message(&descriptor, "WriteRelationshipsRequest");
    let fields = write
        .field
        .iter()
        .map(|field| (field.name.as_deref().unwrap(), field.number.unwrap()))
        .collect::<Vec<_>>();
    assert!(fields.contains(&("updates", 1)));
    assert!(fields.contains(&("idempotency_key", 2)));
    assert!(fields.contains(&("deletions", 3)));

    let register = message(&descriptor, "RegisterPrincipalRequest");
    assert_eq!(
        register
            .field
            .iter()
            .map(|field| (field.name.as_deref().unwrap(), field.number.unwrap()))
            .collect::<Vec<_>>(),
        vec![("issuer", 1), ("subject", 2), ("idempotency_key", 3)]
    );

    let fixture = include_str!("fixtures/authz_v1_additive.json");
    for expected in ["registerPrincipal", "deletions", "writtenAtToken"] {
        assert!(fixture.contains(expected), "fixture misses {expected}");
    }
}

#[test]
fn vendored_spicedb_contract_contains_only_the_required_service_surface() {
    let descriptor = FileDescriptorSet::decode(AUTHZ_DESCRIPTOR_SET).unwrap();
    let service = descriptor
        .file
        .iter()
        .flat_map(|file| &file.service)
        .find(|service| service.name.as_deref() == Some("PermissionsService"))
        .unwrap();
    assert_eq!(
        service
            .method
            .iter()
            .map(|method| method.name.as_deref().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "WriteRelationships",
            "CheckPermission",
            "CheckBulkPermissions"
        ]
    );
}
