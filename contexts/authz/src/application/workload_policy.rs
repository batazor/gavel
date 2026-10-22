use super::{ApplicationError, Relationship, WorkloadIdentity};

#[derive(Clone, Copy, Debug, Default)]
pub struct WorkloadPolicy;

impl WorkloadPolicy {
    pub const fn authorize_principal_registration(
        &self,
        caller: WorkloadIdentity,
    ) -> Result<(), ApplicationError> {
        if matches!(caller, WorkloadIdentity::Bff) {
            Ok(())
        } else {
            Err(ApplicationError::Forbidden)
        }
    }

    pub fn authorize_relationship(
        &self,
        caller: WorkloadIdentity,
        relationship: &Relationship,
    ) -> Result<(), ApplicationError> {
        let resource_type = relationship.resource.resource_type.as_str();
        let relation = relationship.relation.as_str();
        let allowed = match caller {
            WorkloadIdentity::Bff => resource_type == "operation" && relation == "owner",
            WorkloadIdentity::Catalog => {
                resource_type == "lot" && matches!(relation, "platform" | "seller")
            }
            WorkloadIdentity::Auction => {
                resource_type == "auction" && matches!(relation, "platform" | "lot" | "participant")
            }
            WorkloadIdentity::Payments => match resource_type {
                "payment_profile" => relation == "owner",
                "payment_attempt" => matches!(relation, "platform" | "auction" | "owner"),
                _ => false,
            },
            WorkloadIdentity::Notify => {
                resource_type == "notification" && matches!(relation, "platform" | "recipient")
            }
            WorkloadIdentity::Processes => false,
        };

        if allowed {
            Ok(())
        } else {
            Err(ApplicationError::Forbidden)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{PrincipalReference, ResourceReference};

    fn relationship(resource_type: &str, relation: &str) -> Relationship {
        Relationship {
            resource: ResourceReference {
                resource_type: resource_type.to_owned(),
                resource_id: "owned-resource".to_owned(),
            },
            relation: relation.to_owned(),
            subject: PrincipalReference {
                principal_type: "user".to_owned(),
                principal_id: "u1_subject".to_owned(),
            },
        }
    }

    #[test]
    fn every_owned_relationship_boundary_is_explicitly_allowed() {
        let cases = [
            (WorkloadIdentity::Bff, "operation", "owner"),
            (WorkloadIdentity::Catalog, "lot", "platform"),
            (WorkloadIdentity::Catalog, "lot", "seller"),
            (WorkloadIdentity::Auction, "auction", "platform"),
            (WorkloadIdentity::Auction, "auction", "lot"),
            (WorkloadIdentity::Auction, "auction", "participant"),
            (WorkloadIdentity::Payments, "payment_profile", "owner"),
            (WorkloadIdentity::Payments, "payment_attempt", "platform"),
            (WorkloadIdentity::Payments, "payment_attempt", "auction"),
            (WorkloadIdentity::Payments, "payment_attempt", "owner"),
            (WorkloadIdentity::Notify, "notification", "platform"),
            (WorkloadIdentity::Notify, "notification", "recipient"),
        ];
        let policy = WorkloadPolicy;
        for (caller, resource_type, relation) in cases {
            assert_eq!(
                policy.authorize_relationship(caller, &relationship(resource_type, relation)),
                Ok(()),
                "{} must own {resource_type}:{relation}",
                caller.as_str()
            );
        }
    }

    #[test]
    fn cross_owner_and_process_writes_are_denied() {
        let cases = [
            (WorkloadIdentity::Bff, "lot", "seller"),
            (WorkloadIdentity::Catalog, "auction", "participant"),
            (WorkloadIdentity::Auction, "payment_profile", "owner"),
            (WorkloadIdentity::Payments, "notification", "recipient"),
            (WorkloadIdentity::Notify, "operation", "owner"),
            (WorkloadIdentity::Processes, "auction", "platform"),
        ];
        let policy = WorkloadPolicy;
        for (caller, resource_type, relation) in cases {
            assert_eq!(
                policy.authorize_relationship(caller, &relationship(resource_type, relation)),
                Err(ApplicationError::Forbidden)
            );
        }
    }

    #[test]
    fn only_bff_can_register_principals() {
        let policy = WorkloadPolicy;
        for caller in [
            WorkloadIdentity::Auction,
            WorkloadIdentity::Catalog,
            WorkloadIdentity::Payments,
            WorkloadIdentity::Notify,
            WorkloadIdentity::Processes,
        ] {
            assert_eq!(
                policy.authorize_principal_registration(caller),
                Err(ApplicationError::Forbidden)
            );
        }
        assert_eq!(
            policy.authorize_principal_registration(WorkloadIdentity::Bff),
            Ok(())
        );
    }
}
