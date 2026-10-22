# Получить текущую публичную цену

Контекст: Auction · Актор: участник или анонимный зритель · Диаграмма: `flow.c4` → `flow_get_current_price`

## Цель

Вернуть публичную текущую цену аукциона с версией snapshot за P99 `< 200 мс`. Секретные максимумы участников в этот read model не попадают.

## Основной сценарий

```mermaid
sequenceDiagram
    autonumber
    actor Viewer as Зритель
    participant Web
    participant Router as Cosmo Router
    participant Auction as Auction currentPrice
    participant L1 as L1 in-memory
    participant Redis as Redis L2
    participant DB as auction-db leader

    Viewer->>Web: Открыть или обновить цену
    Web->>Router: currentPrice(auctionId)
    Router->>Auction: currentPrice(auctionId)
    Auction->>L1: Получить snapshot

    alt L1 свежий, age < 500 мс
        L1-->>Auction: amount, currency, version
    else L1 отсутствует или устарел
        Auction->>Redis: GET auctionId<br/>(singleflight на реплике)
        alt Redis hit, version не меньше L1
            Redis-->>Auction: amount, currency, version
            Auction->>L1: Сохранить по version
        else Redis miss, timeout или open circuit
            Auction->>DB: Прочитать authoritative projection
            DB-->>Auction: amount, currency, version
            Auction-->>Redis: CAS, только если version новее
            Auction->>L1: Сохранить по version
        end
    end

    Auction-->>Router: CurrentPrice
    Router-->>Web: amount, currency, version
    Web-->>Viewer: Обновить только если version не уменьшилась
```

Ошибка заполнения Redis не превращает успешное чтение PostgreSQL в ошибку пользователя. При L1 miss/stale конкурентные запросы одного `auction_id` объединяются через singleflight.
Если Redis вернул snapshot старее уже известного L1, reader не понижает версию и сверяется с authoritative PostgreSQL. Redis timeout и открытый circuit breaker следуют тому же fallback path.

## Обновление L2

После commit `PriceChanged` публикуется через outbox. Price-cache projector читает событие at-least-once и выполняет atomic compare-by-version в Redis. Повторная или запоздалая версия безопасно игнорируется.

## Инварианты

- «Текущая цена» — публичная вычисленная цена, не секретный максимум лидера.
- Cache state не участвует в `PlaceBid` и других командах.
- Значение с меньшей `version` никогда не заменяет значение с большей версией.
- Cache miss всегда восстанавливается из authoritative проекции.
- Subscription остаётся основным live transport; query предназначен для initial load, explicit refresh и восстановления.
