use std::{env, fmt, net::SocketAddr, str::FromStr, time::Duration};
#[derive(Clone)]
pub struct Config {
    pub http_addr: SocketAddr,
    pub grpc_addr: SocketAddr,
    pub database_url: String,
    pub smtp_addr: String,
    pub smtp_from: String,
    pub smtp_timeout: Duration,
    pub delivery_attempts: u32,
    pub retry_base: Duration,
    pub retry_max: Duration,
    pub processes_spiffe_id: String,
    pub operator_principal: String,
    pub local_operator_token: Option<String>,
}
impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let attempts = parse("NOTIFY_DELIVERY_ATTEMPTS", "3")?;
        if !(1..=10).contains(&attempts) {
            return Err(ConfigError("NOTIFY_DELIVERY_ATTEMPTS is invalid"));
        }
        Ok(Self {
            http_addr: parse("HTTP_ADDR", "0.0.0.0:8080")?,
            grpc_addr: parse("GRPC_ADDR", "0.0.0.0:9090")?,
            database_url: required("DATABASE_URL")?,
            smtp_addr: smtp_address("SMTP_ADDR", "mailpit:1025")?,
            smtp_from: required("SMTP_FROM")?,
            smtp_timeout: Duration::from_millis(parse("SMTP_TIMEOUT_MILLIS", "3000")?),
            delivery_attempts: attempts,
            retry_base: Duration::from_millis(parse("NOTIFY_RETRY_BASE_MILLIS", "100")?),
            retry_max: Duration::from_millis(parse("NOTIFY_RETRY_MAX_MILLIS", "1000")?),
            processes_spiffe_id: required("PROCESSES_SPIFFE_ID")?,
            operator_principal: required("NOTIFY_OPERATOR_PRINCIPAL")?,
            local_operator_token: optional("NOTIFY_LOCAL_OPERATOR_TOKEN"),
        })
    }
}
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("http_addr", &self.http_addr)
            .field("grpc_addr", &self.grpc_addr)
            .field("database_url", &"[REDACTED]")
            .field("smtp_addr", &self.smtp_addr)
            .field("smtp_from", &self.smtp_from)
            .field("smtp_timeout", &self.smtp_timeout)
            .field("delivery_attempts", &self.delivery_attempts)
            .field("retry_base", &self.retry_base)
            .field("retry_max", &self.retry_max)
            .field("processes_spiffe_id", &self.processes_spiffe_id)
            .field("operator_principal", &self.operator_principal)
            .field(
                "local_operator_token",
                &self.local_operator_token.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}
#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("{0}")]
pub struct ConfigError(&'static str);
fn required(name: &'static str) -> Result<String, ConfigError> {
    env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .ok_or(ConfigError("required configuration is missing"))
}
fn optional(name: &'static str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}
fn smtp_address(name: &'static str, default: &'static str) -> Result<String, ConfigError> {
    let value = env::var(name).unwrap_or_else(|_| default.into());
    let Some((host, port)) = value.rsplit_once(':') else {
        return Err(ConfigError("configuration value is invalid"));
    };
    if host.trim().is_empty()
        || host.contains(['\r', '\n'])
        || port.parse::<u16>().ok().filter(|port| *port > 0).is_none()
    {
        return Err(ConfigError("configuration value is invalid"));
    }
    Ok(value)
}
fn parse<T: FromStr>(name: &'static str, default: &'static str) -> Result<T, ConfigError> {
    env::var(name)
        .unwrap_or_else(|_| default.into())
        .parse()
        .map_err(|_| ConfigError("configuration value is invalid"))
}
