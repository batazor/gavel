use std::{fmt, time::Duration};

use async_trait::async_trait;
use hmac::{Hmac, Mac};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use url::Url;

use crate::{
    application::{
        ChargeRequest, PortError, PrepareInstrumentRequest, PreparedInstrument, ProviderGateway,
        ProviderOutcome,
    },
    domain::ProviderReference,
};

#[derive(Clone)]
pub struct StripeConfig {
    pub endpoint: Url,
    secret_key: String,
    callback_secret: String,
    pub signature_tolerance_secs: u64,
}
impl StripeConfig {
    pub fn new(
        endpoint: Url,
        secret_key: String,
        callback_secret: String,
        signature_tolerance_secs: u64,
    ) -> Result<Self, PortError> {
        if secret_key.len() < 16 || callback_secret.len() < 16 || signature_tolerance_secs == 0 {
            return Err(PortError::Internal);
        }
        Ok(Self {
            endpoint,
            secret_key,
            callback_secret,
            signature_tolerance_secs,
        })
    }
}
impl fmt::Debug for StripeConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StripeConfig")
            .field("endpoint", &self.endpoint)
            .field("secret_key", &"[REDACTED]")
            .field("callback_secret", &"[REDACTED]")
            .field("signature_tolerance_secs", &self.signature_tolerance_secs)
            .finish()
    }
}

