# Снять лот (сага)

Контекст: Catalog + Auction · Актор: продавец · Диаграмма: `flow.c4` → `flow_withdraw_lot`

## Цель
Продавец снимает лот с торгов. Разрешено только пока ставок нет.

## Почему сага
Решение «ставок нет» принадлежит Auction, состояние лота — Catalog. Два контекста, две базы, транзакции между ними нет. Оркестрирует Temporal (`WithdrawLot` в process worker).

## Основной сценарий

```mermaid
sequenceDiagram
    autonumber
    actor Seller as Продавец
    participant Web
    participant Router as Cosmo Router
    participant Processes
    participant Temporal
    participant Auction
    participant Catalog
    participant Authz
    participant CatalogDB as catalog-db
    participant Realtime

    Seller->>Web: Снять лот
    Web->>Router: withdrawLot(lotId)
    Router->>Processes: Start WithdrawLot saga
    Processes->>Temporal: Schedule workflow
    Processes-->>Router: Operation accepted
    Router-->>Web: Async operation
    Temporal-->>Processes: Run CancelAuction activity
    Processes->>Auction: gRPC CancelAuction(lotId)
    alt Ставки уже есть
        Auction-->>Processes: Conflict
        Processes-->>Web: Operation failed, lot remains
    else Ставок нет
        Auction-->>Processes: AuctionCancelled
        Processes->>Catalog: gRPC WithdrawLot(lotId, idempotencyKey, correlationId)<br/>mandatory bounded deadline + trusted SPIFFE
        Catalog->>Authz: Check lot:execute_withdraw as service_account:processes
        Authz-->>Catalog: ALLOWED + checked-at revision
        Catalog->>CatalogDB: Atomic state + receipt + LotWithdrawn outbox
        alt Catalog succeeds
            Catalog-->>Processes: LotWithdrawn
            Auction-->>Realtime: AuctionCancelled event
            Realtime-->>Web: CANCELLED
        else Catalog fails after cancellation
            Catalog-->>Processes: Failure
            Processes->>Auction: gRPC ReopenAuction compensation
        end
    end
```

## Ошибки
- Не продавец → сага отказывает до первого шага.
- Ставка пришла между шагами 1 и 2 → Auction отвечает `409`, сага отказывает. Гонка решается версией потока.
- Catalog принимает RPC только от trusted-proxy SPIFFE identity `gavel-processes`; raw identity metadata игнорируется.
- Exact replay возвращает прежний typed outcome, dependency failures остаются retryable gRPC status.
- Telemetry содержит только bounded operation/outcome и не содержит lot id, idempotency key, token или credential.

## Результат для клиента
Клиент узнаёт итог по subscription или query статуса `Operation`/лота. Синхронного ответа «снято» нет.
