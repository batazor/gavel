use std::{collections::BTreeMap, sync::Arc, time::Duration};

use sha2::{Digest, Sha256};

use crate::{
    application::{
        ClaimResult, DeliveryAttempt, DeliveryError, EmailGateway, NotificationRepository,
        PortError, SettlementTemplates,
    },
    domain::{Notification, NotificationCommand, NotificationStatus},
};

pub struct NotifyApplication {
    repository: Arc<dyn NotificationRepository>,
    email: Arc<dyn EmailGateway>,
    templates: SettlementTemplates,
    retry: RetryPolicy,
}

impl NotifyApplication {
    pub fn new(
        repository: Arc<dyn NotificationRepository>,
        email: Arc<dyn EmailGateway>,
        retry: RetryPolicy,
    ) -> Self {
        Self {
            repository,
            email,
            templates: SettlementTemplates,
            retry,
        }
    }

    pub async fn send(
        &self,
        command: NotificationCommand,
    ) -> Result<NotificationView, ApplicationError> {
        command
            .validate()
            .map_err(|_| ApplicationError::InvalidCommand)?;
        // Validate the owner-controlled template input before claiming the durable id. An
        // invalid command must not leave a PENDING row that exact replay could mistake for an
        // accepted delivery.
        let rendered = self
            .templates
            .render(&command)
            .map_err(|_| ApplicationError::InvalidTemplateData)?;
        let fingerprint = canonical_fingerprint(&command)?;
        let notification = Notification::pending(&command, fingerprint);
        match self.repository.claim(notification).await? {
            ClaimResult::Replay(existing) => return Ok(existing.into()),
            ClaimResult::Conflict => return Err(ApplicationError::IdempotencyConflict),
            ClaimResult::Created(_) => {}
        }
        let mut current = self
            .repository
            .get(&command.notification_id)
            .await?
            .ok_or(ApplicationError::Internal)?;
        for attempt in 1..=self.retry.max_attempts {
            current
                .record_attempt()
                .map_err(|_| ApplicationError::Internal)?;
            match self.email.deliver(&rendered).await {
                Ok(receipt) => {
                    current
                        .mark_sent(receipt.provider_reference)
                        .map_err(|_| ApplicationError::Internal)?;
                    self.repository
                        .save_attempt(
                            &current,
                            DeliveryAttempt {
                                number: attempt,
                                outcome: "sent",
                                safe_detail: None,
                            },
                        )
                        .await?;
                    metrics::counter!("gavel_notify_delivery_attempts_total", "template" => command.template.reference(), "outcome" => "sent").increment(1);
                    return Ok(current.into());
                }
                Err(DeliveryError::Permanent(code)) => {
                    current
                        .mark_failed(code.into())
                        .map_err(|_| ApplicationError::Internal)?;
                    self.repository
                        .save_attempt(
                            &current,
                            DeliveryAttempt {
                                number: attempt,
                                outcome: "permanent_failure",
                                safe_detail: Some(code.into()),
                            },
                        )
                        .await?;
                    metrics::counter!("gavel_notify_delivery_attempts_total", "template" => command.template.reference(), "outcome" => "failed").increment(1);
                    return Ok(current.into());
                }
                Err(DeliveryError::Transient(code)) if attempt == self.retry.max_attempts => {
                    current
                        .mark_failed("retry_exhausted".into())
                        .map_err(|_| ApplicationError::Internal)?;
                    self.repository
                        .save_attempt(
                            &current,
                            DeliveryAttempt {
                                number: attempt,
                                outcome: "retry_exhausted",
                                safe_detail: Some(code.into()),
                            },
                        )
                        .await?;
                    metrics::counter!("gavel_notify_delivery_attempts_total", "template" => command.template.reference(), "outcome" => "failed").increment(1);
                    return Ok(current.into());
                }
                Err(DeliveryError::Transient(code)) => {
                    self.repository
                        .save_attempt(
                            &current,
                            DeliveryAttempt {
                                number: attempt,
                                outcome: "transient_failure",
                                safe_detail: Some(code.into()),
                            },
                        )
                        .await?;
                    tokio::time::sleep(self.retry.delay_for(attempt)).await;
                }
            }
        }
        Err(ApplicationError::Internal)
    }

