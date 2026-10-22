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
    participant Email as Email provider
    participant CDC as Debezium
    participant Broker as Redpanda

    Process->>Process: Build deterministic notification_id<br/>settlement:{auction}:{round}:{kind}
    Process->>Notify: SendNotification<br/>(recipient, template, minimal data)
    Notify->>DB: Save logical notification + delivery plan
    Notify->>Email: Rendered transactional email
    alt Temporary provider error
        Email-->>Notify: Retryable failure
        Notify->>Email: Retry with backoff
    else Provider accepted
        Email-->>Notify: Provider reference
    end
    Notify->>DB: Commit Sent or Failed + public outbox
    Notify-->>Process: Idempotent notification status
    CDC->>DB: Capture outbox from WAL
    CDC->>Broker: NotificationSent or NotificationFailed
```

Повтор команды возвращает состояние исходного уведомления и не создаёт второе письмо.
