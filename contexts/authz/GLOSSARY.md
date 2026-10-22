# Authz

Authz is the language of who may perform a capability on a Gavel resource. It is separate from authentication and from domain eligibility.

## Language

**Principal**:
A verified user or workload identity that asks to perform an action.
_Avoid_: Account, actor, JWT user

**Member**:
A verified user currently enabled to invoke ordinary authenticated Gavel capabilities. Membership can be revoked independently of the identity provider session.
_Avoid_: Any JWT subject, customer record

**Resource**:
A domain object against which a permission is checked, identified by its owning context and stable id.
_Avoid_: Endpoint, database row

**Relationship**:
An access-relevant fact connecting a resource to a principal or another resource, such as the seller of a lot.
_Avoid_: Role flag, claim

**Permission**:
A named capability computed from relationships, such as `withdraw` on a lot or `continue_payment` on a payment attempt.
_Avoid_: Role, entitlement flag

**Domain eligibility**:
A business condition evaluated by the owning domain after authorization, such as an open auction or a ready payment instrument.
_Avoid_: Permission

**Authorization decision**:
The result of checking one permission for one principal and one resource at a defined consistency point.
_Avoid_: Authentication result
