use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use prost::Message;
use redis::aio::MultiplexedConnection;

use crate::{
    application::{AuctionCache, CacheError, PriceSnapshot},
    rpc::{self, auction_public_event::Payload},
    telemetry::{CacheOutcome, CacheTier, record_cache},
};

const COMPARE_AND_SET: &str = r#"
local current = redis.call('GET', KEYS[1])
if current then
  local decoded = cjson.decode(current)
  if tonumber(decoded.version) >= tonumber(ARGV[1]) then
    return 0
  end
end
redis.call('SET', KEYS[1], ARGV[2], 'PX', ARGV[3])
return 1
"#;

#[derive(Clone)]
pub struct RedisPriceCache {
    connection: MultiplexedConnection,
    key_prefix: Arc<str>,
    timeout: Duration,
    ttl: Duration,
    breaker: Arc<Mutex<CircuitBreaker>>,
}

impl RedisPriceCache {
    pub async fn connect(
        url: &str,
        key_prefix: impl Into<Arc<str>>,
        timeout: Duration,
        failure_threshold: u32,
        cooldown: Duration,
        ttl: Duration,
    ) -> Result<Self, CacheError> {
        let client = redis::Client::open(url).map_err(|_| CacheError::InvalidValue)?;
        let connection = tokio::time::timeout(timeout, client.get_multiplexed_async_connection())
            .await
            .map_err(|_| CacheError::Unavailable)?
            .map_err(|_| CacheError::Unavailable)?;
        Ok(Self {
            connection,
            key_prefix: key_prefix.into(),
            timeout,
            ttl,
            breaker: Arc::new(Mutex::new(CircuitBreaker::new(failure_threshold, cooldown))),
        })
    }

    fn key(&self, auction_id: &str) -> String {
        format!("{}{}", self.key_prefix, auction_id)
    }

    fn before_call(&self) -> Result<(), CacheError> {
        self.breaker
            .lock()
            .map_err(|_| CacheError::Unavailable)?
            .allow()
    }

    fn record_success(&self) -> Result<(), CacheError> {
        self.breaker
            .lock()
            .map_err(|_| CacheError::Unavailable)?
            .success();
        Ok(())
    }

    fn record_failure<T>(&self) -> Result<T, CacheError> {
        self.breaker
            .lock()
            .map_err(|_| CacheError::Unavailable)?
            .failure();
        Err(CacheError::Unavailable)
    }
}

#[async_trait]
impl AuctionCache for RedisPriceCache {
    async fn get(&self, auction_id: &str) -> Result<Option<PriceSnapshot>, CacheError> {
        self.before_call()?;
        let mut connection = self.connection.clone();
        let result = tokio::time::timeout(
            self.timeout,
            redis::cmd("GET")
                .arg(self.key(auction_id))
                .query_async::<Option<String>>(&mut connection),
        )
        .await;
        let payload = match result {
            Ok(Ok(payload)) => {
                self.record_success()?;
                record_cache(
                    CacheTier::Redis,
                    if payload.is_some() {
                        CacheOutcome::Hit
                    } else {
                        CacheOutcome::Miss
                    },
                );
                payload
            }
            Ok(Err(_)) | Err(_) => {
                record_cache(CacheTier::Redis, CacheOutcome::Failure);
                return self.record_failure();
            }
        };
        let Some(payload) = payload else {
            return Ok(None);
        };
        let snapshot: PriceSnapshot =
            serde_json::from_str(&payload).map_err(|_| CacheError::InvalidValue)?;
        if snapshot.auction_id != auction_id || snapshot.currency_code.len() != 3 {
            return Err(CacheError::InvalidValue);
        }
        Ok(Some(snapshot))
    }

