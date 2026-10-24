use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

use crate::{
    application::{
        ChargeRequest, PortError, PrepareInstrumentRequest, PreparedInstrument, ProviderGateway,
        ProviderOutcome,
    },
    domain::ProviderReference,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxOutcome {
    #[default]
    Success,
    RecoveryThenSuccess,
    Expire,
}

#[derive(Clone, Debug)]
struct SandboxOperation {
    outcome: SandboxOutcome,
}

/// Deterministic local provider adapter. Business code only sees `ProviderGateway`.
#[derive(Clone, Debug)]
pub struct SandboxGateway {
    default_outcome: SandboxOutcome,
    outcomes: Arc<RwLock<HashMap<String, SandboxOutcome>>>,
    operations: Arc<RwLock<HashMap<String, SandboxOperation>>>,
}

impl SandboxGateway {
    pub fn new(default_outcome: SandboxOutcome) -> Self {
        Self {
            default_outcome,
            outcomes: Arc::new(RwLock::new(HashMap::new())),
            operations: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn set_outcome(&self, auction_id: &str, round: u32, outcome: SandboxOutcome) {
        self.outcomes
            .write()
            .await
            .insert(provider_key(auction_id, round), outcome);
    }

    pub fn registration_refs(
        session_ref: &str,
    ) -> Result<(ProviderReference, ProviderReference), PortError> {
        Ok((
            reference("customer", session_ref)?,
            reference("instrument", session_ref)?,
        ))
    }

    fn operation_ref(provider_key: &str) -> Result<ProviderReference, PortError> {
        reference("operation", provider_key)
    }

    async fn operation(
        &self,
        operation_ref: &ProviderReference,
    ) -> Result<SandboxOperation, PortError> {
        self.operations
            .read()
            .await
            .get(operation_ref.expose())
            .cloned()
            .ok_or(PortError::NotFound)
    }
}

#[async_trait]
impl ProviderGateway for SandboxGateway {
    async fn prepare(
        &self,
        request: PrepareInstrumentRequest,
    ) -> Result<PreparedInstrument, PortError> {
        let session_ref = reference("session", &request.provider_key)?;
        Ok(PreparedInstrument {
            provider_url: format!(
                "https://payments.local/local/sandbox/instruments/{}/ready",
                session_ref.expose()
            ),
            provider_session_ref: session_ref,
        })
    }

    async fn charge(&self, request: ChargeRequest) -> Result<ProviderOutcome, PortError> {
        let outcome = self
            .outcomes
            .read()
            .await
            .get(&request.provider_key)
            .copied()
            .unwrap_or(self.default_outcome);
        let operation_ref = Self::operation_ref(&request.provider_key)?;
        let outcome = self
            .operations
            .write()
            .await
            .entry(operation_ref.expose().to_owned())
            .or_insert(SandboxOperation { outcome })
            .outcome;
        match outcome {
            SandboxOutcome::Success => Ok(ProviderOutcome::Succeeded { operation_ref }),
            SandboxOutcome::RecoveryThenSuccess | SandboxOutcome::Expire => {
                Ok(ProviderOutcome::RecoveryRequired {
                    recovery_url: format!(
                        "https://payments.local/local/sandbox/recovery/{}",
                        operation_ref.expose()
                    ),
                    operation_ref,
                })
            }
        }
    }

    async fn continue_payment(
        &self,
        operation_ref: &ProviderReference,
        _timeout_millis: u64,
    ) -> Result<ProviderOutcome, PortError> {
        match self.operation(operation_ref).await?.outcome {
            SandboxOutcome::Success | SandboxOutcome::RecoveryThenSuccess => {
                Ok(ProviderOutcome::Succeeded {
                    operation_ref: operation_ref.clone(),
                })
            }
            SandboxOutcome::Expire => Ok(ProviderOutcome::RecoveryRequired {
                operation_ref: operation_ref.clone(),
                recovery_url: format!(
                    "https://payments.local/local/sandbox/recovery/{}",
                    operation_ref.expose()
                ),
            }),
        }
    }

    async fn query_or_cancel(
        &self,
        operation_ref: &ProviderReference,
        _timeout_millis: u64,
    ) -> Result<ProviderOutcome, PortError> {
        match self.operation(operation_ref).await?.outcome {
            SandboxOutcome::Success => Ok(ProviderOutcome::Succeeded {
                operation_ref: operation_ref.clone(),
            }),
            SandboxOutcome::RecoveryThenSuccess | SandboxOutcome::Expire => {
                Ok(ProviderOutcome::Declined)
            }
        }
    }
}

pub fn provider_key(auction_id: &str, round: u32) -> String {
    format!("settlement:{auction_id}:{round}")
}

fn reference(kind: &str, value: &str) -> Result<ProviderReference, PortError> {
    let digest = URL_SAFE_NO_PAD.encode(Sha256::digest(value.as_bytes()));
    ProviderReference::parse(format!("sandbox_{kind}_{digest}")).map_err(|_| PortError::Internal)
}
