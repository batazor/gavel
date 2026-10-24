use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notification {
    id: String,
    recipient_id: String,
    template: Template,
    fingerprint: String,
    status: NotificationStatus,
    attempts: u32,
    version: u64,
    provider_reference: Option<String>,
    safe_failure: Option<String>,
}

impl Notification {
    pub fn pending(command: &NotificationCommand, fingerprint: String) -> Self {
        Self {
            id: command.notification_id.clone(),
            recipient_id: command.recipient_id.clone(),
            template: command.template,
            fingerprint,
            status: NotificationStatus::Pending,
            attempts: 0,
            version: 1,
            provider_reference: None,
            safe_failure: None,
        }
    }

    pub fn record_attempt(&mut self) -> Result<(), DomainError> {
        self.ensure_pending()?;
        self.attempts = self.attempts.checked_add(1).ok_or(DomainError::Overflow)?;
        self.version = self.version.checked_add(1).ok_or(DomainError::Overflow)?;
        Ok(())
    }

    pub fn mark_sent(&mut self, provider_reference: String) -> Result<(), DomainError> {
        self.ensure_pending()?;
        self.status = NotificationStatus::Sent;
        self.provider_reference = Some(provider_reference);
        self.safe_failure = None;
        self.version = self.version.checked_add(1).ok_or(DomainError::Overflow)?;
        Ok(())
    }

    pub fn mark_failed(&mut self, safe_failure: String) -> Result<(), DomainError> {
        self.ensure_pending()?;
        self.status = NotificationStatus::Failed;
        self.safe_failure = Some(safe_failure);
        self.provider_reference = None;
        self.version = self.version.checked_add(1).ok_or(DomainError::Overflow)?;
        Ok(())
    }

    fn ensure_pending(&self) -> Result<(), DomainError> {
        if self.status == NotificationStatus::Pending {
            Ok(())
        } else {
            Err(DomainError::Terminal)
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn recipient_id(&self) -> &str {
        &self.recipient_id
    }
    pub fn template(&self) -> Template {
        self.template
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
    pub fn status(&self) -> NotificationStatus {
        self.status
    }
    pub fn attempts(&self) -> u32 {
        self.attempts
    }
    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn provider_reference(&self) -> Option<&str> {
        self.provider_reference.as_deref()
    }
    pub fn safe_failure(&self) -> Option<&str> {
        self.safe_failure.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationCommand {
    pub notification_id: String,
    pub recipient_id: String,
    pub template: Template,
    pub data: BTreeMap<String, String>,
    pub idempotency_key: String,
}

impl NotificationCommand {
    pub fn validate(&self) -> Result<(), DomainError> {
        validate_token(&self.notification_id, 160)?;
        validate_token(&self.recipient_id, 320)?;
        validate_token(&self.idempotency_key, 160)?;
        if self.data.len() > 16 {
            return Err(DomainError::InvalidCommand);
        }
        for (key, value) in &self.data {
            validate_key(key)?;
            if value.is_empty() || value.len() > 2048 || value.contains(['\r', '\n']) {
                return Err(DomainError::InvalidCommand);
            }
        }
        Ok(())
    }
}

fn validate_token(value: &str, max: usize) -> Result<(), DomainError> {
    if value.trim() != value
        || value.is_empty()
        || value.len() > max
        || value.chars().any(char::is_control)
    {
        Err(DomainError::InvalidCommand)
    } else {
        Ok(())
    }
}

fn validate_key(value: &str) -> Result<(), DomainError> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        Err(DomainError::InvalidCommand)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NotificationStatus {
    Pending,
    Sent,
    Failed,
}

impl NotificationStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Sent => "sent",
            Self::Failed => "failed",
        }
    }
}

impl fmt::Display for NotificationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_db())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Template {
    SettlementCandidateV1,
    SettlementCandidateFailedV1,
    PaymentRecoveryRequiredV1,
    WinnerConfirmedV1,
}

impl Template {
    pub fn reference(self) -> &'static str {
        match self {
            Self::SettlementCandidateV1 => "settlement_candidate.v1",
            Self::SettlementCandidateFailedV1 => "settlement_candidate_failed.v1",
            Self::PaymentRecoveryRequiredV1 => "payment_recovery_required.v1",
            Self::WinnerConfirmedV1 => "winner_confirmed.v1",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DomainError {
    #[error("invalid notification command")]
    InvalidCommand,
    #[error("notification is already terminal")]
    Terminal,
    #[error("notification counter overflow")]
    Overflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command() -> NotificationCommand {
        NotificationCommand {
            notification_id: "settlement:a-1:1:winner".into(),
            recipient_id: "winner@example.test".into(),
            template: Template::WinnerConfirmedV1,
            data: BTreeMap::from([("auction_id".into(), "a-1".into())]),
            idempotency_key: "settlement:a-1:1:winner".into(),
        }
    }

    #[test]
    fn lifecycle_is_pending_then_terminal() {
        let mut notification = Notification::pending(&command(), "fingerprint".into());
        notification.record_attempt().unwrap();
        notification.mark_sent("mailpit-1".into()).unwrap();
        assert_eq!(notification.status(), NotificationStatus::Sent);
        assert_eq!(notification.attempts(), 1);
        assert_eq!(
            notification.mark_failed("rejected".into()),
            Err(DomainError::Terminal)
        );
    }

    #[test]
    fn command_rejects_log_injection() {
        let mut input = command();
        input
            .data
            .insert("recovery_url".into(), "https://safe.test/\nsecret".into());
        assert_eq!(input.validate(), Err(DomainError::InvalidCommand));
    }
}
