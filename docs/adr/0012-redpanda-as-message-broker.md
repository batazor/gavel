# 12. Redpanda as the message broker (Kafka API)

Date: 2026-10-21

## Status

Accepted. Закрывает кандидата «Брокер» из VISION.md.

## Context

Брокер нужен для [outbox-публикации](0005-transactional-outbox-for-integration.md), GraphQL subscription fan-out через Realtime subgraph ([BFF init](../../contexts/bff/adr/0001-init.md)), прямого чтения в ClickHouse ([Analytics init](../../contexts/analytics/adr/0001-init.md)) и сигналов в Temporal ([Processes init](../../contexts/processes/adr/0001-init.md)). Рассматривали Apache Kafka, Redpanda и NATS JetStream.

Решающие факторы: ClickHouse Kafka engine самый зрелый; event-driven проекции хотят журнал с retention и переигрыванием; в kind важен вес.

## Decision

- **Протокол — Kafka API.** Все клиенты (Rust `rdkafka`, ClickHouse Kafka engine, process worker) работают по Kafka-протоколу и от реализации не зависят.
- **Реализация — Redpanda.** Один бинарник без JVM и ZooKeeper, тот же API. В kind и в production одно и то же.
- **Топики по контексту-владельцу:** `auction.events`, `catalog.events`, `payments.events`, `notify.events`. Ключ сообщения — id агрегата, так порядок событий одного аукциона гарантирован внутри партиции.
- **Retention — долгий** (недели, потом по объёму): топики служат источником для переигрывания проекций и ClickHouse.
- **Схема событий** — публичный контракт. Формат и реестр схем (JSON Schema vs Protobuf, Redpanda Schema Registry) — отдельный ADR.

## Consequences

- Apache Kafka остаётся запасным вариантом без изменений в коде: смена реализации, не протокола.
- Приватные события (`MaxBidPlaced`) в топики не попадают: outbox публикует только публичную проекцию.
- Consumer group на подписчика: `bff-realtime`, `process-worker`, `clickhouse`. At-least-once, подписчики идемпотентны.
- Redpanda Console можно поднять в kind для отладки топиков, в production не обязателен.
