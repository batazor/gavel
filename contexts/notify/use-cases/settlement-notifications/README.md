# Settlement-уведомления

Контекст: Notify · Инициатор: Processes · Диаграмма: `flow.c4` → `flow_settlement_notifications`

## Цель

Доставить email при выборе settlement candidate, необходимости восстановить оплату и подтверждении победителя, не связывая workflow с конкретным email provider.

## Сценарий

```mermaid
sequenceDiagram
    autonumber
    participant Process as SettleAuction
    participant Notify
    participant DB as notify-db
    participant Email as SMTP / Mailpit
    participant Operator as MVP smoke

    Process->>Process: Build deterministic notification_id<br/>settlement:{auction}:{round}:{kind}
    Process->>Notify: SendNotification<br/>(id, recipient, template v1, safe data)
    Notify->>DB: Claim id + canonical fingerprint<br/>state=PENDING
    Notify->>Email: Rendered transactional email<br/>(bounded timeout)
    alt Temporary provider error
        Email-->>Notify: Retryable failure
        Notify->>DB: Append safe attempt outcome
        Notify->>Email: Retry with bounded backoff
    else Provider accepted
        Email-->>Notify: Provider reference
    end
    Notify->>DB: Commit SENT or FAILED<br/>+ terminal outbox
    Notify-->>Process: Idempotent notification status
    Operator->>Notify: GetNotification(notification_id)
    Notify->>DB: Read safe delivery projection
    Notify-->>Operator: recipient, template/version,<br/>status, attempt count, version
```

Точный повтор команды возвращает сохранённый terminal result и не создаёт второе письмо. Notify поддерживает отдельные версии `settlement_candidate.v1`, `settlement_candidate_failed.v1`, `payment_recovery_required.v1` и `winner_confirmed.v1`; решение отправить candidate-failed принимает только Processes после authoritative settlement outcome. Повтор `notification_id` с изменённым recipient, template, version, data либо idempotency key возвращает typed `IDEMPOTENCY_CONFLICT`. Operator read никогда не возвращает template data, recovery URL, provider reference или SMTP body.
