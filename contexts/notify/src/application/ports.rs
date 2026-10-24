use async_trait::async_trait;

use crate::domain::Notification;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortError {
    Unavailable,
    Conflict,
    Internal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClaimResult {
    Created(Notification),
    Replay(Notification),
    Conflict,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliveryAttempt {
    pub number: u32,
    pub outcome: &'static str,
    pub safe_detail: Option<String>,
}

#[async_trait]
pub trait NotificationRepository: Send + Sync {
    async fn claim(&self, notification: Notification) -> Result<ClaimResult, PortError>;
    async fn save_attempt(
        &self,
        notification: &Notification,
        attempt: DeliveryAttempt,
    ) -> Result<(), PortError>;
    async fn get(&self, notification_id: &str) -> Result<Option<Notification>, PortError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedEmail {
    pub to: String,
    pub subject: String,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliveryReceipt {
    pub provider_reference: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeliveryError {
    Transient(&'static str),
    Permanent(&'static str),
}

#[async_trait]
pub trait EmailGateway: Send + Sync {
    async fn deliver(&self, email: &RenderedEmail) -> Result<DeliveryReceipt, DeliveryError>;
}
