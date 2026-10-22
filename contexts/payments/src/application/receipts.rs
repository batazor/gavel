use std::sync::Arc;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use serde::{Serialize, de::DeserializeOwned};
use sha2::Sha256;

use super::{ApplicationError, Clock, ReceiptClaim, ReceiptClaimResult, ReceiptStore};

pub struct CommandFingerprint {
    secret: Vec<u8>,
}
impl CommandFingerprint {
    pub fn new(secret: impl AsRef<[u8]>) -> Result<Self, ApplicationError> {
        let secret = secret.as_ref();
        if secret.len() < 32 {
            return Err(ApplicationError::InvalidInput);
        }
        Ok(Self {
            secret: secret.to_vec(),
        })
    }
    pub fn digest(&self, value: &[u8]) -> String {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.secret).expect("HMAC accepts any key length");
        mac.update(value);
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReceiptDecision<T> {
    Execute { token: String },
    Replay(T),
}

pub struct ReceiptExecutor {
    store: Arc<dyn ReceiptStore>,
    clock: Arc<dyn Clock>,
    fingerprint: CommandFingerprint,
    lease_millis: u64,
}
impl ReceiptExecutor {
    pub fn new(
        store: Arc<dyn ReceiptStore>,
        clock: Arc<dyn Clock>,
        fingerprint: CommandFingerprint,
        lease_millis: u64,
    ) -> Self {
        Self {
            store,
            clock,
            fingerprint,
            lease_millis,
        }
    }
    pub async fn claim<P: Serialize, R: DeserializeOwned>(
        &self,
        scope: &str,
        command: &'static str,
        key: &str,
        payload: &P,
    ) -> Result<ReceiptDecision<R>, ApplicationError> {
        let canonical = serde_json::to_vec(payload).map_err(|_| ApplicationError::Internal)?;
        let claim = ReceiptClaim {
            scope: scope.to_owned(),
            command,
            key_digest: self.fingerprint.digest(key.as_bytes()),
            fingerprint: self.fingerprint.digest(&canonical),
            lease_until_millis: self.clock.now_millis().saturating_add(self.lease_millis),
        };
        match self.store.claim(claim).await? {
            ReceiptClaimResult::Acquired { token } => Ok(ReceiptDecision::Execute { token }),
            ReceiptClaimResult::Replay(bytes) => serde_json::from_slice(&bytes)
                .map(ReceiptDecision::Replay)
                .map_err(|_| ApplicationError::Internal),
            ReceiptClaimResult::Conflict => Err(ApplicationError::IdempotencyConflict),
            ReceiptClaimResult::InProgress => Err(ApplicationError::InProgress),
        }
    }

    pub fn opaque_key(&self, namespace: &str, scope: &str, key: &str) -> String {
        self.fingerprint
            .digest(format!("{namespace}\0{scope}\0{key}").as_bytes())
    }
    pub async fn complete<T: Serialize>(
        &self,
        token: &str,
        result: &T,
    ) -> Result<(), ApplicationError> {
        let bytes = serde_json::to_vec(result).map_err(|_| ApplicationError::Internal)?;
        self.store.complete(token, &bytes).await.map_err(Into::into)
    }
    pub async fn release(&self, token: &str) {
        let _ = self.store.release(token).await;
    }
}