    pub async fn get(
        &self,
        notification_id: &str,
    ) -> Result<Option<NotificationView>, ApplicationError> {
        Ok(self.repository.get(notification_id).await?.map(Into::into))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}
impl RetryPolicy {
    pub fn new(
        max_attempts: u32,
        base_delay: Duration,
        max_delay: Duration,
    ) -> Result<Self, ApplicationError> {
        if max_attempts == 0 || max_attempts > 10 || base_delay > max_delay {
            return Err(ApplicationError::InvalidConfiguration);
        }
        Ok(Self {
            max_attempts,
            base_delay,
            max_delay,
        })
    }
    fn delay_for(self, attempt: u32) -> Duration {
        self.base_delay
            .saturating_mul(2_u32.saturating_pow(attempt.saturating_sub(1)))
            .min(self.max_delay)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct NotificationView {
    pub notification_id: String,
    pub recipient_id: String,
    pub template: String,
    pub status: NotificationStatus,
    pub attempts: u32,
    pub version: u64,
    pub safe_failure: Option<String>,
}
impl From<Notification> for NotificationView {
    fn from(value: Notification) -> Self {
        Self {
            notification_id: value.id().into(),
            recipient_id: value.recipient_id().into(),
            template: value.template().reference().into(),
            status: value.status(),
            attempts: value.attempts(),
            version: value.version(),
            safe_failure: value.safe_failure().map(str::to_owned),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ApplicationError {
    #[error("invalid notification command")]
    InvalidCommand,
    #[error("invalid template data")]
    InvalidTemplateData,
    #[error("notification id already belongs to a different command")]
    IdempotencyConflict,
    #[error("notification is being processed")]
    InProgress,
    #[error("notification was not found")]
    NotFound,
    #[error("dependency is unavailable")]
    DependencyUnavailable,
    #[error("invalid service configuration")]
    InvalidConfiguration,
    #[error("internal notification error")]
    Internal,
}
impl From<PortError> for ApplicationError {
    fn from(value: PortError) -> Self {
        match value {
            PortError::Unavailable => Self::DependencyUnavailable,
            PortError::Conflict => Self::InProgress,
            PortError::Internal => Self::Internal,
        }
    }
}

fn canonical_fingerprint(command: &NotificationCommand) -> Result<String, ApplicationError> {
    #[derive(serde::Serialize)]
    struct Canonical<'a> {
        notification_id: &'a str,
        recipient_id: &'a str,
        template: &'a str,
        data: &'a BTreeMap<String, String>,
        idempotency_key: &'a str,
    }
    let bytes = serde_json::to_vec(&Canonical {
        notification_id: &command.notification_id,
        recipient_id: &command.recipient_id,
        template: command.template.reference(),
        data: &command.data,
        idempotency_key: &command.idempotency_key,
    })
    .map_err(|_| ApplicationError::Internal)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::{DeliveryReceipt, RenderedEmail},
        domain::Template,
    };
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemoryRepo(Mutex<Option<Notification>>);
    #[async_trait]
    impl NotificationRepository for MemoryRepo {
        async fn claim(&self, value: Notification) -> Result<ClaimResult, PortError> {
            let mut slot = self.0.lock().unwrap();
            Ok(match &*slot {
                Some(old) if old.fingerprint() == value.fingerprint() => {
                    ClaimResult::Replay(old.clone())
                }
                Some(_) => ClaimResult::Conflict,
                None => {
                    *slot = Some(value.clone());
                    ClaimResult::Created(value)
                }
            })
        }
        async fn save_attempt(
            &self,
            value: &Notification,
            _: DeliveryAttempt,
        ) -> Result<(), PortError> {
            *self.0.lock().unwrap() = Some(value.clone());
            Ok(())
        }
        async fn get(&self, _: &str) -> Result<Option<Notification>, PortError> {
            Ok(self.0.lock().unwrap().clone())
        }
    }
    #[derive(Default)]
    struct Mailer(Mutex<u32>);
    #[async_trait]
    impl EmailGateway for Mailer {
        async fn deliver(&self, _: &RenderedEmail) -> Result<DeliveryReceipt, DeliveryError> {
            *self.0.lock().unwrap() += 1;
            Ok(DeliveryReceipt {
                provider_reference: "smtp-accepted".into(),
            })
        }
    }
    struct ScriptedMailer(Mutex<VecDeque<Result<DeliveryReceipt, DeliveryError>>>);
    #[async_trait]
    impl EmailGateway for ScriptedMailer {
        async fn deliver(&self, _: &RenderedEmail) -> Result<DeliveryReceipt, DeliveryError> {
            self.0.lock().unwrap().pop_front().unwrap()
        }
    }
    fn command() -> NotificationCommand {
        NotificationCommand {
            notification_id: "n-1".into(),
            recipient_id: "user@example.test".into(),
            template: Template::WinnerConfirmedV1,
            data: BTreeMap::from([("auction_id".into(), "a-1".into())]),
            idempotency_key: "key-1".into(),
        }
    }

    #[tokio::test]
    async fn exact_replay_does_not_send_twice() {
        let repo = Arc::new(MemoryRepo::default());
        let mail = Arc::new(Mailer::default());
        let app = NotifyApplication::new(
            repo,
            mail.clone(),
            RetryPolicy::new(2, Duration::ZERO, Duration::ZERO).unwrap(),
        );
        assert_eq!(
            app.send(command()).await.unwrap().status,
            NotificationStatus::Sent
        );
        assert_eq!(
            app.send(command()).await.unwrap().status,
            NotificationStatus::Sent
        );
        assert_eq!(*mail.0.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn reused_id_with_changed_payload_conflicts() {
        let repo = Arc::new(MemoryRepo::default());
        let mail = Arc::new(Mailer::default());
        let app = NotifyApplication::new(
            repo,
            mail,
            RetryPolicy::new(1, Duration::ZERO, Duration::ZERO).unwrap(),
        );
        app.send(command()).await.unwrap();
        let mut changed = command();
        changed.recipient_id = "other@example.test".into();
        assert_eq!(
            app.send(changed).await,
            Err(ApplicationError::IdempotencyConflict)
        );
    }

    #[tokio::test]
    async fn transient_failure_retries_within_bound() {
        let repo = Arc::new(MemoryRepo::default());
        let mail = Arc::new(ScriptedMailer(Mutex::new(VecDeque::from([
            Err(DeliveryError::Transient("smtp_unavailable")),
            Ok(DeliveryReceipt {
                provider_reference: "smtp-accepted".into(),
            }),
        ]))));
        let app = NotifyApplication::new(
            repo,
            mail,
            RetryPolicy::new(2, Duration::ZERO, Duration::ZERO).unwrap(),
        );
        let view = app.send(command()).await.unwrap();
        assert_eq!(view.status, NotificationStatus::Sent);
        assert_eq!(view.attempts, 2);
    }
}
