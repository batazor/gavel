# Выставить лот и открыть торги

Контекст: Catalog → Auction · Актор: продавец · Диаграмма: `flow.c4` → `flow_list_lot`

## Цель
Продавец создаёт лот с описанием, стартовой ценой, шагом и политикой окончания. Торги открываются автоматически.

## Основной сценарий

```mermaid
sequenceDiagram
    autonumber
    actor Seller as Продавец
    participant Web
    participant Router as Cosmo Router
    participant Catalog
    participant Authz
    participant CatalogDB as catalog-db
    participant CDC as Debezium
    participant Broker as Redpanda
    participant Auction
    participant Worker as Process worker

    Note over Seller,Catalog: 1–20 изображений заранее загружены и имеют READY
    Seller->>Web: Выставить лот
    Web->>Router: listLot(input with imageId + alt, idempotency key)
    Router->>Catalog: ListLot
    Catalog->>Catalog: Validate fields, image owner and READY state
    Catalog->>CatalogDB: Claim HMAC command receipt
    Catalog->>Authz: Check platform:gavel#create_lot<br/>with causal token and deadline
    Authz-->>Catalog: ALLOWED + checked-at revision
    Catalog->>Authz: Atomic TOUCH lot:platform + lot:seller
    Authz-->>Catalog: written-at revision
    Catalog->>CatalogDB: Commit lot + ATTACHED images + LotListed outbox
    CatalogDB-->>Catalog: Lot id
    Catalog-->>Router: Lot { id }
    Router-->>Web: GraphQL result
    CDC->>CatalogDB: Capture outbox from WAL
    CDC->>Broker: LotListed, key = lot_id
    Broker-->>Auction: LotListed, at-least-once
    Auction->>Auction: Idempotent OpenAuction<br/>append AuctionOpened version 1
    Auction->>Broker: AuctionOpened from outbox
    Broker-->>Worker: AuctionOpened
    Worker->>Worker: Start AuctionLifecycle
```

## Ошибки и граничные случаи
- Consumer идемпотентен по `lot_id`: повторная доставка `LotListed` не открывает второй аукцион.
- Точный повтор команды возвращает durable receipt; тот же ключ с другим payload даёт `idempotency_conflict`.
- GraphQL adapter детерминированно выводит `lot_id` из verified principal и idempotency key, поэтому потерянный ответ не превращает точный повтор в новый intent; server timestamp не входит в canonical client payload.
- Если Authz grant записан, а local commit не состоялся, lease-based reconciliation удаляет его только после proof-of-absence лота.
- Метрики используют только bounded `operation/outcome`; seller/lot/image ids, tokens, URLs и object keys в labels/logs запрещены.
- Политика «только `inactivity`» без `hard_deadline` допустима, UI предупреждает, что торги могут длиться долго.

## Между контекстами только события
Catalog не вызывает Auction синхронно. Задержка открытия торгов — миллисекунды, зато нет распределённой транзакции.
