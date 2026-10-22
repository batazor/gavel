use super::{
    ApplicationError, PermissionCheck, PrincipalReference, Relationship, ResourceReference,
};

const MAX_IDENTIFIER_BYTES: usize = 1024;
const MAX_TOKEN_BYTES: usize = 4096;

const PLATFORM_PERMISSIONS: &[&str] = &[
    "administer",
    "create_lot",
    "participate_in_auction",
    "register_payment_instrument",
    "operate_catalog",
    "operate_auctions",
    "operate_payments",
    "send_notifications",
];
const LOT_PERMISSIONS: &[&str] = &[
    "admin_override",
    "is_seller",
    "edit",
    "withdraw",
    "execute_withdraw",
    "reschedule_auction",
    "view_seller_state",
];
const AUCTION_PERMISSIONS: &[&str] = &[
    "place_bid",
    "view_personal_state",
    "reschedule",
    "close",
    "cancel",
    "reopen",
    "settle",
];
const PAYMENT_PROFILE_PERMISSIONS: &[&str] = &["manage", "view"];
const PAYMENT_ATTEMPT_PERMISSIONS: &[&str] = &["view", "continue_payment", "process"];
const NOTIFICATION_PERMISSIONS: &[&str] = &["view", "deliver"];
const OPERATION_PERMISSIONS: &[&str] = &["view"];

pub fn validate_permission_check(check: &PermissionCheck) -> Result<(), ApplicationError> {
    validate_resource(&check.resource)?;
    validate_principal(&check.principal)?;

    let allowed = match check.resource.resource_type.as_str() {
        "platform" => PLATFORM_PERMISSIONS,
        "lot" => LOT_PERMISSIONS,
        "auction" => AUCTION_PERMISSIONS,
        "payment_profile" => PAYMENT_PROFILE_PERMISSIONS,
        "payment_attempt" => PAYMENT_ATTEMPT_PERMISSIONS,
        "notification" => NOTIFICATION_PERMISSIONS,
        "operation" => OPERATION_PERMISSIONS,
        _ => return Err(invalid("unknown_resource_type", "resource type is unknown")),
    };
    if !allowed.contains(&check.permission.as_str()) {
        return Err(invalid("unknown_permission", "permission is unknown"));
    }
    Ok(())
}

pub fn validate_consistency_token(token: Option<&str>) -> Result<(), ApplicationError> {
    if let Some(token) = token {
        if token.is_empty() || token.len() > MAX_TOKEN_BYTES || token.chars().any(char::is_control)
        {
            return Err(invalid(
                "invalid_consistency_token",
                "consistency token is invalid",
            ));
        }
    }
    Ok(())
}

pub fn validate_relationship(relationship: &Relationship) -> Result<(), ApplicationError> {
    validate_resource(&relationship.resource)?;
    validate_identifier(&relationship.subject.principal_id, "principal_id")?;

    let expected_subject_type = match (
        relationship.resource.resource_type.as_str(),
        relationship.relation.as_str(),
    ) {
        ("platform", "administrator" | "member") => "user",
        ("platform", "process_worker") => "service_account",
        ("lot", "platform") => "platform",
        ("lot", "seller") => "user",
        ("auction", "platform") => "platform",
        ("auction", "lot") => "lot",
        ("auction", "participant") => "user",
        ("payment_profile", "owner") => "user",
        ("payment_attempt", "platform") => "platform",
        ("payment_attempt", "auction") => "auction",
        ("payment_attempt", "owner") => "user",
        ("notification", "platform") => "platform",
        ("notification", "recipient") => "user",
        ("operation", "owner") => "user",
        (resource_type, _) if !is_resource_type(resource_type) => {
            return Err(invalid("unknown_resource_type", "resource type is unknown"));
        }
        _ => {
            return Err(invalid(
                "unknown_relation",
                "relationship relation is unknown",
            ));
        }
    };
    if relationship.subject.principal_type != expected_subject_type {
        return Err(invalid(
            "invalid_subject_type",
            "relationship subject type is invalid",
        ));
    }
    Ok(())
}

fn is_resource_type(value: &str) -> bool {
    matches!(
        value,
        "platform"
            | "lot"
            | "auction"
            | "payment_profile"
            | "payment_attempt"
            | "notification"
            | "operation"
    )
}

fn validate_resource(resource: &ResourceReference) -> Result<(), ApplicationError> {
    if resource.resource_type.is_empty() {
        return Err(invalid(
            "missing_resource_type",
            "resource type is required",
        ));
    }
    validate_identifier(&resource.resource_id, "resource_id")
}

fn validate_principal(principal: &PrincipalReference) -> Result<(), ApplicationError> {
    if !matches!(
        principal.principal_type.as_str(),
        "user" | "service_account"
    ) {
        return Err(invalid(
            "unknown_principal_type",
            "principal type is unknown",
        ));
    }
    validate_identifier(&principal.principal_id, "principal_id")
}

fn validate_identifier(value: &str, field: &'static str) -> Result<(), ApplicationError> {
    let valid = !value.is_empty()
        && value.len() <= MAX_IDENTIFIER_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/'));
    if valid {
        Ok(())
    } else {
        Err(invalid(
            match field {
                "resource_id" => "invalid_resource_id",
                _ => "invalid_principal_id",
            },
            match field {
                "resource_id" => "resource id is invalid",
                _ => "principal id is invalid",
            },
        ))
    }
}

fn invalid(reason: &'static str, safe_message: &'static str) -> ApplicationError {
    ApplicationError::InvalidArgument {
        reason,
        safe_message: safe_message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(resource_type: &str, permission: &str, principal_type: &str) -> PermissionCheck {
        PermissionCheck {
            resource: ResourceReference {
                resource_type: resource_type.to_owned(),
                resource_id: "resource-1".to_owned(),
            },
            permission: permission.to_owned(),
            principal: PrincipalReference {
                principal_type: principal_type.to_owned(),
                principal_id: "u1_principal".to_owned(),
            },
        }
    }

    #[test]
    fn permissions_match_the_checked_in_schema() {
        let valid = [
            ("platform", "create_lot"),
            ("lot", "edit"),
            ("auction", "place_bid"),
            ("payment_profile", "manage"),
            ("payment_attempt", "continue_payment"),
            ("notification", "deliver"),
            ("operation", "view"),
        ];
        for (resource_type, permission) in valid {
            assert!(validate_permission_check(&check(resource_type, permission, "user")).is_ok());
        }
    }

    #[test]
    fn empty_unknown_and_malformed_references_are_rejected() {
        let mut cases = vec![
            check("mystery", "view", "user"),
            check("auction", "destroy", "user"),
            check("auction", "place_bid", "robot"),
        ];
        let mut empty_resource = check("auction", "place_bid", "user");
        empty_resource.resource.resource_id.clear();
        cases.push(empty_resource);
        let mut malformed_principal = check("auction", "place_bid", "user");
        malformed_principal.principal.principal_id = "contains whitespace".to_owned();
        cases.push(malformed_principal);

        for invalid_check in cases {
            assert!(matches!(
                validate_permission_check(&invalid_check),
                Err(ApplicationError::InvalidArgument { .. })
            ));
        }
    }
}
