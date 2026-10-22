# 5. Transactional outbox via Debezium

Date: 2026-10-21

## Status

Accepted

## Context

Проекты разделены по bounded contexts и владеют собственными базами (ADR-0004). Между ними нет общей транзакции, но доменное изменение и публикация факта для других проектов не должны расходиться при сбое.

Способ хранения внутреннего состояния принадлежит проекту: Auction использует event sourcing, Catalog и Payments — обычное состояние. Интеграционный механизм должен работать одинаково для обоих вариантов.

## Decision

- Каждый владелец записывает изменение своего состояния и строку outbox в одной локальной транзакции.
- Kafka Connect запускает Debezium PostgreSQL source connector для каждого физического Postgres shard. Connector читает изменения outbox из WAL и применяет Debezium Outbox Event Router SMT перед публикацией в Redpanda.
- Доставка at-least-once; потребители обязаны быть идемпотентными.
- В outbox попадают только публичные интеграционные события. Приватные события и внутреннее состояние проекта наружу не публикуются.
- Connector захватывает только outbox-таблицы. Прямой CDC event store, aggregate tables и payment tables запрещён: схема БД не становится публичным контрактом, а `MaxBidPlaced(user, maximum)` не может утечь.
- Порядок событий одного агрегата сохраняется через его id как ключ сообщения и монотонную версию.
- Общая форма outbox: `event_id`, `aggregate_type`, `aggregate_id`, `aggregate_version`, `event_type`, `schema_version`, `occurred_at`, `payload`. Kafka key — `aggregate_id`.
- Конкретная модель хранения — event sourcing или CRUD — фиксируется в локальном init проекта.
- Брокер — Redpanda с Kafka API (ADR-0012).
- ClickHouse продолжает читать Redpanda напрямую через Kafka engine; отдельный Kafka Connect sink для Analytics не нужен.

## Consequences

- Не нужны распределённые транзакции между проектами.
- Возможны повторы, поэтому идемпотентность становится частью интерфейса каждого подписчика.
- Схема публичных событий является межпроектным контрактом и должна версионироваться.
- Не нужен собственный relay в каждом Rust-сервисе.
- Требуют мониторинга connector lag, возраст и размер outbox backlog, состояние replication slot, удержанный WAL и failed tasks.
- Остановка connector не блокирует доменную транзакцию: события накапливаются в outbox/WAL. Capacity и alert должны не допустить переполнения диска.
- Outbox очищается retention job только когда connector offset прошёл соответствующий WAL position и истёк safety window; cleanup не входит в доменную транзакцию.

## References

- [Debezium Outbox Event Router](https://debezium.io/documentation/reference/stable/transformations/outbox-event-router.html)
- [Debezium PostgreSQL connector](https://debezium.io/documentation/reference/stable/connectors/postgresql.html)
- [Debezium on Kafka Connect](https://debezium.io/installation/kafka-connect/)