pub struct StripeSignatureVerifier {
    secret: Vec<u8>,
    tolerance_secs: u64,
}
impl StripeSignatureVerifier {
    pub fn new(config: &StripeConfig) -> Self {
        Self {
            secret: config.callback_secret.as_bytes().to_vec(),
            tolerance_secs: config.signature_tolerance_secs,
        }
    }
    pub fn verify(
        &self,
        header: &str,
        raw_body: &[u8],
        now_secs: u64,
    ) -> Result<(), SignatureError> {
        let mut timestamp = None;
        let mut signatures = Vec::new();
        for part in header.split(',') {
            let Some((key, value)) = part.trim().split_once('=') else {
                continue;
            };
            match key {
                "t" => timestamp = value.parse::<u64>().ok(),
                "v1" => signatures.push(decode_hex(value)?),
                _ => {}
            }
        }
        let timestamp = timestamp.ok_or(SignatureError::Malformed)?;
        if now_secs.abs_diff(timestamp) > self.tolerance_secs {
            return Err(SignatureError::Stale);
        }
        let mut signed = timestamp.to_string().into_bytes();
        signed.push(b'.');
        signed.extend_from_slice(raw_body);
        if signatures.iter().any(|signature| {
            Hmac::<Sha256>::new_from_slice(&self.secret).is_ok_and(|mut mac| {
                mac.update(&signed);
                mac.verify_slice(signature).is_ok()
            })
        }) {
            Ok(())
        } else {
            Err(SignatureError::Invalid)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SignatureError {
    #[error("malformed signature")]
    Malformed,
    #[error("stale signature")]
    Stale,
    #[error("invalid signature")]
    Invalid,
}

fn decode_hex(value: &str) -> Result<Vec<u8>, SignatureError> {
    if value.len() % 2 != 0 {
        return Err(SignatureError::Malformed);
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = digit(pair[0])?;
            let low = digit(pair[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}
fn digit(value: u8) -> Result<u8, SignatureError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(SignatureError::Malformed),
    }
}

#[derive(Clone)]
pub struct StripeGateway {
    client: Client,
    config: StripeConfig,
}
impl StripeGateway {
    pub fn new(client: Client, config: StripeConfig) -> Self {
        Self { client, config }
    }
}

#[derive(Serialize)]
struct ProviderRequest<'a> {
    idempotency_key: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    recovery_email: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    customer_ref: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    instrument_ref: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    amount_minor_units: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    currency_code: Option<&'a str>,
}
#[derive(Deserialize)]
struct ProviderResponse {
    status: String,
    operation_ref: Option<String>,
    session_ref: Option<String>,
    url: Option<String>,
}

impl StripeGateway {
    async fn post(
        &self,
        path: &str,
        key: &str,
        payload: &ProviderRequest<'_>,
        timeout_millis: u64,
    ) -> Result<ProviderResponse, PortError> {
        let endpoint = self
            .config
            .endpoint
            .join(path)
            .map_err(|_| PortError::Internal)?;
        let response = self
            .client
            .post(endpoint)
            .bearer_auth(&self.config.secret_key)
            .header("Idempotency-Key", key)
            .timeout(Duration::from_millis(timeout_millis.min(15_000)))
            .json(payload)
            .send()
            .await
            .map_err(|error| {
                let mapped = map_reqwest(error);
                let outcome = if mapped == PortError::Timeout {
                    "timeout"
                } else {
                    "transport_failure"
                };
                metrics::counter!("gavel_payments_provider_requests_total", "outcome" => outcome)
                    .increment(1);
                mapped
            })?;
        if response.status() == StatusCode::CONFLICT {
            return Err(PortError::Conflict);
        }
        if response.status().is_server_error() {
            metrics::counter!("gavel_payments_provider_requests_total", "outcome" => "provider_failure").increment(1);
            return Err(PortError::Unavailable);
        }
        if !response.status().is_success() {
            return Err(PortError::Internal);
        }
        let result = response.json().await.map_err(|_| PortError::Internal);
        metrics::counter!("gavel_payments_provider_requests_total", "outcome" => if result.is_ok() { "success" } else { "malformed" }).increment(1);
        result
    }
}

#[async_trait]
impl ProviderGateway for StripeGateway {
    async fn prepare(
        &self,
        request: PrepareInstrumentRequest,
    ) -> Result<PreparedInstrument, PortError> {
        let response = self
            .post(
                "v1/instrument-sessions",
                &request.provider_key,
                &ProviderRequest {
                    idempotency_key: &request.provider_key,
                    recovery_email: Some(&request.recovery_email),
                    customer_ref: None,
                    instrument_ref: None,
                    amount_minor_units: None,
                    currency_code: None,
                },
                request.timeout_millis,
            )
            .await?;
        Ok(PreparedInstrument {
            provider_url: response.url.ok_or(PortError::Internal)?,
            provider_session_ref: ProviderReference::parse(
                response.session_ref.ok_or(PortError::Internal)?,
            )
            .map_err(|_| PortError::Internal)?,
        })
    }
    async fn charge(&self, request: ChargeRequest) -> Result<ProviderOutcome, PortError> {
        let response = self
            .post(
                "v1/payment-attempts",
                &request.provider_key,
                &ProviderRequest {
                    idempotency_key: &request.provider_key,
                    recovery_email: None,
                    customer_ref: Some(request.customer_ref.expose()),
                    instrument_ref: Some(request.instrument_ref.expose()),
                    amount_minor_units: Some(request.amount_minor_units),
                    currency_code: Some(&request.currency_code),
                },
                request.timeout_millis,
            )
            .await?;
        map_outcome(response)
    }
    async fn continue_payment(
        &self,
        operation_ref: &ProviderReference,
        timeout_millis: u64,
    ) -> Result<ProviderOutcome, PortError> {
        let key = format!("continue:{}", operation_ref.expose());
        let response = self
            .post(
                "v1/payment-attempts/continue",
                &key,
                &ProviderRequest {
                    idempotency_key: &key,
                    recovery_email: None,
                    customer_ref: None,
                    instrument_ref: Some(operation_ref.expose()),
                    amount_minor_units: None,
                    currency_code: None,
                },
                timeout_millis,
            )
            .await?;
        map_outcome(response)
    }
    async fn query_or_cancel(
        &self,
        operation_ref: &ProviderReference,
        timeout_millis: u64,
    ) -> Result<ProviderOutcome, PortError> {
        let key = format!("expire:{}", operation_ref.expose());
        let response = self
            .post(
                "v1/payment-attempts/query-or-cancel",
                &key,
                &ProviderRequest {
                    idempotency_key: &key,
                    recovery_email: None,
                    customer_ref: None,
                    instrument_ref: Some(operation_ref.expose()),
                    amount_minor_units: None,
                    currency_code: None,
                },
                timeout_millis,
            )
            .await?;
        map_outcome(response)
    }
}

fn map_outcome(response: ProviderResponse) -> Result<ProviderOutcome, PortError> {
    let operation_ref = || {
        response
            .operation_ref
            .clone()
            .ok_or(PortError::Internal)
            .and_then(|value| ProviderReference::parse(value).map_err(|_| PortError::Internal))
    };
    match response.status.as_str() {
        "processing" => Ok(ProviderOutcome::Processing {
            operation_ref: operation_ref()?,
        }),
        "recovery_required" => Ok(ProviderOutcome::RecoveryRequired {
            operation_ref: operation_ref()?,
            recovery_url: response.url.ok_or(PortError::Internal)?,
        }),
        "succeeded" => Ok(ProviderOutcome::Succeeded {
            operation_ref: operation_ref()?,
        }),
        "declined" => Ok(ProviderOutcome::Declined),
        _ => Err(PortError::Internal),
    }
}
fn map_reqwest(error: reqwest::Error) -> PortError {
    if error.is_timeout() {
        PortError::Timeout
    } else if error.is_connect() {
        PortError::Unavailable
    } else {
        PortError::Internal
    }
}
