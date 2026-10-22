use std::{env, fmt, net::SocketAddr, str::FromStr};
use url::Url;

#[derive(Clone)]
pub struct Config {
    pub http_addr: SocketAddr,
    pub grpc_addr: SocketAddr,
    pub database_url: String,
    pub authz_endpoint: String,
    pub provider_endpoint: Url,
    pub provider_secret: String,
    pub callback_secret: String,
    pub fingerprint_secret: String,
    pub processes_spiffe_id: String,
}
impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            http_addr: parse("HTTP_ADDR", "0.0.0.0:8080")?,
            grpc_addr: parse("GRPC_ADDR", "0.0.0.0:9090")?,
            database_url: required("DATABASE_URL")?,
            authz_endpoint: required("AUTHZ_ENDPOINT")?,
            provider_endpoint: Url::parse(&required("PAYMENT_PROVIDER_ENDPOINT")?)
                .map_err(|_| ConfigError("PAYMENT_PROVIDER_ENDPOINT is invalid"))?,
            provider_secret: secret("PAYMENT_PROVIDER_SECRET", 16)?,
            callback_secret: secret("PAYMENT_CALLBACK_SECRET", 16)?,
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
            .field("provider_endpoint", &self.provider_endpoint)
            .field("provider_secret", &"[REDACTED]")
            .field("callback_secret", &"[REDACTED]")
            .field("fingerprint_secret", &"[REDACTED]")
            .field("processes_spiffe_id", &self.processes_spiffe_id)
            .finish()
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
