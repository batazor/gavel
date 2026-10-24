use std::{env, fmt, net::SocketAddr, str::FromStr};
use url::Url;

use crate::adapters::outbound::SandboxOutcome;

#[derive(Clone)]
pub enum ProviderConfig {
    Stripe {
        endpoint: Url,
        secret: String,
        callback_secret: String,
    },
    Sandbox {
        default_outcome: SandboxOutcome,
        control_token: String,
    },
}

impl fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stripe { endpoint, .. } => f
                .debug_struct("Stripe")
                .field("endpoint", endpoint)
                .field("secret", &"[REDACTED]")
                .field("callback_secret", &"[REDACTED]")
                .finish(),
            Self::Sandbox {
                default_outcome, ..
            } => f
                .debug_struct("Sandbox")
                .field("default_outcome", default_outcome)
                .field("control_token", &"[REDACTED]")
                .finish(),
        }
    }
}

#[derive(Clone)]
pub struct Config {
    pub http_addr: SocketAddr,
    pub grpc_addr: SocketAddr,
    pub database_url: String,
    pub authz_endpoint: String,
    pub provider: ProviderConfig,
    pub fingerprint_secret: String,
    pub processes_spiffe_id: String,
}
impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let runtime_profile =
            env::var("PAYMENTS_RUNTIME_PROFILE").unwrap_or_else(|_| "production".to_owned());
        let adapter = env::var("PAYMENT_PROVIDER_ADAPTER").unwrap_or_else(|_| "stripe".to_owned());
        let provider = match adapter.as_str() {
            "stripe" => ProviderConfig::Stripe {
                endpoint: Url::parse(&required("PAYMENT_PROVIDER_ENDPOINT")?)
                    .map_err(|_| ConfigError("PAYMENT_PROVIDER_ENDPOINT is invalid"))?,
                secret: secret("PAYMENT_PROVIDER_SECRET", 16)?,
                callback_secret: secret("PAYMENT_CALLBACK_SECRET", 16)?,
            },
            "sandbox" if runtime_profile == "local" => ProviderConfig::Sandbox {
                default_outcome: sandbox_outcome(
                    &env::var("PAYMENT_SANDBOX_DEFAULT_OUTCOME")
                        .unwrap_or_else(|_| "success".to_owned()),
                )?,
                control_token: secret("PAYMENT_SANDBOX_CONTROL_TOKEN", 16)?,
            },
            "sandbox" => {
                return Err(ConfigError(
                    "sandbox payment adapter requires local runtime profile",
                ));
            }
            _ => return Err(ConfigError("PAYMENT_PROVIDER_ADAPTER is invalid")),
        };
        Ok(Self {
            http_addr: parse("HTTP_ADDR", "0.0.0.0:8080")?,
            grpc_addr: parse("GRPC_ADDR", "0.0.0.0:9090")?,
            database_url: required("DATABASE_URL")?,
            authz_endpoint: required("AUTHZ_ENDPOINT")?,
            provider,
            fingerprint_secret: secret("PAYMENT_FINGERPRINT_SECRET", 32)?,
            processes_spiffe_id: required("PROCESSES_SPIFFE_ID")?,
        })
    }
}
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("http_addr", &self.http_addr)
            .field("grpc_addr", &self.grpc_addr)
            .field("database_url", &"[REDACTED]")
            .field("authz_endpoint", &self.authz_endpoint)
            .field("provider", &self.provider)
            .field("fingerprint_secret", &"[REDACTED]")
            .field("processes_spiffe_id", &self.processes_spiffe_id)
            .finish()
    }
}

fn sandbox_outcome(value: &str) -> Result<SandboxOutcome, ConfigError> {
    match value {
        "success" => Ok(SandboxOutcome::Success),
        "recovery_then_success" => Ok(SandboxOutcome::RecoveryThenSuccess),
        "expire" => Ok(SandboxOutcome::Expire),
        _ => Err(ConfigError("PAYMENT_SANDBOX_DEFAULT_OUTCOME is invalid")),
    }
}
#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("{0}")]
pub struct ConfigError(&'static str);
fn required(name: &'static str) -> Result<String, ConfigError> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or(ConfigError("required configuration is missing"))
}
fn secret(name: &'static str, minimum: usize) -> Result<String, ConfigError> {
    let value = required(name)?;
    if value.len() < minimum {
        Err(ConfigError("secret configuration is invalid"))
    } else {
        Ok(value)
    }
}
fn parse<T: FromStr>(name: &'static str, default: &'static str) -> Result<T, ConfigError> {
    env::var(name)
        .unwrap_or_else(|_| default.to_owned())
        .parse()
        .map_err(|_| ConfigError("network address is invalid"))
}
