use crate::{
    application::RenderedEmail,
    domain::{NotificationCommand, Template},
};

#[derive(Clone, Copy, Default)]
pub struct SettlementTemplates;

impl SettlementTemplates {
    pub fn render(&self, command: &NotificationCommand) -> Result<RenderedEmail, TemplateError> {
        command.validate().map_err(|_| TemplateError::InvalidData)?;
        let auction_id = required(command, "auction_id")?;
        let (subject, body) = match command.template {
            Template::SettlementCandidateV1 => (
                "Действие по аукциону",
                format!(
                    "Вы выбраны кандидатом на покупку по аукциону {auction_id}. Проверьте статус оплаты."
                ),
            ),
            Template::SettlementCandidateFailedV1 => (
                "Оплата по аукциону не завершена",
                format!(
                    "Время оплаты по аукциону {auction_id} истекло. Аукцион продолжит settlement со следующим кандидатом."
                ),
            ),
            Template::PaymentRecoveryRequiredV1 => {
                let recovery_url = required(command, "recovery_url")?;
                if !(recovery_url.starts_with("https://")
                    || recovery_url.starts_with("http://localhost"))
                {
                    return Err(TemplateError::InvalidData);
                }
                (
                    "Требуется восстановить оплату",
                    format!("Для аукциона {auction_id} завершите оплату: {recovery_url}"),
                )
            }
            Template::WinnerConfirmedV1 => (
                "Покупка подтверждена",
                format!("Оплата по аукциону {auction_id} подтверждена. Вы стали победителем."),
            ),
        };
        Ok(RenderedEmail {
            to: command.recipient_id.clone(),
            subject: subject.into(),
            body,
        })
    }
}

fn required<'a>(command: &'a NotificationCommand, key: &str) -> Result<&'a str, TemplateError> {
    command
        .data
        .get(key)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(TemplateError::MissingData)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TemplateError {
    #[error("template data is missing")]
    MissingData,
    #[error("template data is invalid")]
    InvalidData,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn input(template: Template, data: BTreeMap<String, String>) -> NotificationCommand {
        NotificationCommand {
            notification_id: "n-1".into(),
            recipient_id: "person@example.test".into(),
            template,
            data,
            idempotency_key: "key-1".into(),
        }
    }

    #[test]
    fn renders_winner_v1() {
        let email = SettlementTemplates
            .render(&input(
                Template::WinnerConfirmedV1,
                BTreeMap::from([("auction_id".into(), "a-1".into())]),
            ))
            .unwrap();
        assert!(email.body.contains("a-1"));
        assert!(!email.body.contains("provider"));
    }

    #[test]
    fn renders_recovery_url_only_in_body() {
        let email = SettlementTemplates
            .render(&input(
                Template::PaymentRecoveryRequiredV1,
                BTreeMap::from([
                    ("auction_id".into(), "a-1".into()),
                    (
                        "recovery_url".into(),
                        "https://pay.example.test/recover/secret".into(),
                    ),
                ]),
            ))
            .unwrap();
        assert!(!email.subject.contains("secret"));
        assert!(
            email
                .body
                .contains("https://pay.example.test/recover/secret")
        );
    }

    #[test]
    fn all_settlement_templates_require_auction_id() {
        let error = SettlementTemplates
            .render(&input(Template::WinnerConfirmedV1, BTreeMap::new()))
            .unwrap_err();
        assert_eq!(error, TemplateError::MissingData);
    }

    #[test]
    fn renders_candidate_failure_without_private_payment_data() {
        let email = SettlementTemplates
            .render(&input(
                Template::SettlementCandidateFailedV1,
                BTreeMap::from([("auction_id".into(), "a-1".into())]),
            ))
            .unwrap();
        assert!(email.body.contains("a-1"));
        assert!(!email.body.contains("provider"));
        assert!(!email.body.contains("recovery"));
    }
}
