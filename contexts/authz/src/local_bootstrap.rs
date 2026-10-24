use std::{collections::BTreeMap, env, time::Duration};

use serde::Serialize;
use tonic::{Request, metadata::MetadataValue};

use crate::rpc::{
    self, authz_service_client::AuthzServiceClient, register_principal_response,
    write_relationships_response,
};

const IDENTITY_METADATA: &str = "x-gavel-spiffe-id";
const PLATFORM_ID: &str = "gavel";
const PROCESSES_ID: &str = "processes";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalBootstrapConfig {
    pub endpoint: String,
    pub spiffe_id: String,
    pub issuer: String,
    pub actors: Vec<SeedActor>,
    pub request_timeout: Duration,
    pub connect_attempts: u32,
}

impl LocalBootstrapConfig {
    pub fn from_env() -> Result<Self, BootstrapError> {
        if env::var("AUTHZ_BOOTSTRAP_LOCAL_ONLY").as_deref() != Ok("true") {
            return Err(BootstrapError::LocalGuard);
        }
        let config = Self {
            endpoint: required("AUTHZ_BOOTSTRAP_ENDPOINT")?,
            spiffe_id: required("AUTHZ_BOOTSTRAP_SPIFFE_ID")?,
            issuer: required("AUTHZ_BOOTSTRAP_ISSUER")?,
            actors: vec![
                SeedActor::new("seller", required("AUTHZ_BOOTSTRAP_SELLER_SUBJECT")?),
                SeedActor::new(
                    "bidder-one",
                    required("AUTHZ_BOOTSTRAP_BIDDER_ONE_SUBJECT")?,
                ),
                SeedActor::new(
                    "bidder-two",
                    required("AUTHZ_BOOTSTRAP_BIDDER_TWO_SUBJECT")?,
                ),
            ],
            request_timeout: Duration::from_millis(parse("AUTHZ_BOOTSTRAP_TIMEOUT_MS", 5_000)?),
            connect_attempts: parse("AUTHZ_BOOTSTRAP_CONNECT_ATTEMPTS", 30)?,
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), BootstrapError> {
        if !self.endpoint.starts_with("http://")
            || !self.spiffe_id.starts_with("spiffe://")
            || !self.spiffe_id.ends_with("/gavel-authz-bootstrap")
            || self.issuer.is_empty()
            || self.request_timeout.is_zero()
            || self.connect_attempts == 0
            || self.connect_attempts > 120
            || self.actors.len() != 3
        {
            return Err(BootstrapError::InvalidConfig);
        }
        let unique_subjects = self
            .actors
            .iter()
            .map(|actor| actor.subject.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if unique_subjects.len() != self.actors.len() {
            return Err(BootstrapError::InvalidConfig);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeedActor {
    pub logical_name: &'static str,
    pub subject: String,
}

impl SeedActor {
    fn new(logical_name: &'static str, subject: String) -> Self {
        Self {
            logical_name,
            subject,
        }
    }

    fn idempotency_key(&self) -> String {
        format!("local-mvp-authz-register-{}-v1", self.logical_name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BootstrapReport {
    pub actors: BTreeMap<String, String>,
    pub process_workload: String,
    pub permissions_verified: usize,
}

pub async fn run_from_env() -> Result<BootstrapReport, BootstrapError> {
    run(LocalBootstrapConfig::from_env()?).await
}

pub async fn run(config: LocalBootstrapConfig) -> Result<BootstrapReport, BootstrapError> {
    config.validate()?;
    let mut client = connect(&config).await?;
    let mut actors = BTreeMap::new();
    let mut actor_tokens = BTreeMap::new();
    for actor in &config.actors {
        let response = client
            .register_principal(authenticated(
                &config,
                rpc::RegisterPrincipalRequest {
                    issuer: config.issuer.clone(),
                    subject: actor.subject.clone(),
                    idempotency_key: actor.idempotency_key(),
                },
            )?)
            .await?
            .into_inner();
        let registered = match response.outcome {
            Some(register_principal_response::Outcome::Registered(value)) => value,
            Some(register_principal_response::Outcome::Rejection(value)) => {
                return Err(BootstrapError::Rejected(value.reason));
            }
            None => return Err(BootstrapError::InvalidResponse),
        };
        if registered.principal_id.is_empty() || registered.written_at_token.is_empty() {
            return Err(BootstrapError::InvalidResponse);
        }
        actors.insert(
            actor.logical_name.to_owned(),
            registered.principal_id.clone(),
        );
        actor_tokens.insert(actor.logical_name, registered.written_at_token);
    }

    let relationship = rpc::Relationship {
        resource: Some(resource("platform", PLATFORM_ID)),
        relation: "process_worker".to_owned(),
        subject: Some(principal("service_account", PROCESSES_ID)),
    };
    let response = client
        .write_relationships(authenticated(
            &config,
            rpc::WriteRelationshipsRequest {
                updates: vec![relationship],
                idempotency_key: "local-mvp-authz-process-worker-v1".to_owned(),
                deletions: vec![],
            },
        )?)
        .await?
        .into_inner();
    let workload_token = match response.outcome {
        Some(write_relationships_response::Outcome::Written(value)) => value.written_at_token,
        Some(write_relationships_response::Outcome::Rejection(value)) => {
            return Err(BootstrapError::Rejected(value.reason));
        }
        None => return Err(BootstrapError::InvalidResponse),
    };
    if workload_token.is_empty() {
        return Err(BootstrapError::InvalidResponse);
    }

    let mut verified = 0;
    for actor in &config.actors {
        let principal_id = actors
            .get(actor.logical_name)
            .ok_or(BootstrapError::InvalidResponse)?;
        let token = actor_tokens
            .get(actor.logical_name)
            .ok_or(BootstrapError::InvalidResponse)?;
        for permission in [
            "create_lot",
            "participate_in_auction",
            "register_payment_instrument",
        ] {
            expect_permission(
                &mut client,
                &config,
                "platform",
                PLATFORM_ID,
                permission,
                "user",
                principal_id,
                token,
                rpc::PermissionDecision::Allowed,
            )
            .await?;
            verified += 1;
        }
    }
    for permission in [
        "operate_catalog",
        "operate_auctions",
        "operate_payments",
        "send_notifications",
    ] {
        expect_permission(
            &mut client,
            &config,
            "platform",
            PLATFORM_ID,
            permission,
            "service_account",
            PROCESSES_ID,
            &workload_token,
            rpc::PermissionDecision::Allowed,
        )
        .await?;
        verified += 1;
    }
    expect_permission(
        &mut client,
        &config,
        "platform",
        PLATFORM_ID,
        "administer",
        "service_account",
        PROCESSES_ID,
        &workload_token,
        rpc::PermissionDecision::Denied,
    )
    .await?;
    verified += 1;

    Ok(BootstrapReport {
        actors,
        process_workload: PROCESSES_ID.to_owned(),
        permissions_verified: verified,
    })
}

#[allow(clippy::too_many_arguments)]
async fn expect_permission(
    client: &mut AuthzServiceClient<tonic::transport::Channel>,
    config: &LocalBootstrapConfig,
    resource_type: &str,
    resource_id: &str,
    permission: &str,
    principal_type: &str,
    principal_id: &str,
    token: &str,
    expected: rpc::PermissionDecision,
) -> Result<(), BootstrapError> {
    let response = client
        .check_permission(authenticated(
            config,
            rpc::CheckPermissionRequest {
                check: Some(rpc::PermissionCheck {
                    resource: Some(resource(resource_type, resource_id)),
                    permission: permission.to_owned(),
                    principal: Some(principal(principal_type, principal_id)),
                }),
                consistency_token: token.to_owned(),
            },
        )?)
        .await?
        .into_inner();
    if response.decision == expected as i32 && !response.checked_at_token.is_empty() {
        Ok(())
    } else {
        Err(BootstrapError::PermissionVerification)
    }
}

async fn connect(
    config: &LocalBootstrapConfig,
) -> Result<AuthzServiceClient<tonic::transport::Channel>, BootstrapError> {
    for attempt in 1..=config.connect_attempts {
        match AuthzServiceClient::connect(config.endpoint.clone()).await {
            Ok(client) => return Ok(client),
            Err(_) if attempt < config.connect_attempts => {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(error) => return Err(BootstrapError::Transport(error.to_string())),
        }
    }
    Err(BootstrapError::Transport(
        "connection attempts exhausted".into(),
    ))
}

fn authenticated<T>(
    config: &LocalBootstrapConfig,
    message: T,
) -> Result<Request<T>, BootstrapError> {
    let mut request = Request::new(message);
    request.metadata_mut().insert(
        IDENTITY_METADATA,
        MetadataValue::try_from(config.spiffe_id.as_str())
            .map_err(|_| BootstrapError::InvalidConfig)?,
    );
    request.set_timeout(config.request_timeout);
    Ok(request)
}

fn resource(resource_type: &str, resource_id: &str) -> rpc::ResourceReference {
    rpc::ResourceReference {
        resource_type: resource_type.to_owned(),
        resource_id: resource_id.to_owned(),
    }
}

fn principal(principal_type: &str, principal_id: &str) -> rpc::PrincipalReference {
    rpc::PrincipalReference {
        principal_type: principal_type.to_owned(),
        principal_id: principal_id.to_owned(),
    }
}

fn required(name: &'static str) -> Result<String, BootstrapError> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or(BootstrapError::Missing(name))
}

fn parse<T: std::str::FromStr>(name: &'static str, default: T) -> Result<T, BootstrapError> {
    match env::var(name) {
        Ok(value) => value.parse().map_err(|_| BootstrapError::InvalidConfig),
        Err(_) => Ok(default),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BootstrapError {
    #[error("local Authz bootstrap is disabled")]
    LocalGuard,
    #[error("required bootstrap configuration {0} is missing")]
    Missing(&'static str),
    #[error("bootstrap configuration is invalid")]
    InvalidConfig,
    #[error("Authz bootstrap transport failed: {0}")]
    Transport(String),
    #[error("Authz owner API rejected bootstrap: {0}")]
    Rejected(String),
    #[error("Authz owner API returned an invalid response")]
    InvalidResponse,
    #[error("Authz bootstrap permission verification failed")]
    PermissionVerification,
    #[error(transparent)]
    Status(Box<tonic::Status>),
}

impl From<tonic::Status> for BootstrapError {
    fn from(value: tonic::Status) -> Self {
        Self::Status(Box::new(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> LocalBootstrapConfig {
        LocalBootstrapConfig {
            endpoint: "http://gavel-authz:9090".into(),
            spiffe_id: "spiffe://cluster.local/ns/gavel-mvp/sa/gavel-authz-bootstrap".into(),
            issuer: "http://local-jwt:8080/default".into(),
            actors: vec![
                SeedActor::new("seller", "seller".into()),
                SeedActor::new("bidder-one", "bidder-one".into()),
                SeedActor::new("bidder-two", "bidder-two".into()),
            ],
            request_timeout: Duration::from_secs(5),
            connect_attempts: 3,
        }
    }

    #[test]
    fn bootstrap_plan_has_stable_distinct_receipt_keys() {
        let config = config();
        config.validate().unwrap();
        let keys = config
            .actors
            .iter()
            .map(SeedActor::idempotency_key)
            .collect::<Vec<_>>();
        assert_eq!(keys.len(), 3);
        assert_eq!(keys[0], "local-mvp-authz-register-seller-v1");
        assert_ne!(keys[0], keys[1]);
        assert_ne!(keys[1], keys[2]);
    }

    #[test]
    fn bootstrap_rejects_duplicate_subjects_and_non_bootstrap_identity() {
        let mut duplicate = config();
        duplicate.actors[2].subject = duplicate.actors[1].subject.clone();
        assert!(matches!(
            duplicate.validate(),
            Err(BootstrapError::InvalidConfig)
        ));
        let mut forged = config();
        forged.spiffe_id = "spiffe://cluster.local/ns/gavel-mvp/sa/gavel-bff".into();
        assert!(matches!(
            forged.validate(),
            Err(BootstrapError::InvalidConfig)
        ));
    }
}
