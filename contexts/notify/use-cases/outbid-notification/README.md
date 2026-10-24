# Уведомление «вас перебили»

Контекст: Notify · Инициатор: Processes · Authoritative decision: Auction · Диаграмма: `flow.c4` → `flow_outbid_notification`

## Цель

Доставить участнику email после того, как authoritative proxy bidding определил потерю лидерства, не раскрывая maximum или identity другого участника и не связывая commit ставки с email provider.

## Сценарий

```mermaid
sequenceDiagram
    autonumber
    participant Auction
    participant Broker as Redpanda restricted topic
    participant Process as NotifyOutbid workflow
    participant Notify
    participant DB as notify-db
    participant Email as SMTP / Mailpit

    Auction->>Broker: BidderOutbid(recipient, bid_id)<br/>auction + aggregate version в envelope
    Broker-->>Process: signal-with-start<br/>notify-outbid/{auction}/{version}
    Process->>Process: Build notification_id<br/>outbid/{auction}:{version}
    Process->>Notify: SendNotification<br/>(bidder_outbid.v1, auction_id)
    Notify->>DB: Claim id + canonical fingerprint
    Notify->>Email: Rendered email without maximum/leader
    Email-->>Notify: Accepted or bounded failure
    Notify->>DB: Commit SENT | FAILED + attempt
    Notify-->>Process: Safe idempotent acknowledgement
```

Auction определяет recipient: прежний лидер при смене лидерства либо автор принятой ставки при immediate proxy outbid. Processes durable повторяет intent, а Notify дедуплицирует точный повтор. Ошибка доставки не меняет ставку, цену или лидера.

Template принимает только `auction_id`. Recipient, notification id, bid id, тело письма и template data не попадают в telemetry labels или обычные логи.
