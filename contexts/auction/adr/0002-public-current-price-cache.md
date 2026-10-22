# 2. Version-aware cache публичной текущей цены

Date: 2026-10-22

## Status

Accepted

## Контекст

Auction должен выдерживать global peak `2 000 PlaceBid/s` и до `20 000 currentPrice reads/s`. При proxy bidding наибольший максимум участника является секретом. Публичный query возвращает только вычисленную текущую цену, которая может не измениться после принятой ставки.

Операционная PostgreSQL-проекция Auction остаётся authoritative источником публичной текущей цены. Направлять весь read peak в неё необязательно, но кэш не должен попадать в командный путь или становиться новым источником истины.

## Решение

`currentPrice(auctionId)` обслуживает глубокий read-модуль с одним внешним интерфейсом и двумя внутренними уровнями кэша:

1. **L1** — bounded in-memory cache каждой реплики auction-service. Freshness window — 500 мс. На refresh одного `auction_id` действует singleflight/request coalescing.
2. **L2** — HA Redis deployment, partitionable по `auction_id`.
3. **Fallback** — authoritative current-price projection в leader `auction-db`. Результат opportunistically заполняет L2 и L1.

Кэшируемое значение:

```text
auction_id
amount
currency
version
updated_at
```

`version` — версия Auction stream, на которой получено это значение. Обновление L1 или Redis заменяет сохранённый snapshot только при строго большей входящей версии; равная версия является безопасным no-op. Асинхронный projector использует atomic compare-by-version в Redis. Более старое сообщение или fallback-read не может затереть новую цену.

После commit события и outbox `PriceChanged` попадает в Redpanda. Price-cache projector читает его at-least-once и обновляет Redis. Удаление ключа или delayed double deletion не используются как протокол согласованности: Redis содержит versioned value, а пропуск/задержка обновления исправляется следующей проекцией или fallback.

`PlaceBid`, `CloseAuction` и другие команды никогда не читают L1/Redis для проверки инвариантов. Они загружают authoritative aggregate state из event store/snapshot. Публичный price cache также не заменяет стратегию snapshot восстановления агрегата.

## Staleness и lifecycle

- При здоровых L1/L2 отдельная реплика может отдавать ранее увиденную цену в пределах 500 мс плюс фактический lag projector. Клиент применяет snapshot только если `version` не меньше уже увиденной.
- Redis miss или недоступность Redis не делает Auction недоступным: read-модуль идёт в PostgreSQL-проекцию. Redis circuit breaker не допускает cascade timeout.
- Если устаревший L1 всё же новее ответа L2, reader сохраняет L1 и идёт в authoritative PostgreSQL: downgrade read-model невозможен даже на fallback path.
- После завершения аукциона terminal snapshot хранится дольше активного TTL, затем ключ удаляется lifecycle policy. TTL очищает память, но не является механизмом согласованности.
- GraphQL subscriptions остаются основным способом live-наблюдения. Кэш защищает explicit queries и reconnect/refetch, но не превращает polling в рекомендуемый transport.
- Максимальный допустимый projector lag, L1 capacity, Redis memory/eviction policy и terminal retention фиксируются по benchmark. Eviction не может быть `noeviction` без capacity proof; потеря cache key безопасна благодаря fallback.

## Последствия

- Read traffic перестаёт линейно нагружать PostgreSQL: при hot key L1 создаёт не более одного L2 refresh на freshness window на реплику, независимо от числа одновременных запросов этой реплики.
- Redis получает до одного version-aware write на `PriceChanged`, а не write на каждый public read.
- Цена может быть временно устаревшей, но никогда не используется для принятия ставки; mutation возвращает authoritative результат команды.
- Для проверки решения нужны отдельные метрики L1/L2 hit ratio, singleflight joins, projector lag, fallback RPS, Redis latency/errors и age возвращаемого snapshot.

## Референсы

- [Auction](../README.md)
- [Получить текущую цену](../use-cases/get-current-price/README.md)
- [ADR-0013 — Workload and capacity model](../../../docs/adr/0013-workload-and-capacity-model.md)
- [ADR-0014 — Availability, replication and sharding](../../../docs/adr/0014-availability-replication-and-sharding.md)
