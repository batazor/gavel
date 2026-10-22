# Продавец меняет время окончания

Контекст: Auction · Актор: продавец · Диаграмма: `flow.c4` → `flow_reschedule`

## Цель
Продавец переносит плановое окончание торгов в любой момент, пока они открыты.

## Правила
- Требуются authoritative permission `auction#reschedule` и совпадение principal с `seller_id` из `AuctionOpened`; обе проверки fail-closed.
- Новое `scheduled_end` строго больше `now`. В прошлое нельзя.
- Можно и приблизить, и отодвинуть. `hard_deadline` не превышается.
- Тип политики (добавить или убрать `inactivity`, поменять `hard_deadline`) — только до первой ставки. После первой ставки меняется только `scheduled_end`.

## Основной сценарий

```mermaid
sequenceDiagram
    autonumber
    actor Seller as Продавец
    participant Web
    participant Router as Cosmo Router
    participant Auction
    participant Authz
    participant Aggregate as Auction aggregate
    participant DB as auction-db
    participant Broker as Redpanda
    participant Worker as Process worker
    participant Temporal
    participant Realtime

    Seller->>Web: Перенести scheduled_end
    Web->>Router: rescheduleAuction(scheduledEnd, idempotencyKey)
    Router->>Auction: RescheduleAuction(seller, scheduledEnd)
    Auction->>DB: Lookup idempotency key + fingerprint
    Auction->>Authz: Check auction#reschedule
    Authz-->>Auction: Allowed
    Auction->>Aggregate: Validate seller, open state and future time
    Aggregate->>Aggregate: Closing policy recalculates ends_at<br/>without exceeding hard_deadline
    Aggregate-->>Auction: AuctionRescheduled(ends_at)
    Auction->>DB: Atomic commit: event + projection + outbox + command result
    Auction-->>Router: Auction { endsAt }
    Router-->>Web: Authoritative result
    Auction->>Broker: AuctionRescheduled from outbox
    Broker-->>Worker: Deadline changed
    Worker->>Temporal: Reset durable timer
    Broker-->>Realtime: AuctionRescheduled
    Realtime-->>Router: Subscription update
    Router-->>Web: New endsAt
```

## Ошибки
- Authz deny или не продавец → `FORBIDDEN`.
- Торги закрыты → `DOMAIN_REJECTED`, reason `not_open`.
- `scheduled_end ≤ now` → `DOMAIN_REJECTED`, reason `deadline_not_future`.
- Смена типа политики после первой ставки → `DOMAIN_REJECTED`, reason `invalid_closing_policy`.
- Повтор того же request возвращает stored result без нового события; key с другим payload → `IDEMPOTENCY_CONFLICT`.

## Инварианты
6, 7. Изменение `scheduled_end` не затрагивает инварианты цены (1–4).
