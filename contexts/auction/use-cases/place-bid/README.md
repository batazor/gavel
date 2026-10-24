# Ставка

Контекст: Auction · Актор: участник · Диаграмма: `flow.c4` → `flow_place_bid`

## Цель
Участник задаёт максимум по лоту. Система принимает ставку, пересчитывает лидера и цену, уведомляет подписчиков.

## Предусловия
- Участник аутентифицирован, `user_id` из JWT.
- Auction имеет актуальную проекцию `PaymentInstrumentReady(user_id)` из Payments.
- Торги открыты, `now < ends_at`.

## Основной сценарий

```mermaid
sequenceDiagram
    autonumber
    actor Bidder as Участник
    participant Web
    participant Router as Cosmo Router
    participant Auction as auction-service
    participant Authz
    participant Aggregate as Auction aggregate
    participant DB as auction-db
    participant CDC as Debezium
    participant Broker as Redpanda
    participant Processes
    participant Notify
    participant Realtime
    participant Analytics

    Note over Bidder,Auction: Платёжный инструмент подготовлен до первой ставки
    Bidder->>Web: Задать секретный максимум
    Web->>Router: placeBid(input, idempotencyKey)
    Router->>Auction: PlaceBid(user, maximum)
    Auction->>Auction: Derive stable bid_id<br/>from verified user + auction + key
    Auction->>DB: Lookup idempotency key + fingerprint
    alt Уже committed тот же request
        DB-->>Auction: Stored PlaceBidResult
        Auction-->>Router: Replay без нового события
    else Новая команда
        Auction->>Authz: Check auction#place_bid (deadline, fail-closed)
        Authz-->>Auction: Allowed
    Auction->>DB: Check PaymentInstrumentReady projection
    Auction->>Aggregate: Load authoritative journal/snapshot, version N
    Note over Auction,Aggregate: Public L1/Redis price cache не читается
    Aggregate->>Aggregate: Validate open and maximum > currentPrice<br/>recalculate leader, price, ends_at
    Aggregate->>Aggregate: If leadership is lost:<br/>create private BidderOutbid(recipient, bid_id)
    Aggregate->>DB: Atomic commit: private events + projection<br/>public + restricted outbox + command result, expected version N
    DB-->>Auction: Commit authoritative version
    Auction-->>Router: PlaceBidResult(price, outbid, version)
    Router-->>Web: Authoritative result
    Web-->>Bidder: Accepted or immediately outbid
    CDC->>DB: Capture outbox from WAL
    CDC->>Broker: BidRecorded + optional PriceChanged
    CDC->>Broker: Restricted BidderOutbid<br/>(only when a participant loses leadership)
    Broker-->>Processes: ACL-protected owner event
    Processes->>Notify: Durable idempotent outbid notification
    Notify-->>Bidder: Email «вас перебили»
    Broker-->>Realtime: PriceChanged
    Broker-->>Analytics: BidRecorded
    Realtime-->>Router: Subscription update
    Router-->>Web: price, version
    end
```

## Альтернативы и ошибки
- `max ≤ текущей цены` → GraphQL `DOMAIN_REJECTED`, reason `maximum_not_above_current_price`.
- Торги закрыты → `DOMAIN_REJECTED`, reason `not_open`/`bidding_ended`.
- Конфликт версии (параллельная ставка) → сервис повторяет команду на актуальном состоянии; клиенту возвращается результат пересчёта, не ошибка.
- Повтор с тем же `Idempotency-Key` → тот же детерминированный `bid_id` и сохранённый ответ, без второго события; новое серверное время не меняет canonical fingerprint.
- Тот же key с другим request → `IDEMPOTENCY_CONFLICT`; исчерпан bounded retry → retryable `CONTENTION`.
- Понижение своего максимума → `DOMAIN_REJECTED`, reason `maximum_not_raised`.
- Нет готового способа оплаты → `PAYMENT_METHOD_REQUIRED`; Authz deny → `FORBIDDEN`; timeout/transport Authz → retryable `DEPENDENCY_UNAVAILABLE`.
- Если новый maximum меняет лидера, `BidderOutbid` адресован прежнему лидеру; если proxy maximum удерживает лидерство, событие адресовано автору новой ставки. Повышение maximum текущим лидером событие не создаёт.
- Недоступность Processes/Notify не откатывает принятую ставку: restricted outbox доставляется асинхронно и идемпотентно.

## Инварианты
1–4 из [README контекста](../../README.md). Ключевое для Lean: `apply(state, MaxBidPlaced)` сохраняет «цена не убывает» и «лидер держит наибольший максимум».

## События
`MaxBidPlaced` и `BidderOutbid` (приватные); `BidRecorded` (публичное для каждой принятой ставки); `PriceChanged` и `DeadlineExtended` (публичные при соответствующем изменении). Публичные envelopes уходят только в `gavel.auction.public.v1`; recipient `BidderOutbid` и settlement identity — только в restricted `gavel.auction.processes.v1`. Restricted событие не содержит maximum или identity нового лидера.

## Открытые вопросы
- Сериализация команд по `auction_id` внутри сервиса (актор) для горячих лотов — после нагрузочных тестов.