    async fn put_if_newer(&self, snapshot: PriceSnapshot) -> Result<bool, CacheError> {
        self.before_call()?;
        let payload = serde_json::to_string(&snapshot).map_err(|_| CacheError::InvalidValue)?;
        let ttl_millis = u64::try_from(self.ttl.as_millis())
            .unwrap_or(u64::MAX)
            .max(1);
        let mut connection = self.connection.clone();
        let result = tokio::time::timeout(
            self.timeout,
            redis::Script::new(COMPARE_AND_SET)
                .key(self.key(&snapshot.auction_id))
                .arg(snapshot.version)
                .arg(payload)
                .arg(ttl_millis)
                .invoke_async::<i32>(&mut connection),
        )
        .await;
        match result {
            Ok(Ok(written)) => {
                self.record_success()?;
                if written == 1 {
                    record_cache(CacheTier::Redis, CacheOutcome::Write);
                }
                Ok(written == 1)
            }
            Ok(Err(_)) | Err(_) => {
                record_cache(CacheTier::Redis, CacheOutcome::Failure);
                self.record_failure()
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectOutcome {
    Applied,
    Ignored,
    Unsupported,
}

pub struct PriceChangedProjector<C> {
    cache: Arc<C>,
}

impl<C: AuctionCache> PriceChangedProjector<C> {
    pub fn new(cache: Arc<C>) -> Self {
        Self { cache }
    }

    pub async fn handle(&self, bytes: &[u8]) -> Result<ProjectOutcome, CacheError> {
        let envelope =
            rpc::AuctionPublicEvent::decode(bytes).map_err(|_| CacheError::InvalidValue)?;
        let Some(Payload::PriceChanged(event)) = envelope.payload else {
            return Ok(ProjectOutcome::Unsupported);
        };
        let price = event.price.ok_or(CacheError::InvalidValue)?;
        if envelope.aggregate_id.trim().is_empty()
            || envelope.aggregate_version == 0
            || price.currency_code.len() != 3
        {
            return Err(CacheError::InvalidValue);
        }
        let updated_at_millis = envelope
            .occurred_at
            .as_ref()
            .and_then(prost_millis)
            .ok_or(CacheError::InvalidValue)?;
        let written = self
            .cache
            .put_if_newer(PriceSnapshot {
                auction_id: envelope.aggregate_id,
                amount_minor: price.minor_units,
                currency_code: price.currency_code,
                version: envelope.aggregate_version,
                updated_at_millis,
            })
            .await?;
        Ok(if written {
            ProjectOutcome::Applied
        } else {
            ProjectOutcome::Ignored
        })
    }
}

fn prost_millis(timestamp: &prost_types::Timestamp) -> Option<u64> {
    if timestamp.seconds < 0 || !(0..1_000_000_000).contains(&timestamp.nanos) {
        return None;
    }
    u64::try_from(timestamp.seconds)
        .ok()?
        .checked_mul(1_000)?
        .checked_add(u64::from(timestamp.nanos as u32) / 1_000_000)
}

struct CircuitBreaker {
    failures: u32,
    threshold: u32,
    cooldown: Duration,
    open_until: Option<Instant>,
}

impl CircuitBreaker {
    fn new(threshold: u32, cooldown: Duration) -> Self {
        Self {
            failures: 0,
            threshold: threshold.max(1),
            cooldown,
            open_until: None,
        }
    }

    fn allow(&mut self) -> Result<(), CacheError> {
        match self.open_until {
            Some(until) if Instant::now() < until => Err(CacheError::Unavailable),
            Some(_) => {
                self.failures = 0;
                self.open_until = None;
                Ok(())
            }
            None => Ok(()),
        }
    }

    fn success(&mut self) {
        self.failures = 0;
        self.open_until = None;
    }

    fn failure(&mut self) {
        self.failures = self.failures.saturating_add(1);
        if self.failures >= self.threshold {
            self.open_until = Some(Instant::now() + self.cooldown);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circuit_opens_and_recovers_after_cooldown() {
        let mut breaker = CircuitBreaker::new(2, Duration::from_millis(10));
        breaker.failure();
        assert_eq!(breaker.allow(), Ok(()));
        breaker.failure();
        assert_eq!(breaker.allow(), Err(CacheError::Unavailable));
        std::thread::sleep(Duration::from_millis(15));
        assert_eq!(breaker.allow(), Ok(()));
    }

    #[tokio::test]
    async fn connection_handshake_obeys_timeout() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(1)).await;
        });
        let result = RedisPriceCache::connect(
            &format!("redis://{address}/"),
            "test:",
            Duration::from_millis(20),
            2,
            Duration::from_secs(1),
            Duration::from_secs(60),
        )
        .await;
        assert!(matches!(result, Err(CacheError::Unavailable)));
        server.abort();
    }
}
